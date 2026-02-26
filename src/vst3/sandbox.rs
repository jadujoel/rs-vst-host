//! Plugin crash sandbox: signal-handler-based recovery for VST3 plugin calls.
//!
//! Uses `sigsetjmp`/`siglongjmp` to recover from crashes (SIGBUS, SIGSEGV,
//! SIGABRT, SIGFPE) in plugin COM methods. This allows the host to survive
//! buggy plugins that crash during shutdown, processing, or other operations.
//!
//! # How it works
//!
//! 1. `sandbox_call` installs custom signal handlers for crashable signals
//! 2. `sigsetjmp` saves the current execution context (registers, signal mask)
//! 3. The plugin function is called normally
//! 4. If the plugin triggers a fatal signal, the signal handler fires
//! 5. The handler calls `siglongjmp` to return to the `sigsetjmp` point
//! 6. `sandbox_call` returns `SandboxResult::Crashed` instead of terminating
//! 7. Previous signal handlers are restored
//!
//! # Thread safety
//!
//! Each thread has its own jump buffer and active flag (thread-local storage).
//! Signal handlers for SIGBUS/SIGSEGV are delivered to the offending thread,
//! so the correct thread-local jump buffer is always used.
//!
//! Signal handler installation uses reference counting so that concurrent
//! sandbox calls on different threads share a single handler installation.

use std::cell::{Cell, UnsafeCell};
use std::fmt;
use std::panic::AssertUnwindSafe;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use tracing::{error, warn};

// ── Platform-specific FFI for sigjmp_buf / sigsetjmp / siglongjmp ───────────
//
// On macOS, the libc crate does not expose sigjmp_buf, sigsetjmp, or
// siglongjmp. We declare the types and functions directly from <setjmp.h>.

/// `sigjmp_buf` on macOS (ARM64 and x86_64): `int[(14+8+2)*2 + 1]` = `int[49]`.
#[cfg(target_os = "macos")]
type SigJmpBuf = [libc::c_int; 49];

/// `sigjmp_buf` on Linux: matches glibc's `__jmp_buf_tag[1]`.
#[cfg(target_os = "linux")]
type SigJmpBuf = libc::sigjmp_buf;

unsafe extern "C" {
    /// Save the calling environment (registers + signal mask) for later
    /// restoration by `siglongjmp`. Returns 0 on direct call, non-zero
    /// when returning via `siglongjmp`.
    #[cfg(target_os = "macos")]
    fn sigsetjmp(env: *mut SigJmpBuf, savemask: libc::c_int) -> libc::c_int;

    /// Restore the environment saved by `sigsetjmp`, making `sigsetjmp`
    /// return `val`. The signal mask saved by `sigsetjmp` is restored.
    #[cfg(target_os = "macos")]
    fn siglongjmp(env: *mut SigJmpBuf, val: libc::c_int) -> !;
}

// On Linux, use the libc crate's definitions directly.
#[cfg(target_os = "linux")]
use libc::{siglongjmp, sigsetjmp};

// ── Result types ────────────────────────────────────────────────────────────

/// Result of a sandboxed plugin call.
#[derive(Debug)]
pub enum SandboxResult<T> {
    /// Call completed successfully.
    Ok(T),
    /// Plugin crashed with a signal.
    Crashed(PluginCrash),
    /// Plugin triggered a Rust panic.
    Panicked(String),
}

impl<T> SandboxResult<T> {
    /// Returns true if the call succeeded.
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok(_))
    }

    /// Returns true if the plugin crashed.
    pub fn is_crashed(&self) -> bool {
        matches!(self, Self::Crashed(_))
    }

    /// Returns true if the plugin panicked.
    #[allow(dead_code)]
    pub fn is_panicked(&self) -> bool {
        matches!(self, Self::Panicked(_))
    }

    /// Unwrap the success value, panicking if crashed or panicked.
    #[allow(dead_code)]
    pub fn unwrap(self) -> T {
        match self {
            Self::Ok(v) => v,
            Self::Crashed(c) => panic!("Plugin crashed: {}", c),
            Self::Panicked(msg) => panic!("Plugin panicked: {}", msg),
        }
    }

    /// Extract the success value if present.
    #[allow(dead_code)]
    pub fn ok(self) -> Option<T> {
        match self {
            Self::Ok(v) => Some(v),
            _ => None,
        }
    }
}

