# Debug & Profiling Infrastructure Plan

## Problem Statement

The app suffers from heap corruption caused by `siglongjmp`-based crash recovery from C++ plugin code. The sandbox successfully catches the initial signal (SIGBUS/SIGABRT from plugin COM cleanup), but `siglongjmp` skips C++ destructors and in-progress `free()` calls, leaving the malloc freelist inconsistent. Subsequent allocations — even from pure Rust/egui code — trigger `malloc_zone_error` → `abort()`. This second abort happens **outside** any sandbox scope, crashing the host.

### Crash Evidence

1. **CRASH_REPORT.md** — `EXC_BAD_ACCESS (SIGBUS)` during plugin deactivation. `Vst3Instance::Drop` → FabFilter Pro-Q 4 plugin code → trashed C++ vtable in `libc++abi.dylib __AUTH_CONST`.
2. **report.txt** — `EXC_BAD_ACCESS (SIGABRT)` from heap corruption. `egui` rendering `ui.label()` → `accesskit::Node::set_bounds` → `Vec::push` → `alloc::alloc` → `tiny_malloc_from_free_list` → `malloc_zone_error` → `abort()`.
3. **log.ansi** — Runtime sequence: `SIGABRT` during sandbox-recovered `release_controller` COM call → module skips library unload → `malloc: Double free of object 0x1451d4c00` → `malloc: Corruption of free object 0x1451b8470: msizes 19456/0 disagree`.

## Goal

Build a multi-layered diagnostic system to characterize and catch the heap corruption. All changes are additive — the existing sandbox architecture is unchanged. The root cause fix (out-of-process plugin sandboxing) is deferred.

---

## Workstreams

### 1. Cargo Feature Flags (`Cargo.toml`)

Define three optional features:

| Feature | Enables | Crate |
|---------|---------|-------|
| `debug-alloc` | Heap allocation profiling | `dhat = "0.3"` |
| `debug-trace` | Chrome trace timeline output | `tracing-chrome = "0.7"` |
| `debug-tools` | Both of the above | — |

Additional changes:
- Add `tracing-subscriber` feature `"registry"` alongside existing `"env-filter"`.
- Add `backtrace = "0.3"` as a direct dependency (currently only transitive via `anyhow`).

### 2. Backtrace Capture in Signal Handler (`src/vst3/sandbox.rs`)

**Problem**: `siglongjmp` destroys the crash stack before any backtrace can be taken in the recovery path.

**Solution**: Two-phase capture.

1. **In the signal handler** (before `siglongjmp`): Two new thread-locals:
   - `CRASH_BACKTRACE: UnsafeCell<[*mut c_void; 128]>`
   - `CRASH_BACKTRACE_LEN: Cell<usize>`
   
   Call raw `libc::backtrace()` via FFI (from `<execinfo.h>`) to capture up to 128 frame addresses. This is commonly used in signal handlers on macOS ARM64 (CrashPad/BreakPad pattern) — it does frame-pointer walking which is safe on ARM64.

2. **In the recovery path** (after `sigsetjmp` returns non-zero): Read the raw addresses from the thread-local, symbolicate using the `backtrace` crate's `resolve()` (safe here — back in normal context). Add symbolicated frames as `Vec<String>` on `PluginCrash`. Log with `error!`.

**Why raw `backtrace()` in handler, `backtrace` crate for symbolication**: The Rust `backtrace` crate allocates memory and is not signal-safe. Raw `backtrace()` from `<execinfo.h>` does frame-pointer walking without allocation.

### 3. Heap Integrity Checks After Sandbox Recovery (`src/vst3/sandbox.rs`)

**FFI declaration**:
```rust
extern "C" {
    fn malloc_zone_check(zone: *mut c_void) -> i32;
}
```

**Insertion point**: In the recovery path (after clearing `SANDBOX_ACTIVE`, before constructing `PluginCrash`). Call `malloc_zone_check(null)`:
- Returns `1` → heap OK
- Returns `0` → corruption detected

