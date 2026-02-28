# FFI Boundaries in `rs-vst-host`

This document lists all places where Rust code interfaces with non-Rust code (C/C++/Objective-C/OS ABI), and why each boundary exists.

## Scope

- **Included:** runtime (non-test) boundaries in `src/` that cross Rust ↔ non-Rust ABI.
- **Included:** test-only boundaries in a separate section.
- **Not included:** pure-Rust crate APIs (even if those crates use FFI internally).

---

## 1) VST3 module loading + plugin entry points

### Where
- `src/vst3/module.rs`

### Boundary calls
- Dynamic library loading via `libloading::Library::new` (loads plugin binary).
- Symbol lookup + invocation for C exports:
  - `GetPluginFactory` (`unsafe extern "C" fn() -> *mut c_void`)
  - macOS: `bundleEntry`, `bundleExit`
  - Linux: `ModuleEntry`

### Why
- VST3 plugins are native shared libraries (typically C++). The host must load them and call their exported bootstrap functions to obtain the plugin factory and initialize/teardown module state.

---

## 2) CoreFoundation boundary for macOS VST3 bundles

### Where
- `src/vst3/cf_bundle.rs`
- used by `src/vst3/module.rs`

### Boundary calls
- `CFStringCreateWithCString`
- `CFURLCreateWithFileSystemPath`
- `CFBundleCreate`
- `CFRelease`

### Why
- On macOS, VST3 `bundleEntry` expects a valid `CFBundleRef` so plugins can locate resources inside the `.vst3` bundle.

---

## 3) VST3 COM ABI definitions (Rust <-> plugin C++ ABI contract)

### Where
- `src/vst3/com.rs`
- `src/vst3/module.rs` (factory vtables)

### Boundary declarations
- Manual `#[repr(C)]` COM vtable and struct layouts with `unsafe extern "system" fn` entries.

### Why
- VST3 interfaces are COM-style binary contracts. Rust must exactly match the C++ ABI layout and calling convention for correct cross-language calls.

---

## 4) Host -> plugin COM calls (calling plugin C++ code)

### Where
- `src/vst3/instance.rs`
- `src/vst3/module.rs`
- `src/gui/editor.rs`

### Boundary calls (examples)
- Factory/component/processor/controller methods:
  - `create_instance`, `initialize`, `terminate`, `query_interface`
  - `set_active`, `set_processing`, `process`, `get_bus_info`, `set_state`/`get_state`
  - controller methods like `create_view`, parameter APIs
- Plugin editor view (`IPlugView`) methods:
  - `attached`, `removed`, `set_frame`, `can_resize`, `check_size_constraint`, etc.

### Why
- This is the core host functionality: creating plugin instances, driving audio processing, loading/saving state, and embedding plugin UI.

---

## 5) Plugin -> host COM callbacks (plugin calls into Rust)

### Where
- `src/vst3/host_context.rs` (`IHostApplication`)
- `src/vst3/component_handler.rs` (`IComponentHandler`)
- `src/vst3/event_list.rs` (`IEventList`)
- `src/vst3/param_changes.rs` (`IParameterChanges`, `IParamValueQueue`)
- `src/vst3/plug_frame.rs` (`IPlugFrame`)
- `src/vst3/ibstream.rs` (`IBStream`)

### Boundary calls
- Rust exposes COM vtables with `unsafe extern "system" fn` function pointers that plugin C++ code invokes.

### Why
- Plugins need host services and host-owned data interfaces:
  - host identity/context
  - parameter edit notifications
  - MIDI/event transport
  - automation queues
  - editor resize negotiation
  - state serialization streams

---

## 6) System allocator boundary for plugin-facing objects

### Where
- `src/vst3/host_alloc.rs`
- used by: `host_context.rs`, `component_handler.rs`, `plug_frame.rs`, `ibstream.rs`

### Boundary calls
- `libc::malloc`
- `libc::free`
- macOS check: `malloc_zone_from_ptr` (extern declaration)

### Why
- Plugin-facing COM objects must reside on system malloc so misbehaving plugin C++ code that uses `free`/`delete` on host pointers does not immediately violate allocator ownership assumptions.

---

## 7) Crash sandbox: POSIX signals + setjmp/longjmp + raw backtrace

### Where
- `src/vst3/sandbox.rs`

### Boundary calls
- FFI declarations:
  - `backtrace` (from `execinfo.h`)
  - macOS: `malloc_zone_check`
  - macOS: `sigsetjmp`, `siglongjmp` (manual declarations)
- libc ABI calls:
  - `sigaction`, `signal`, `raise`

### Why
- Recover host control flow after plugin crashes (SIGBUS/SIGSEGV/SIGABRT/SIGFPE), capture crash context, and assess potential heap corruption instead of hard-terminating the host process.

---

## 8) Native macOS GUI boundary (Objective-C runtime/AppKit)

### Where
- `src/gui/editor.rs`

### Boundary calls
- Linked ObjC runtime symbols:
  - `objc_getClass`
  - `sel_registerName`
  - `objc_msgSend`
- Many typed `objc_msgSend` signatures to invoke NSApplication/NSWindow/NSString APIs.

### Why
- Plugin editors on macOS require native `NSView` parenting and AppKit event-loop integration; this cannot be done purely in safe Rust without crossing into ObjC runtime APIs.

---

## 9) Cross-process zero-copy audio transport (POSIX shared memory)

### Where
- `src/ipc/shm.rs`

### Boundary calls
- `libc::shm_open`
- `libc::ftruncate`
- `libc::mmap`
- `libc::munmap`
- `libc::shm_unlink`
- `libc::close`

### Why
- Move audio buffers between host and worker process without per-block serialization/copy overhead.

---

## 10) Heap diagnostics boundary

### Where
- `src/diagnostics.rs`

### Boundary calls
- macOS: `malloc_zone_check`

### Why
- Runtime heap-integrity checks after risky plugin interactions/crash recovery paths.

---

## Test-only FFI boundaries

### Where
- `src/asan_tests.rs`
- `src/vst3/sandbox.rs` (test module)
- `src/vst3/module.rs` (test module)

### Boundary calls
- Signal-triggering and libc-level calls used in tests (for crash/recovery and allocator validation), e.g. `libc::raise` and other low-level paths exercised under ASan/sandbox tests.

### Why
- Validate crash containment, allocator interoperability, and cleanup behavior under failure scenarios that require real OS-level signal/ABI behavior.

---

## Summary

The non-Rust interfaces in this project are concentrated in four areas:
1. **VST3 COM ABI** (plugin hosting itself),
2. **Platform APIs** (CoreFoundation, ObjC/AppKit),
3. **OS primitives** (signals/setjmp, shared memory, system allocator),
4. **Diagnostics/safety** (heap validation, crash recovery).

These boundaries are necessary to host native C++ VST3 plugins safely from Rust while preserving performance and crash resilience.