/// Information about a plugin crash caught by the sandbox.
#[derive(Debug, Clone)]
pub struct PluginCrash {
    /// The signal number (e.g., libc::SIGBUS).
    pub signal: i32,
    /// Human-readable signal name.
    pub signal_name: String,
    /// Description of what the plugin was doing when it crashed.
    pub context: String,
}

impl fmt::Display for PluginCrash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Plugin crashed ({}) during {}",
            self.signal_name, self.context
        )
    }
}

impl std::error::Error for PluginCrash {}

// ── Thread-local state for signal recovery ──────────────────────────────────

thread_local! {
    /// Whether a sandbox is currently active on this thread.
    static SANDBOX_ACTIVE: Cell<bool> = const { Cell::new(false) };
    /// The signal number that caused the crash (set by the signal handler).
    static CRASH_SIGNAL: Cell<i32> = const { Cell::new(0) };
    /// The jump buffer for `siglongjmp` recovery.
    /// Initialized to zeroes; set by `sigsetjmp` before each sandbox call.
    /// Uses `const` init so `.with()` is signal-safe (no lazy-init machinery).
    static JUMP_BUF: UnsafeCell<SigJmpBuf> = const {
        // Safety: SigJmpBuf is [c_int; N], zero-initialized is valid.
        unsafe { UnsafeCell::new(std::mem::zeroed()) }
    };
}

// ── Signal handler management ───────────────────────────────────────────────

/// Signals that we intercept during sandboxed plugin calls.
const SANDBOX_SIGNALS: &[libc::c_int] = &[libc::SIGBUS, libc::SIGSEGV, libc::SIGABRT, libc::SIGFPE];

/// Reference count for handler installations. When >0, our handlers are
/// installed. This prevents races when multiple threads use sandbox_call
/// concurrently.
static HANDLER_REFCOUNT: AtomicU32 = AtomicU32::new(0);

/// Saved original signal handlers (before we installed ours).
/// Protected by mutex; only accessed during install/restore transitions.
static SAVED_HANDLERS: Mutex<Vec<(libc::c_int, libc::sigaction)>> = Mutex::new(Vec::new());

/// Signal handler: recovers from plugin crashes via `siglongjmp`.
///
/// If a sandbox is active on the current thread, this stores the signal
/// number and jumps back to the `sigsetjmp` point. If no sandbox is active,
/// it re-raises the signal with the default handler.
extern "C" fn sandbox_signal_handler(sig: libc::c_int) {
    // Check if this thread has an active sandbox
    let is_active = SANDBOX_ACTIVE.with(|active| active.get());

    if is_active {
        CRASH_SIGNAL.with(|s| s.set(sig));
        JUMP_BUF.with(|buf| {
            // Safety: buf.get() points to a valid SigJmpBuf that was
            // initialized by sigsetjmp on this thread.
            unsafe {
                siglongjmp(buf.get(), 1);
            }
        });
        // siglongjmp never returns
    }

    // No sandbox active — restore default handler and re-raise the signal
    // so the process terminates normally with the correct signal.
    unsafe {
        libc::signal(sig, libc::SIG_DFL);
        libc::raise(sig);
    }
}

/// Acquire signal handlers: increment refcount and install on first acquisition.
fn acquire_signal_handlers() {
    let prev = HANDLER_REFCOUNT.fetch_add(1, Ordering::SeqCst);
    if prev == 0 {
        // First caller — install our handlers and save the originals
        let mut saved = SAVED_HANDLERS.lock().unwrap();
        *saved = install_handlers_impl();
    }
}