Add `heap_corrupted: bool` field to `PluginCrash`. Log `error!("HEAP CORRUPTION DETECTED after plugin crash recovery")` when detected. This gives the host actionable info for UI warnings.

### 4. Diagnostics Module (`src/diagnostics.rs`)

Central hub for debug infrastructure:

- **`check_malloc_env()`** — Detects and logs whether `MallocStackLogging`, `MallocGuardEdges`, `MallocScribble` etc. are active (via `std::env::var`). Called at startup.
- **`heap_check() -> bool`** — Wraps `malloc_zone_check(null)` for use anywhere. Returns `true` = OK, `false` = corrupt.
- **`init_profiler()` / `shutdown_profiler()`** — `dhat::Profiler` lifecycle (behind `#[cfg(feature = "debug-alloc")]`). On drop, writes `dhat-heap.json`.
- **`recommended_env_vars() -> Vec<(&str, &str)>`** — Returns recommended `MALLOC_*` vars for debugging, used by `--malloc-debug` CLI flag.

### 5. `dhat` Global Allocator (`src/main.rs`)

Behind `#[cfg(feature = "debug-alloc")]`:
```rust
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;
```

Create `dhat::Profiler::new_heap()` early in `main()`. On program exit, writes `dhat-heap.json` — shows exactly which allocation sites are hit after crash recovery, which trigger corruption, and total heap usage.

**Why `dhat` over `tikv-jemallocator`**: dhat is pure Rust, simpler to integrate, and has a web viewer. jemalloc replaces the system allocator entirely, which could mask the corruption we're trying to diagnose — we want to keep system `malloc` to reproduce the bug.

### 6. Structured Tracing Refactor (`src/main.rs`)

Switch from `tracing_subscriber::fmt().init()` to layered `Registry` pattern:
```rust
use tracing_subscriber::{Registry, layer::SubscriberExt, util::SubscriberInitExt};

Registry::default()
    .with(fmt::layer().with_env_filter(...))
    .with(optional_chrome_layer)  // Option<Layer> — None is zero-cost no-op
    .init();
```

Behind `#[cfg(feature = "debug-trace")]`: `tracing_chrome::ChromeLayerBuilder` produces `trace-{timestamp}.json`, viewable in `chrome://tracing` or Perfetto UI.

**Why `tracing-chrome` over `tracing-tracy`**: Chrome is zero-dependency, file-based, requires no external GUI tool. Tracy can be added later behind a separate feature flag.

### 7. Performance Spans on Hot Paths

Instrument key functions with `#[instrument]` or manual `tracing::span!`:

| Location | Span | Purpose |
|----------|------|---------|
| `sandbox_call()` in `sandbox.rs` | Per plugin COM call with context string | Correlate crashes with timing |
| `AudioEngine::process()` in `engine.rs` | Per audio callback | Block-level timing |
| `Vst3Instance::process()` in `instance.rs` | Per plugin process call | Plugin processing latency |
| `Vst3Instance::Drop` in `instance.rs` | 5-step COM cleanup | Cleanup phase timing |
| `HostBackend::activate/deactivate_plugin()` in `backend.rs` | Lifecycle operations | Activation/deactivation timing |
| `HostApp::update()` in `app.rs` | Per GUI frame | Frame timing |

### 8. `--malloc-debug` CLI Flag (`src/app/cli.rs`)

A `gui` subcommand flag that:
1. Calls `diagnostics::check_malloc_env()` to log current malloc debug state.
2. If malloc env vars aren't set, prints re-launch instructions: `MallocGuardEdges=1 MallocScribble=1 MallocErrorAbort=1 cargo run -- gui`.
3. Calls `diagnostics::heap_check()` periodically from the GUI update loop (every ~60 frames) and logs warnings.

### 9. Heap Corruption Warning in GUI (`src/gui/backend.rs`, `src/gui/app.rs`)

