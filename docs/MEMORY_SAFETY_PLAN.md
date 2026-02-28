# Memory Safety Plan: Fix "pointer being freed was not allocated" During Plugin Teardown

## Issue Summary

When switching from one VST3 plugin (FabFilter Pro-Q 4) to another (Pro-MB), the host
crashes during plugin deactivation with:

```
malloc: *** error for object 0x12219c240: pointer being freed was not allocated
```

The crash occurs **after** the custom `ActiveState::drop` completes (logged as
"ActiveState dropped with controlled teardown order") but **during** the implicit
field drops — specifically when the `engine` Arc reaches refcount 0, destroying the
`AudioEngine` → `Vst3Instance`, or when the `Vst3Module` is dropped afterward.

---

## Root Cause Analysis

### Evidence from the trace and log

| Source | Observation |
|--------|-------------|
| **log.txt** | Shutdown sequence completes normally: "Processing stopped" → "Component deactivated" → "Audio engine shut down" → "ActiveState dropped with controlled teardown order" → **malloc error** |
| **log.txt** | No sandbox crash recovery messages appear — the crash occurs outside or between sandbox calls |
| **trace** | Audio thread (tid 1) has an **unclosed** `audio_engine_process` span at the very end — the last BEGIN has no matching END before the trace cuts off |
| **trace** | The trace file ends abruptly at ~44.9s; the crash occurs at ~45s (matching the log timestamps). Deactivation events were never flushed to the trace |
| **CRASH_REPORT.md** | A previous crash at the same code path (Vst3Instance::Drop) manifested as SIGBUS in `__cxxabiv1::__class_type_info` (C++ vtable access violation) |
| **log.txt** | `MallocErrorAbort=1` is active → the "pointer being freed was not allocated" error triggers `abort()` → SIGABRT |

### Identified Issues

#### Issue 1: Heap Domain Mismatch (mimalloc vs system malloc) — PRIMARY CAUSE

Since v0.15.0, the host uses **mimalloc** as the global Rust allocator to isolate Rust
heap allocations from plugin-induced system malloc corruption. However, this creates a
**reverse problem**: host-side COM objects passed to plugins are now allocated on the
mimalloc heap, but the plugin's C++ code (which uses system malloc) may try to `free()`
or `delete` these objects during teardown instead of properly calling `Release()` through
the COM vtable.

Affected objects allocated via `Box::new()` → mimalloc:
- `HostApplication` — passed to plugins via `IComponent::initialize()`
- `HostComponentHandler` — passed to plugins via `IEditController::setComponentHandler()`
- `HostPlugFrame` — passed to plugins via `IPlugView::setFrame()`

When a plugin's C++ destructor runs during `terminate()` or `release()` and calls
`operator delete` (→ system `free()`) on one of these pointers, macOS malloc zone
validation detects that the pointer does not belong to any system malloc zone →
"pointer being freed was not allocated" → `abort()`.

**Why this is the primary cause**: The error message is specifically about system
malloc not recognizing a pointer. This is the exact symptom of freeing a mimalloc
allocation through system malloc. The error address `0x12219c240` is in a heap region
consistent with mimalloc's allocation space.

#### Issue 2: Audio Thread Race During Teardown

The trace shows the audio thread (tid 1) has an **unclosed** `audio_engine_process`
span when the trace ends. While the Mutex provides mutual exclusion for the
`AudioEngine`, there is a timing gap:

1. GUI thread locks engine → `shutdown()` → sets `is_shutdown = true` + plugin deactivation
2. GUI thread unlocks engine
3. GUI thread calls `active._stream.take()` → drops cpal stream → `AudioOutputUnitStop`
4. **Timing gap**: between steps 2 and 3, the audio callback can fire and acquire the
   lock. It will see `is_shutdown = true` and fill silence, but the `try_lock()` call
   and lock acquisition itself race with the stream drop.

Additionally, on macOS, `AudioOutputUnitStop` may not guarantee that an in-flight
CoreAudio render callback has fully completed before returning. If the callback's
closure (holding an `Arc<Mutex<AudioEngine>>` clone) is dropped while the callback
is still on the audio thread's stack, this creates undefined behavior.

#### Issue 3: Unsandboxed Host Object Destruction

In `Vst3Instance::drop`, after all COM cleanup steps (which ARE sandboxed), the host
objects are destroyed **without** sandbox protection:

```rust
// NOT sandboxed:
HostApplication::destroy(self.host_context);
HostComponentHandler::destroy(self.component_handler);
```

If the plugin has deferred callbacks or background threads that reference these objects,
destroying them could trigger a use-after-free. With mimalloc, the free goes to mimalloc
(not system malloc), so this specific path wouldn't cause the observed error. However,
if a plugin's deferred work fires between the sandboxed COM cleanup and these destroy
calls, it could access freed plugin objects and trigger a system malloc error.

#### Issue 4: Double Factory Release Risk