/// Release signal handlers: decrement refcount and restore on last release.
fn release_signal_handlers() {
    let prev = HANDLER_REFCOUNT.fetch_sub(1, Ordering::SeqCst);
    if prev == 1 {
        // Last caller — restore original handlers
        let saved = std::mem::take(&mut *SAVED_HANDLERS.lock().unwrap());
        restore_handlers_impl(&saved);
    }
}

/// Install our signal handlers, returning the previous handlers.
fn install_handlers_impl() -> Vec<(libc::c_int, libc::sigaction)> {
    let mut old_handlers = Vec::with_capacity(SANDBOX_SIGNALS.len());

    for &sig in SANDBOX_SIGNALS {
        unsafe {
            let mut old_action: libc::sigaction = std::mem::zeroed();
            let mut new_action: libc::sigaction = std::mem::zeroed();
            new_action.sa_sigaction = sandbox_signal_handler as libc::sighandler_t;
            new_action.sa_flags = 0;
            libc::sigemptyset(&mut new_action.sa_mask);

            if libc::sigaction(sig, &new_action, &mut old_action) == 0 {
                old_handlers.push((sig, old_action));
            }
        }
    }

    old_handlers
}

/// Restore saved signal handlers.
fn restore_handlers_impl(old_handlers: &[(libc::c_int, libc::sigaction)]) {
    for &(sig, ref old_action) in old_handlers {
        unsafe {
            libc::sigaction(sig, old_action, std::ptr::null_mut());
        }
    }
}

/// Human-readable signal name.
fn signal_name(sig: i32) -> &'static str {
    match sig {
        libc::SIGBUS => "SIGBUS (bus error)",
        libc::SIGSEGV => "SIGSEGV (segmentation fault)",
        libc::SIGABRT => "SIGABRT (abort)",
        libc::SIGFPE => "SIGFPE (floating point exception)",
        _ => "UNKNOWN",
    }
}

// ── Public API ──────────────────────────────────────────────────────────────

/// Execute a closure inside a crash sandbox.
///
/// Catches SIGBUS, SIGSEGV, SIGABRT, and SIGFPE signals triggered by the
/// closure and returns `SandboxResult::Crashed` instead of terminating.
/// Also catches Rust panics via `catch_unwind`.
///
/// # Arguments
///
/// * `context` — Description of the operation (for logging on crash).
/// * `f` — The closure to execute. Should ideally only contain FFI/COM calls;
///          avoid creating Rust objects with Drop impls inside the closure,
///          as their destructors may be skipped on signal recovery.
///
/// # Thread safety
///
/// Thread-safe. Each thread tracks its own sandbox state via TLS.
/// Nested calls are detected and run directly inside the outermost sandbox.
pub fn sandbox_call<F, R>(context: &str, f: F) -> SandboxResult<R>
where
    F: FnOnce() -> R,
{
    // If already inside a sandbox on this thread, run directly.
    // The outermost sandbox's sigsetjmp handles recovery.
    let already_active = SANDBOX_ACTIVE.with(|a| a.get());
    if already_active {
        return match std::panic::catch_unwind(AssertUnwindSafe(f)) {
            Ok(v) => SandboxResult::Ok(v),
            Err(e) => SandboxResult::Panicked(panic_message(&e)),
        };
    }

    // Install signal handlers (ref-counted; first caller actually installs)
    acquire_signal_handlers();

    let result = JUMP_BUF.with(|buf| {
        // Save the current execution context. If a signal fires during f(),
        // siglongjmp returns here with val != 0.
        let jmp_val = unsafe { sigsetjmp(buf.get(), 1) };

        if jmp_val == 0 {
            // ── Normal execution path ──
            SANDBOX_ACTIVE.with(|a| a.set(true));

            let call_result = std::panic::catch_unwind(AssertUnwindSafe(f));

            SANDBOX_ACTIVE.with(|a| a.set(false));

            match call_result {
                Ok(value) => SandboxResult::Ok(value),
                Err(e) => {
                    let msg = panic_message(&e);
                    warn!(context = context, panic = %msg, "Plugin panicked inside sandbox");
                    SandboxResult::Panicked(msg)
                }
            }
        } else {
            // ── Signal recovery path ── (reached via siglongjmp)
            SANDBOX_ACTIVE.with(|a| a.set(false));

            let sig = CRASH_SIGNAL.with(|s| s.get());
            let name = signal_name(sig);

            error!(
                signal = name,
                context = context,
                "Plugin crashed — recovered via sandbox"
            );

            SandboxResult::Crashed(PluginCrash {
                signal: sig,
                signal_name: name.to_string(),
                context: context.to_string(),
            })
        }
    });

    // Release signal handlers (ref-counted; last caller actually restores)
    release_signal_handlers();

    result
}