When `PluginCrash.heap_corrupted` is true:
- Set `HostBackend.heap_corruption_detected: bool` flag.
- GUI displays persistent red banner: **"Heap corruption detected — save your session and restart."**
- Prevents user from continuing unknowingly in a corrupted process.

### 10. Unit Tests

Maintain >80% test coverage. New tests:

| Test | What it verifies |
|------|-----------------|
| `diagnostics::heap_check()` | Returns `true` in clean process |
| `diagnostics::check_malloc_env()` | Works with/without env vars |
| Backtrace in sandbox | `sandbox_call` with `libc::raise(SIGBUS)` → `PluginCrash.backtrace` non-empty |
| `malloc_zone_check` integration | Clean recovery → `heap_corrupted: false` |
| CLI `--malloc-debug` | Flag parsed correctly |
| Feature-flag gating | `debug-alloc` and `debug-trace` compile in both enabled/disabled states |
| Chrome trace layer | `debug-trace` feature doesn't panic during subscriber init |

### 11. Documentation Updates

- **README.md** — New "Debugging" section covering feature flags and workflows.
- **USER_GUIDE.md** — `--malloc-debug` usage, malloc env var instructions.
- **CHANGELOG.md** — Version entry for diagnostic infrastructure.
- **STATUS.md** — Updated with current phase progress.
- **CODE_COVERAGE.md** — Updated with new test counts.

---

## macOS Malloc Debug Environment Variables Reference

| Variable | Purpose | Set at |
|----------|---------|--------|
| `MallocStackLogging=1` | Records allocation/free call stacks; enables `leaks`, `malloc_history` | Launch only |
| `MallocGuardEdges=1` | Guard pages around large allocations (detect overflow/underflow) | Launch only |
| `MallocScribble=1` | Fills freed memory with `0x55`, allocated with `0xAA` (detect use-after-free) | Launch only |
| `MallocCheckHeapStart=N` | First heap validation after N allocations | Launch only |
| `MallocCheckHeapEach=N` | Heap validation every N allocations | Launch only |
| `MallocErrorAbort=1` | `abort()` on any malloc error (trap corruption immediately) | Launch only |

**Full diagnostic launch command**:
```bash
MallocGuardEdges=1 MallocScribble=1 MallocErrorAbort=1 \
  RUST_LOG=rs_vst_host=debug \
  cargo run --features debug-tools -- gui --malloc-debug
```

---

## Verification Checklist

- [ ] `cargo test` — all existing 415 tests pass + new diagnostic tests
- [ ] `cargo test --features debug-tools` — all tests pass with debug features
- [ ] `cargo build` — release build compiles with zero debug overhead (no features)
- [ ] `cargo build --features debug-trace` — produces Chrome trace on run
- [ ] `cargo build --features debug-alloc` — produces `dhat-heap.json` on exit
- [ ] Manual: activate FabFilter plugin → deactivate → verify backtrace in logs, `malloc_zone_check` result logged, heap corruption banner shown if applicable
- [ ] Full diagnostic mode: `MallocGuardEdges=1 MallocScribble=1 cargo run --features debug-tools -- gui --malloc-debug`

---

## Key Design Decisions

| Decision | Rationale |
|----------|-----------|
| `tracing-chrome` over `tracing-tracy` | Zero external dependencies, file-based, no GUI tool needed |
| `dhat` over `tikv-jemallocator` | Pure Rust, web viewer, keeps system `malloc` to reproduce the bug |
| Raw `backtrace()` in handler + `backtrace` crate post-recovery | Rust `backtrace` crate is not signal-safe; raw FFI does frame-pointer walk |
| `malloc_zone_check(NULL)` on-demand | Env vars are launch-time only; programmatic check after every crash recovery |
| `heap_corrupted` flag propagated to GUI | Unmissable visual warning prevents data loss from operating in corrupted process |
| Feature flags for all debug tooling | Zero cost in release builds; opt-in for development |
