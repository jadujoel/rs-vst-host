//! Diagnostic infrastructure for heap corruption detection and debug profiling.
//!
//! Provides:
//! - `check_malloc_env()` — detects and logs active macOS malloc debug environment variables
//! - `heap_check()` — wraps `malloc_zone_check(NULL)` for on-demand heap integrity testing
//! - `init_profiler()` / `shutdown_profiler()` — `dhat::Profiler` lifecycle (behind `debug-alloc` feature)
//! - `recommended_env_vars()` — returns recommended `MALLOC_*` vars for debugging

use std::ffi::c_void;
use tracing::{info, warn};

// ── macOS malloc zone FFI ───────────────────────────────────────────────────

#[cfg(target_os = "macos")]
unsafe extern "C" {
    /// Validates the heap integrity of all malloc zones (when `zone` is NULL).
    /// Returns 1 if heap is OK, 0 if corruption is detected.
    fn malloc_zone_check(zone: *mut c_void) -> i32;
}

// ── Heap integrity check ────────────────────────────────────────────────────

/// Check the process heap for corruption using `malloc_zone_check(NULL)`.
///
/// Returns `true` if the heap is OK, `false` if corruption is detected.
/// On non-macOS platforms, always returns `true` (no-op).
pub fn heap_check() -> bool {
    #[cfg(target_os = "macos")]
    {
        // Safety: malloc_zone_check(NULL) checks all zones; safe to call at any time.
        let result = unsafe { malloc_zone_check(std::ptr::null_mut()) };
        result == 1
    }

    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

// ── malloc environment variable detection ───────────────────────────────────

/// Names of macOS malloc debug environment variables to check.
const MALLOC_ENV_VARS: &[&str] = &[
    "MallocStackLogging",
    "MallocGuardEdges",
    "MallocScribble",
    "MallocCheckHeapStart",
    "MallocCheckHeapEach",
    "MallocErrorAbort",
];

/// Detect and log which malloc debug environment variables are active.
///
/// Called at startup to inform the user about the malloc debugging state.
/// Returns a list of (name, value) pairs for active variables.
pub fn check_malloc_env() -> Vec<(String, String)> {
    let mut active = Vec::new();

    for &var in MALLOC_ENV_VARS {
        if let Ok(val) = std::env::var(var) {
            info!(var = var, value = %val, "Malloc debug env var active");
            active.push((var.to_string(), val));
        }
    }

    if active.is_empty() {
        info!("No malloc debug environment variables detected");
    } else {
        info!(
            count = active.len(),
            "Malloc debug environment variables active"
        );
    }

    active
}

/// Returns recommended environment variables for malloc debugging on macOS.
///
/// Each entry is `(name, value)`. Used by the `--malloc-debug` CLI flag
/// to print re-launch instructions if these vars aren't set.
pub fn recommended_env_vars() -> Vec<(&'static str, &'static str)> {
    vec![
        ("MallocGuardEdges", "1"),
        ("MallocScribble", "1"),
        ("MallocErrorAbort", "1"),
    ]
}

// ── dhat profiler lifecycle ─────────────────────────────────────────────────

/// Holds the `dhat::Profiler` so it can be dropped on shutdown.
/// When dropped, writes `dhat-heap.json`.
#[cfg(feature = "debug-alloc")]
pub struct DhatProfiler {
    _profiler: dhat::Profiler,
}

/// Initialize the dhat heap profiler.
///
/// Returns a guard that, when dropped, writes `dhat-heap.json` to the
/// current directory. Call early in `main()`.
#[cfg(feature = "debug-alloc")]
pub fn init_profiler() -> DhatProfiler {
    info!("Initializing dhat heap profiler");
    DhatProfiler {
        _profiler: dhat::Profiler::new_heap(),
    }
}

/// Shutdown the profiler (writes `dhat-heap.json`).
///
/// This is called implicitly when the `DhatProfiler` guard is dropped.
/// Calling explicitly just drops the guard.
#[cfg(feature = "debug-alloc")]
pub fn shutdown_profiler(profiler: DhatProfiler) {
    info!("Shutting down dhat heap profiler — writing dhat-heap.json");
    drop(profiler);
}

/// Print re-launch instructions with recommended malloc debug env vars.
pub fn print_malloc_debug_instructions() {
    let active = check_malloc_env();
    let recommended = recommended_env_vars();

    let missing: Vec<_> = recommended
        .iter()
        .filter(|(name, _)| !active.iter().any(|(a, _)| a == name))
        .collect();

    if missing.is_empty() {
        info!("All recommended malloc debug environment variables are set");
        return;
    }

    warn!("Some recommended malloc debug environment variables are not set.");
    warn!("For full diagnostic mode, re-launch with:");
    let env_str: Vec<String> = recommended
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect();
    warn!(
        "  {} RUST_LOG=rs_vst_host=debug cargo run --features debug-tools -- gui --malloc-debug",
        env_str.join(" ")
    );
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heap_check_returns_true_in_clean_process() {
        // In a newly started test process, the heap should be clean.
        assert!(
            heap_check(),
            "heap_check() should return true in a clean process"
        );
    }

    #[test]
    fn test_check_malloc_env_works_without_env_vars() {
        // In a normal test environment, no malloc debug vars should be set.
        // (We don't assert the result because CI might set them, but it must not panic.)
        let result = check_malloc_env();
        // Just verify it returns a Vec without panicking.
        let _ = result;
    }

    #[test]
    fn test_check_malloc_env_detects_set_var() {
        // Temporarily set a malloc env var and verify detection.
        // Safety: no other threads are reading env vars concurrently in this test.
        unsafe {
            std::env::set_var("MallocScribble", "1");
        }
        let result = check_malloc_env();
        assert!(
            result.iter().any(|(k, _)| k == "MallocScribble"),
            "Should detect MallocScribble=1"
        );
        // Clean up
        unsafe {
            std::env::remove_var("MallocScribble");
        }
    }

    #[test]
    fn test_recommended_env_vars_non_empty() {
        let vars = recommended_env_vars();
        assert!(!vars.is_empty(), "Should have recommended vars");
        // Should include the three key vars
        assert!(vars.iter().any(|(k, _)| *k == "MallocGuardEdges"));
        assert!(vars.iter().any(|(k, _)| *k == "MallocScribble"));
        assert!(vars.iter().any(|(k, _)| *k == "MallocErrorAbort"));
    }

    #[test]
    fn test_recommended_env_vars_all_have_value_1() {
        let vars = recommended_env_vars();
        for (name, value) in &vars {
            assert_eq!(
                *value, "1",
                "{} should have value '1', got '{}'",
                name, value
            );
        }
    }

    #[test]
    fn test_print_malloc_debug_instructions_does_not_panic() {
        // Just verify it doesn't panic in any environment.
        print_malloc_debug_instructions();
    }

    #[cfg(feature = "debug-alloc")]
    #[test]
    fn test_init_profiler_does_not_panic() {
        let profiler = init_profiler();
        shutdown_profiler(profiler);
    }
}