/// Extract a human-readable message from a caught panic payload.
fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_call_normal_returns_value() {
        let result = sandbox_call("test", || 42);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_sandbox_call_returns_unit() {
        let result = sandbox_call("unit", || {});
        assert!(result.is_ok());
    }

    #[test]
    fn test_sandbox_call_with_side_effects() {
        let mut value = 0;
        let result = sandbox_call("side_effect", || {
            value = 42;
            value
        });
        assert_eq!(result.unwrap(), 42);
        assert_eq!(value, 42);
    }

    #[test]
    fn test_sandbox_call_panic_recovery() {
        let result: SandboxResult<i32> = sandbox_call("test_panic", || {
            panic!("test panic");
        });
        assert!(result.is_panicked());
        match result {
            SandboxResult::Panicked(msg) => {
                assert!(msg.contains("test panic"));
            }
            _ => panic!("Expected Panicked"),
        }
    }

    #[test]
    fn test_sandbox_nested_calls() {
        let result = sandbox_call("outer", || {
            let inner = sandbox_call("inner", || 99);
            inner.unwrap()
        });
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 99);
    }

    #[test]
    fn test_sandbox_nested_with_inner_panic() {
        let result: SandboxResult<i32> = sandbox_call("outer", || {
            let inner: SandboxResult<i32> = sandbox_call("inner", || {
                panic!("inner panic");
            });
            match inner {
                SandboxResult::Panicked(_) => 100,
                _ => 0,
            }
        });
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 100);
    }

    #[test]
    fn test_sandbox_result_is_ok() {
        let ok: SandboxResult<i32> = SandboxResult::Ok(5);
        assert!(ok.is_ok());
        assert!(!ok.is_crashed());
        assert!(!ok.is_panicked());
    }

    #[test]
    fn test_sandbox_result_is_crashed() {
        let crashed: SandboxResult<i32> = SandboxResult::Crashed(PluginCrash {
            signal: libc::SIGBUS,
            signal_name: "SIGBUS".into(),
            context: "test".into(),
        });
        assert!(!crashed.is_ok());
        assert!(crashed.is_crashed());
        assert_eq!(crashed.ok(), None);
    }

    #[test]
    fn test_sandbox_result_is_panicked() {
        let panicked: SandboxResult<i32> = SandboxResult::Panicked("boom".into());
        assert!(!panicked.is_ok());
        assert!(panicked.is_panicked());
        assert_eq!(panicked.ok(), None);
    }

    #[test]
    fn test_plugin_crash_display() {
        let crash = PluginCrash {
            signal: libc::SIGBUS,
            signal_name: "SIGBUS".into(),
            context: "shutdown".into(),
        };
        let s = format!("{}", crash);
        assert!(s.contains("SIGBUS"));
        assert!(s.contains("shutdown"));
    }

    #[test]
    fn test_plugin_crash_is_error() {
        let crash = PluginCrash {
            signal: libc::SIGSEGV,
            signal_name: "SIGSEGV".into(),
            context: "process".into(),
        };
        // Verify it implements std::error::Error
        let _err: &dyn std::error::Error = &crash;
    }

    #[test]
    fn test_signal_name_known_signals() {
        assert_eq!(signal_name(libc::SIGBUS), "SIGBUS (bus error)");
        assert_eq!(signal_name(libc::SIGSEGV), "SIGSEGV (segmentation fault)");
        assert_eq!(signal_name(libc::SIGABRT), "SIGABRT (abort)");
        assert_eq!(
            signal_name(libc::SIGFPE),
            "SIGFPE (floating point exception)"
        );
    }

    #[test]
    fn test_signal_name_unknown() {
        assert_eq!(signal_name(999), "UNKNOWN");
    }

    #[test]
    fn test_panic_message_str_type() {
        let result = std::panic::catch_unwind(|| panic!("hello"));
        let Err(e) = result else { unreachable!() };
        let msg = panic_message(&e);
        assert_eq!(msg, "hello");
    }

    #[test]
    fn test_panic_message_string_type() {
        let result = std::panic::catch_unwind(|| panic!("{}", "world"));
        let Err(e) = result else { unreachable!() };
        let msg = panic_message(&e);
        assert_eq!(msg, "world");
    }

    #[test]
    fn test_panic_message_unknown_type() {
        let result = std::panic::catch_unwind(|| panic!("{}", 42));
        let Err(e) = result else { unreachable!() };
        let msg = panic_message(&e);
        assert_eq!(msg, "42");
    }

    // ── Signal-based crash recovery tests ───────────────────────────────

    #[test]
    fn test_sandbox_catches_raised_sigbus() {
        let result: SandboxResult<()> = sandbox_call("raise_sigbus", || unsafe {
            libc::raise(libc::SIGBUS);
        });
        assert!(
            result.is_crashed(),
            "Expected Crashed from raised SIGBUS, got {:?}",
            result
        );
        if let SandboxResult::Crashed(crash) = result {
            assert_eq!(crash.signal, libc::SIGBUS);
            assert!(crash.context.contains("raise_sigbus"));
        }
    }

    #[test]
    fn test_sandbox_catches_sigsegv() {
        // Use raise() to deliver SIGSEGV synchronously (avoids Rust 2024
        // UB precondition checks that trigger SIGABRT rather than the
        // expected hardware signal when accessing invalid memory).
        let result: SandboxResult<i32> = sandbox_call("segv_test", || {
            unsafe {
                libc::raise(libc::SIGSEGV);
            }
            0 // Never reached
        });
        assert!(
            result.is_crashed(),
            "Expected Crashed from SIGSEGV, got {:?}",
            result
        );
        if let SandboxResult::Crashed(crash) = result {
            assert_eq!(
                crash.signal,
                libc::SIGSEGV,
                "Expected SIGSEGV, got {} ({})",
                crash.signal,
                crash.signal_name
            );
        }
    }

    #[test]
    fn test_sandbox_recovery_allows_subsequent_calls() {
        // First call crashes
        let r1: SandboxResult<()> = sandbox_call("crash1", || unsafe {
            libc::raise(libc::SIGBUS);
        });
        assert!(r1.is_crashed());

        // Second call should work fine
        let r2 = sandbox_call("normal", || 42);
        assert!(r2.is_ok());
        assert_eq!(r2.unwrap(), 42);

        // Third call crashes again
        let r3: SandboxResult<()> = sandbox_call("crash2", || unsafe {
            libc::raise(libc::SIGBUS);
        });
        assert!(r3.is_crashed());
    }

    #[test]
    fn test_sandbox_catches_sigabrt() {
        let result: SandboxResult<()> = sandbox_call("abort_test", || unsafe {
            libc::raise(libc::SIGABRT);
        });
        assert!(result.is_crashed());
        if let SandboxResult::Crashed(crash) = result {
            assert_eq!(crash.signal, libc::SIGABRT);
        }
    }

    #[test]
    fn test_handler_refcount_returns_to_zero() {
        // After sandbox_call completes, the refcount should be 0
        let _ = sandbox_call("refcount_test", || 42);
        assert_eq!(HANDLER_REFCOUNT.load(Ordering::SeqCst), 0);
    }
}