Both `Vst3Instance::drop` (Step 5) and `Vst3Module::drop` call `factory.release()`.
This is **correctly balanced** (the instance AddRef's the factory during creation), but
if the instance's factory release destroys the factory object (refcount hits 0), the
module's subsequent factory release accesses a freed COM object. This depends on whether
the module's factory is the same pointer — it is, and the module holds its own AddRef'd
reference, so refcounts should be: module(1) + instance(1) = 2. After instance release:
1, after module release: 0 → destroyed. This is correct **unless** the plugin's factory
implementation has bugs in its reference counting.

#### Issue 5: C++ Static Destructors During Library Unload

When `Vst3Module::drop` calls `CFRelease(bundle_ref)`, the plugin's dynamic library is
unloaded. This triggers C++ static destructors (global/thread-local object destructors)
in the plugin code. These destructors may reference memory that was already freed during
the earlier COM cleanup, or they may try to interact with host objects that have been
destroyed.

---

## Proposed Fixes

### Fix 1: System Malloc Allocator for Plugin-Facing COM Objects

**Priority: HIGH — addresses the primary cause**

Create a `SystemAllocBox<T>` wrapper (or use `libc::malloc`/`libc::free` directly) to
allocate COM objects that are passed to plugins. This ensures they live on the system
malloc heap, so even if a plugin incorrectly calls `free()` on them, the pointer is
recognized by system malloc.

```rust
// New file: src/vst3/host_alloc.rs

use std::alloc::{Layout, handle_alloc_error};
use std::ffi::c_void;

/// Allocate a T on the system malloc heap (bypassing mimalloc).
/// Returns a raw pointer suitable for COM objects shared with plugins.
pub unsafe fn system_alloc<T>(value: T) -> *mut T {
    let layout = Layout::new::<T>();
    let ptr = unsafe { libc::malloc(layout.size()) } as *mut T;
    if ptr.is_null() {
        handle_alloc_error(layout);
    }
    unsafe { std::ptr::write(ptr, value) };
    ptr
}

/// Free a T previously allocated with `system_alloc`.
pub unsafe fn system_free<T>(ptr: *mut T) {
    if !ptr.is_null() {
        unsafe {
            std::ptr::drop_in_place(ptr);
            libc::free(ptr as *mut c_void);
        }
    }
}
```

**Apply to**: `HostApplication::new()`/`destroy()`, `HostComponentHandler::new()`/`destroy()`,
`HostPlugFrame::new()`/`destroy()`.

**Rationale**: These are the objects where the host's allocation domain crosses into
the plugin's domain. All other Rust allocations (buffers, Vecs, Arcs) remain on mimalloc
for isolation. Only the COM objects visible to plugins need to be on the system heap.

### Fix 2: Synchronous Audio Stream Shutdown

**Priority: HIGH — addresses the race condition**

Replace the current two-step shutdown (lock engine → shutdown; then take stream) with
an atomic stream-first approach that ensures no in-flight callbacks:

```rust
pub fn deactivate_plugin(&mut self) {
    // ...
    if let Some(mut active) = self.active.take() {
        // 1. Stop the audio stream FIRST — no more callbacks after this.
        //    This must happen before shutdown() because AudioOutputUnitStop
        //    needs to drain any in-flight callback.
        active._stream.take();

        // 2. Small sleep to allow any in-flight CoreAudio callback to complete.
        //    AudioOutputUnitStop should handle this, but defense-in-depth.
        std::thread::sleep(std::time::Duration::from_millis(10));

        // 3. Now safe to shut down — no audio thread can access the engine.
        if let Ok(mut eng) = active.engine.lock() {
            eng.shutdown();
        }

        // 4. Drop everything.
        drop(active);
    }
}
```

**Alternative (better)**: Use an `AtomicBool` shutdown flag that the audio callback
checks **before** acquiring the lock, eliminating the need for a sleep:

```rust
// In AudioEngine:
pub fn request_shutdown(&self) {
    self.shutdown_requested.store(true, Ordering::Release);
}

// In audio callback:
move |data: &mut [f32], _info| {
    if shutdown_requested.load(Ordering::Acquire) {
        data.fill(0.0);
        return;
    }
    if let Ok(mut eng) = engine_cb.try_lock() {
        eng.process(data);
    } else {
        data.fill(0.0);
    }
}
```

This way, the audio callback stops calling process() immediately when shutdown is
requested, without needing the Mutex. Then the stream can be safely dropped, and
the engine can be destroyed knowing no thread is inside process().

### Fix 3: Sandbox Host Object Destruction

**Priority: MEDIUM — defense-in-depth**

Wrap the host object destroy calls in `Vst3Instance::drop` inside sandbox calls:

```rust
// Instead of raw destroy calls:
let host_ctx = self.host_context;
let _ = sandbox_call("destroy_host_context", move || unsafe {
    HostApplication::destroy(host_ctx);
});

let handler = self.component_handler;
if !handler.is_null() {
    let _ = sandbox_call("destroy_component_handler", move || unsafe {
        HostComponentHandler::destroy(handler);
    });
}
```

If a deferred plugin callback fires during destruction (accessing freed plugin objects),
the sandbox catches the resulting crash.

### Fix 4: Per-Plugin Allocation Zone (Long-term)

**Priority: LOW — long-term architecture improvement**

As suggested in `MEMORY_ALLOCATION.idea.md`, create a per-plugin malloc zone on macOS
using `malloc_create_zone()`:

```rust
pub struct PluginAllocZone {
    zone: *mut libc::malloc_zone_t,
}

impl PluginAllocZone {
    pub fn new() -> Self {
        unsafe {
            let zone = libc::malloc_create_zone(0, 0);
            Self { zone }
        }
    }

    pub fn alloc(&self, size: usize) -> *mut c_void {
        unsafe { libc::malloc_zone_malloc(self.zone, size) }
    }

    pub fn free(&self, ptr: *mut c_void) {
        unsafe { libc::malloc_zone_free(self.zone, ptr) }
    }

    pub fn destroy(self) {
        unsafe { libc::malloc_destroy_zone(self.zone) }
    }
}
```

This provides:
- **Isolation**: Plugin-related allocations in their own zone
- **Bulk cleanup**: When a plugin is unloaded, its entire zone can be destroyed
- **Corruption detection**: `malloc_zone_check(zone)` can validate just the plugin's zone
- **No cross-heap issues**: Host COM objects allocated in the plugin's zone are
  recognized by system malloc

However, this requires intercepting the plugin's malloc calls (e.g., via
`malloc_zone_register` + `malloc_set_zone_name`), which is complex. The system
allocator shim approach is simpler for the immediate fix.

### Fix 5: Defensive Delay Before Module Unload

**Priority: MEDIUM — protects against C++ static destructor issues**

Add a brief delay/fence between COM cleanup and library unload to allow deferred
plugin work (background threads, dispatch queues) to complete:

```rust
// In Vst3Module::drop, before bundleExit:
std::thread::sleep(std::time::Duration::from_millis(50));
```

This is a pragmatic workaround for plugins that dispatch cleanup work asynchronously.
The proper fix is process isolation (v0.16.0), which eliminates the problem entirely
by running the plugin in a separate address space.

---

## Implementation Order

| Phase | Fix | Effort | Impact |
|-------|-----|--------|--------|
| **Phase A** | Fix 1: System malloc for COM objects | 2-3 hours | **Eliminates primary crash cause** |
| **Phase A** | Fix 2: AtomicBool shutdown flag | 1-2 hours | **Eliminates audio thread race** |
| **Phase B** | Fix 3: Sandbox host object destruction | 30 min | Defense-in-depth |
| **Phase B** | Fix 5: Delay before module unload | 15 min | Pragmatic workaround |
| **Phase C** | Fix 4: Per-plugin allocation zone | 4-6 hours | Long-term architecture |

Phase A addresses the two high-priority issues and should resolve the crash.
Phase B adds safety margins.
Phase C is a longer-term architectural improvement.

---

## Validation Plan

1. **Reproduce**: Run `debug.bash` with `MallocGuardEdges=1 MallocScribble=1 MallocErrorAbort=1`
   and switch between FabFilter plugins repeatedly
2. **Verify Fix 1**: Confirm host COM objects show allocations in system malloc zones
   (via `malloc_zone_from_ptr()` check in tests)
3. **Verify Fix 2**: Confirm no unclosed audio_engine_process spans in the Chrome trace
   after deactivation
4. **Stress test**: Rapid plugin switching (activate → deactivate → activate, 50 cycles)
   under malloc debug environment
5. **Regression**: Full `cargo test` suite must pass (currently 498 tests)
6. **Process isolation**: Verify that sandboxed mode (v0.16.0) is unaffected by these
   changes (it doesn't share the address space, so heap domain mismatch is irrelevant)

---

## Background: Why mimalloc Made This Worse

Before v0.15.0, both Rust and plugin allocations used system malloc. A plugin calling
`free()` on a host-allocated COM object would succeed (wrong, but not caught). After
v0.15.0, Rust allocations moved to mimalloc, making host COM objects invisible to system
malloc. The "pointer being freed was not allocated" error is actually `MallocGuardEdges`
detecting what was previously a silent use-after-free or double-free.

In other words: **mimalloc didn't cause the bug — it exposed a pre-existing bug** where
a plugin was incorrectly freeing a host-owned COM object instead of calling Release().
The fix (using system malloc for plugin-visible objects) makes both allocator domains
consistent while keeping Rust's heap isolated for everything else.

---

## Related Files

| File | Relevance |
|------|-----------|
| `src/vst3/instance.rs` | Vst3Instance Drop — COM cleanup sequence |
| `src/vst3/module.rs` | Vst3Module Drop — factory release, bundleExit, library unload |
| `src/vst3/host_context.rs` | HostApplication — Box::new/from_raw → needs system malloc |
| `src/vst3/component_handler.rs` | HostComponentHandler — Box::new/from_raw → needs system malloc |
| `src/vst3/plug_frame.rs` | HostPlugFrame — Box::new/from_raw → needs system malloc |
| `src/gui/backend.rs` | ActiveState Drop order, deactivate_plugin sequence |
| `src/audio/engine.rs` | AudioEngine, audio callback, shutdown logic |
| `src/vst3/sandbox.rs` | Signal-handler-based crash recovery |
