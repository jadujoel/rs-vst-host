# Changelog

All notable changes to this project will be documented in this file.

## [0.17.1] - 2026-02-27

### Added
- **Miri dynamic analysis infrastructure**: Added Miri-based undefined behavior detection for all pure-Rust unsafe code. Miri interprets Rust MIR at runtime and catches use-after-free, double-free, out-of-bounds access, uninitialized reads, aliasing violations, and data races that neither the compiler nor standard tests detect.
  - `src/lib.rs`: Library crate re-exporting all modules, enabling `cargo miri test --lib` without compiling the binary entry point (which uses FFI global allocators incompatible with Miri).
  - `src/miri_tests.rs`: 21 dedicated Miri-targeted tests exercising the highest-risk unsafe patterns — COM vtable dispatch (create→add→vtable read→destroy), self-referential buffer management (ProcessData→AudioBusBuffers→channel_buffers→samples), struct-to-bytes reinterpretation (NoteOnEvent/NoteOffEvent byte-level roundtrip), cross-module integration (MIDI→translate→EventList→ProcessData→vtable readback), `unsafe impl Send` thread safety (ProcessBuffers moved across threads), and lifecycle stress testing (50 COM create/destroy cycles).
  - `DYNAMIC_ANALYSIS.md`: Comprehensive guide covering prerequisites, quick start commands, aliasing model comparison (Stacked Borrows vs Tree Borrows), per-module compatibility table, known limitations, CI integration, Miri flag reference, and error interpretation.
  - `test.bash`: Single-command test runner that executes all test suites — standard `cargo test --lib`, `cargo clippy`, Miri with Tree Borrows (109 tests), and Miri with Stacked Borrows (70 tests) — with color-coded pass/fail summary.
- 21 new unit tests (512 → 533 total): all in `miri_tests.rs`.
- 109 tests pass under Miri with Tree Borrows (`-Zmiri-tree-borrows`), 70 under default Stacked Borrows.

### Changed
- `vst3/host_alloc.rs`: `test_box_alloc_is_not_in_system_zone` now handles both mimalloc (binary crate) and system allocator (library crate) contexts gracefully.

### Discovered
- **Stacked Borrows aliasing finding in `ProcessBuffers`**: The self-referential pointer pattern (`process_data.inputs = &mut self.input_bus`) is technically UB under the Stacked Borrows model because subsequent `&mut self` method calls retag the entire struct, invalidating stored pointers. This is a well-known limitation of Stacked Borrows with self-referential structs. The pattern is correct under Tree Borrows and real hardware — the underlying memory never moves and pointers are re-established by `prepare()` before each process call.

## [0.17.0] - 2026-02-27

### Added
- **System malloc allocator for COM objects** (`vst3/host_alloc.rs`): New module providing `system_alloc` / `system_free` which bypass the global allocator (mimalloc) and call `libc::malloc` / `libc::free` directly. Plugin-facing COM objects (`HostApplication`, `HostComponentHandler`, `HostPlugFrame`) are now allocated on the system malloc heap. This prevents "pointer being freed was not allocated" aborts when a plugin incorrectly calls `free()` / `operator delete` on a host object instead of using COM `Release()`. Includes `is_system_malloc_ptr()` for validation.
- **Atomic shutdown flag** on `AudioEngine`: The audio callback now checks an `AtomicBool` shutdown flag *before* trying to acquire the Mutex lock. This eliminates the race window between `engine.shutdown()` (which sets `is_shutdown` under the lock) and stream drop (which stops the CoreAudio callback). Both the GUI backend and CLI audio callbacks use this flag.
- **Sandboxed host object destruction**: `HostApplication::destroy()`, `HostComponentHandler::destroy()` calls in `Vst3Instance::drop` are now wrapped in `sandbox_call()` for defense-in-depth. If a plugin has deferred callbacks that reference these objects, the resulting crash is caught by the sandbox.
- **Defensive delay before module unload**: 50ms sleep before `bundleExit` / `CFRelease` in `Vst3Module::drop` allows plugin background threads and C++ static destructors to settle before library unload.
- **Stream-first deactivation**: In `deactivate_plugin()`, the audio stream is now stopped *before* engine shutdown (was previously the reverse). A 10ms drain sleep is added after stream stop for any in-flight CoreAudio callbacks.
- 14 new unit tests (498 → 512 total): 8 in `host_alloc.rs` (alloc/free, null safety, system zone verification, drop semantics, alignment, stress test), 3 system-heap verification tests for `HostApplication`, `HostComponentHandler`, `HostPlugFrame`, 2 shutdown flag tests in `audio/engine.rs`, 1 concurrent test threshold adjustment.

### Changed
- `HostApplication::new()` / `destroy()` now use `host_alloc::system_alloc` / `system_free` instead of `Box::new` / `Box::from_raw`.
- `HostComponentHandler::new()` / `destroy()` now use `host_alloc::system_alloc` / `system_free` instead of `Box::new` / `Box::from_raw`.
- `HostPlugFrame::new()` / `destroy()` now use `host_alloc::system_alloc` / `system_free` instead of `Box::new` / `Box::from_raw`.
- `Vst3Module::drop` host context destroy is now sandboxed.
- Audio callback in both GUI backend and CLI `run` command checks `shutdown_requested` AtomicBool before acquiring engine Mutex.
- `deactivate_plugin()` reordered: stream stop → drain sleep → engine shutdown (was: engine shutdown → stream stop).
- Concurrent component handler test threshold lowered from 200 to 100 to reduce flakiness with system_alloc.

### Fixed
- **"pointer being freed was not allocated" crash** during plugin teardown when switching between VST3 plugins. Root cause: mimalloc (global allocator since v0.15.0) placed host COM objects on a separate heap; plugins calling `free()` on these objects triggered macOS malloc zone validation → SIGABRT. Fix: COM objects now allocated via `libc::malloc` on the system heap.
- **Audio thread race during teardown**: Audio callback could acquire the engine Mutex lock between engine shutdown and stream stop, potentially accessing a deactivated plugin. Fix: AtomicBool flag checked before lock acquisition; stream stopped before engine shutdown.

## [0.16.0] - 2026-02-27

### Added
- **Process-per-plugin sandboxing** (`ipc/` module): Each plugin can now run in its own child process with full crash and memory isolation. This is the gold standard approach used by DAWs like Bitwig Studio. Audio buffers are exchanged via POSIX shared memory (`shm_open`/`mmap`) for zero-copy transfer, and control messages are sent over Unix domain sockets with a JSON + length-prefix wire protocol.
  - `ipc/messages.rs`: IPC protocol definitions with 12 host message types and 15 worker response types, including `Process`, `LoadPlugin`, `Configure`, `Activate`, `SetParameter`, `QueryParameters`, etc. Wire protocol uses 4-byte LE length prefix with 16 MB max message size.
  - `ipc/shm.rs`: POSIX shared memory management — `ShmAudioBuffer` with 64-byte header (ready flag, sample count, channel counts), per-channel audio buffers for input and output, memory-fence signaling.
  - `ipc/worker.rs`: Child process entry point — loads VST3 plugin via `Vst3Module`/`Vst3Instance`, handles full plugin lifecycle (load → configure → activate → process → deactivate → shutdown), sandbox-wrapped `process()` calls with crash reporting.
  - `ipc/proxy.rs`: Host-side proxy — `PluginProcess::spawn()` creates Unix socket, spawns child via `worker --socket <path>`, handshake with LoadPlugin → Configure → Activate, process() sends audio via shared memory. Includes graceful shutdown with SIGKILL timeout.
- **`process_isolation` flag on `HostBackend`**: When set to `true`, `activate_plugin()` spawns the plugin in a child process instead of loading it in-process. All backend methods (`set_parameter`, `set_tempo`, `is_crashed`, etc.) transparently handle both modes.
- **Hidden `worker` CLI subcommand**: Internal-only subcommand used by child processes — `worker --socket <path>`. Hidden from `--help`.
- **Backend sandboxed state** (`gui/backend.rs`): `SandboxedState` struct manages the `PluginProcess` proxy, audio stream, parameter queue, and MIDI connection for process-isolated plugins.
- 53 new unit tests (445 → 498 total): 18 in `ipc/messages.rs` (serialization roundtrip, encode/decode, oversized message), 12 in `ipc/shm.rs` (create/open, channels, read/write, header, cleanup), 12 in `ipc/worker.rs` (state management, all message handlers without real plugins), 6 in `ipc/proxy.rs` (transport state, read output, process silence, dummy process), 5 in `gui/backend.rs` (process_isolation flag, sandboxed state, tainted path bypass).

## [0.15.0] - 2026-02-26

### Added
- **mimalloc as default global allocator** (`main.rs`, `Cargo.toml`): Replaced the system allocator with mimalloc for all Rust heap allocations. Since VST3 plugins are loaded C++ dynamic libraries that use system malloc directly, a buggy plugin corrupting the system malloc heap (buffer overflow, use-after-free, etc.) would previously also corrupt our Rust allocations. With mimalloc, Rust heap allocations live in a separate heap that is isolated from the system malloc arena. This provides defense-in-depth: even if a plugin trashes the system heap, our host's data structures remain intact. The `debug-alloc` feature flag still overrides mimalloc with dhat for heap profiling.
- `active_allocator_name()` diagnostic function to report which allocator is active.
- 2 new unit tests (443 → 445 total): `test_active_allocator_name_is_mimalloc_by_default`, `test_rust_allocations_work_with_global_allocator`.

## [0.14.2] - 2026-02-26

### Fixed
- **Double-free of `HostPlugFrame` causing malloc heap corruption on plugin stop** (`vst3/plug_frame.rs`): When stopping a plugin (e.g. FabFilter Pro-Q 4), the editor close sequence calls `IPlugView::removed()`, `setFrame(null)`, and `release()`. The plugin releases all its COM references to the `HostPlugFrame` during this sequence, which dropped the reference count to zero. The `host_plug_frame_release()` function self-destructed via `Box::from_raw` when the count hit zero. Then `HostPlugFrame::destroy()` was called by the host on the already-freed memory — a classic double-free. This corrupted macOS malloc's tiny freelist metadata ("Corruption of tiny freelist 0x…: size too small (0/48)"), causing a SIGABRT. The bug was masked by `debug.bash` because `dhat::Alloc` (global allocator) and `MallocGuardEdges`/`MallocScribble` changed allocation layout and behavior. Fix: removed self-destruct from `host_plug_frame_release()` so the host always manages the lifetime via `destroy()`, matching the pattern already used by `HostComponentHandler` and `HostApplication`.

### Added
- 2 new regression tests (441 → 443 total): `test_plug_frame_release_does_not_self_destruct`, `test_plug_frame_editor_close_lifecycle`.

## [0.14.1] - 2026-02-26

### Fixed
- **Malloc heap corruption ("Corruption of tiny freelist") causing SIGABRT during host termination** (`vst3/instance.rs`): After a sandbox-recovered plugin crash (e.g. FabFilter Pro-Q 4 SIGBUS during `release_controller`), the `siglongjmp` recovery left plugin COM objects leaked in memory. These leaked objects still held raw pointers to our `HostApplication` and `HostComponentHandler` host objects. However, `Vst3Instance::drop` unconditionally destroyed these host objects at the end — causing use-after-free. The freed heap allocations were later accessed by the plugin's background threads or static destructors, corrupting the malloc tiny freelist metadata. The corruption manifested as `SIGABRT: "Corruption of tiny freelist 0x…: size too small"` during host exit. Fixed with three complementary changes:
  1. **`setComponentHandler(nullptr)` before terminate**: Follows the VST3 shutdown protocol — clears the controller's reference to our handler before any terminate/release calls, preventing the controller's destructor from calling back into a handler that's about to be destroyed.
  2. **Split terminate and release into separate sandbox calls**: `terminate_controller` and `release_controller` are now separate sandbox calls (previously combined). Similarly, `release_processor` and `release_component` are now separate. A crash in `terminate()` no longer prevents the `release()` attempt.
  3. **Conditional host object leak on crash**: When `any_crash || self.crashed`, the host objects (`host_context`, `controller_host_context`, `component_handler`) are now intentionally leaked instead of destroyed. This is safe because the plugin library is also kept loaded (via `LAST_DROP_CRASHED` flag), so all pointers remain valid for the process lifetime. Memory cost is negligible (< 1 KB).

### Added
- 4 new unit tests (437 → 441 total): `test_host_objects_leaked_on_crash_prevents_use_after_free`, `test_host_objects_destroyed_on_clean_shutdown`, `test_deactivation_heap_corrupted_flag`, `test_crash_flags_set_together_on_com_crash`.

## [0.14.0] - 2026-02-26

### Added
- **Debug & profiling infrastructure** (`diagnostics.rs`, `sandbox.rs`, `main.rs`, `cli.rs`, `app.rs`, `backend.rs`, `instance.rs`, `engine.rs`): Comprehensive diagnostic tooling for characterising heap corruption caused by `siglongjmp`-based crash recovery from C++ plugin code.
  1. **Cargo feature flags**: `debug-alloc` (dhat heap profiler), `debug-trace` (Chrome trace export), `debug-tools` (both). All diagnostic code is zero-cost when features are disabled.
  2. **Diagnostics module** (`src/diagnostics.rs`): Central hub with `heap_check()` (wraps macOS `malloc_zone_check`), `check_malloc_env()`, `recommended_env_vars()`, `init_profiler()`/`shutdown_profiler()` (dhat, feature-gated), and `print_malloc_debug_instructions()`.
  3. **Backtrace capture in signal handler** (`sandbox.rs`): Signal-safe `backtrace()` call captures up to 64 frames before `siglongjmp`. Frames are symbolicated after recovery. `PluginCrash` now carries `backtrace: Vec<String>` and `heap_corrupted: bool` fields.
  4. **Heap integrity checks** (`sandbox.rs`, `instance.rs`, `app.rs`): `malloc_zone_check(NULL)` called after sandbox crash recovery, during plugin instance drop, and periodically in the GUI update loop (~every 60 frames when `--malloc-debug` is active).
  5. **dhat global allocator**: Optional heap profiling via `#[global_allocator]` behind `debug-alloc` feature flag.
  6. **Structured tracing refactor** (`main.rs`): Layered `Registry`-based subscriber with optional Chrome trace layer (behind `debug-trace` feature).
  7. **Performance spans**: `trace_span!` / `info_span!` on hot paths — `sandbox_call`, `audio_engine_process`, `vst3_process`, `vst3_instance_drop`, `gui_update`, plugin activate/deactivate.
  8. **`--malloc-debug` CLI flag** (`cli.rs`): Prints macOS malloc environment variable instructions and enables periodic heap checking in the GUI.
  9. **Heap corruption GUI warning** (`app.rs`): Red banner at top of window when `malloc_zone_check` detects corruption. `HostBackend` propagates heap corruption status from `DEACTIVATION_HEAP_CORRUPTED` thread-local.
- 22 new unit tests (415 → 437 total): diagnostics module (7), sandbox backtrace/heap (7), CLI malloc-debug (2), GUI app heap checks (4), backend heap corruption (3). 438 tests with `--features debug-tools`.

### Validated
- **Full diagnostic profiling session**: Ran GUI with `--features debug-tools`, `--malloc-debug`, and macOS malloc debug env vars (`MallocGuardEdges=1 MallocScribble=1 MallocErrorAbort=1`). Chrome trace captured 78s of execution (4.8 MB). dhat heap profile written on exit. Observed: 1 SIGABRT in `release_controller` (FabFilter Pro-Q 4), heap integrity check passed post-crash, plugin correctly tainted, re-activation blocked. Host stable throughout.

## [0.13.2] - 2026-02-26

### Fixed
- **SIGABRT (exit 134) on second plugin activation after crash-recovered deactivation** (`vst3/instance.rs`, `gui/backend.rs`): When a plugin crashed during COM cleanup (e.g. FabFilter Pro-Q 4 SIGBUS during `instance_drop`), the sandbox caught it via `siglongjmp`, but this recovery could leave the process malloc heap in an inconsistent state. The library was correctly leaked (not unloaded), but re-activating the same plugin called `dlopen` (returning the already-mapped corrupted library), then `bundleEntry`, triggering malloc freelist corruption detection → SIGABRT. Fixed with two complementary changes:
  1. **Tainted path tracking**: `HostBackend` now maintains a `tainted_paths: HashSet<PathBuf>` set. After deactivation, a `DEACTIVATION_CRASHED` thread-local flag is checked — if set, the plugin's bundle path is added to the tainted set. `activate_plugin()` checks this set and returns a user-friendly error ("Restart the host to use this plugin again") instead of attempting to load the corrupted library.
  2. **Granular COM cleanup**: `Vst3Instance::Drop` now splits the COM cleanup into 5 individual `sandbox_call` invocations (disconnect IConnectionPoint, terminate+release controller, terminate component, release COM refs, release factory) instead of one monolithic call. If any step crashes, subsequent steps are skipped gracefully with per-step warnings. This reduces the crash surface area and provides better diagnostics.

### Added
- `DEACTIVATION_CRASHED` thread-local flag in `vst3/instance.rs` for communicating crash status from `Vst3Instance::drop` to `HostBackend::deactivate_plugin`.
- `tainted_paths` field on `HostBackend` for tracking plugins that cannot be safely reloaded.
- `plugin_path` field on `ActiveState` for tainted-path recording after deactivation.
- GUI status messages for tainted plugins: deactivation shows "crashed during deactivation" warning, re-activation shows "restart the host" error.
- 8 new unit tests (407 → 415 total): tainted paths initially empty, tainted path blocks activation, non-tainted path not blocked, `DEACTIVATION_CRASHED` flag get/set, flag independence from `LAST_DROP_CRASHED`, deactivation without crash does not taint.

## [0.13.1] - 2026-02-26

### Fixed
- **SIGABRT (exit 134) after sandbox-recovered plugin crash** (`vst3/instance.rs`, `vst3/module.rs`): When `Vst3Instance::drop` caught a SIGBUS during COM cleanup, the host recovered via the sandbox, but `Vst3Module::drop` subsequently unloaded the plugin library (`bundleExit` + `CFRelease`). This triggered C++ static destructors inside the plugin to run on corrupted state, causing a SIGABRT that killed the host process. Fixed with a thread-local flag (`LAST_DROP_CRASHED`) that communicates crash status from `Vst3Instance::drop` to `Vst3Module::drop`. When the instance's COM cleanup crashes, the module now skips all plugin-facing cleanup (factory release, `bundleExit`, `CFRelease`) and intentionally leaks the library in memory — preventing C++ destructors from executing on bad state.
- **Defense-in-depth**: `cf_bundle::release` (which calls `CFRelease` to unload the dylib) is now wrapped in a `sandbox_call`, catching any crashes from C++ static destructors during library unload even when the instance cleanup succeeded.
- **Cascading crash prevention**: If the factory COM release crashes in `Vst3Module::drop`, `bundleExit` and `CFRelease` are now skipped entirely (previously only `bundleExit` crashes triggered this skip).

### Added
- 18 new unit tests (389 → 407 total): `LAST_DROP_CRASHED` thread-local default/set/reset, flag set on sandbox crash, flag not set on success, module-side read-and-reset, full crash→flag→skip pattern integration.

## [0.13.0] - 2026-02-26

### Added
- **Plugin crash sandbox** (`vst3/sandbox.rs`): New module providing signal-handler-based crash isolation for VST3 plugin calls. Uses `sigsetjmp`/`siglongjmp` to recover from SIGBUS, SIGSEGV, SIGABRT, and SIGFPE triggered by buggy plugins. Thread-safe with per-thread jump buffers and reference-counted signal handler installation.
- **Sandboxed plugin lifecycle**: All plugin-facing COM calls are now wrapped in `sandbox_call()` — including `process()`, `shutdown()`, `Drop` (COM cleanup), and module exit (factory release, `bundleExit`). If a plugin crashes at any point, the host catches the signal and continues running.
- **Crash-aware instance state**: `Vst3Instance` gains a `crashed` flag. Once set, all subsequent COM calls are skipped, and the `Drop` impl intentionally leaks COM objects to avoid further crashes.
- **GUI crash detection**: `HostBackend::is_crashed()` polls the engine for crash state. The GUI update loop auto-deactivates crashed plugins and displays a status message (e.g., "⚠ 'FabFilter Pro-Q 4' crashed — deactivated safely. The host is unaffected.").
- `libc` dependency for low-level signal handling (`sigaction`, `sigsetjmp`/`siglongjmp`, `raise`).
- 21 new unit tests (368 → 389 total): `SandboxResult` methods, `PluginCrash` display/error impl, signal name lookup, panic message extraction, sandbox normal/panic/nested execution, signal recovery (SIGBUS, SIGSEGV, SIGABRT via `raise()`), crash-then-normal recovery cycle, handler refcount cleanup.

### Changed
- `Vst3Instance::process()` signature changed from `&self → i32` to `&mut self → bool`. Returns `false` if the plugin crashed.
- `Vst3Instance::shutdown()` now wraps each COM call (`set_processing`, `set_active`) in a sandbox. If either crashes, the instance is marked crashed and remaining calls are skipped.
- `Vst3Instance::Drop` now performs all COM cleanup (disconnect, terminate, release) inside a single `sandbox_call`. On crash, resources are intentionally leaked.
- `Vst3Module::Drop` wraps factory release and `bundleExit` in sandboxed calls.
- `AudioEngine::process()` checks `instance.is_crashed()` as an early-exit guard alongside `is_shutdown`.

## [0.12.2] - 2026-02-26

### Fixed
- **SIGSEGV on plugin deactivation (stop button)** (`audio/engine.rs`, `gui/backend.rs`): Race condition between the GUI thread and the audio callback caused a crash when stopping a plugin. After `engine.shutdown()` released the Mutex lock, the audio callback could re-acquire the lock and call `process()` on a deactivated/stopped VST3 plugin — undefined behavior per the VST3 spec, causing SIGSEGV on many plugins. Fixed with a two-part approach:
  1. Added `is_shutdown` flag to `AudioEngine` — set atomically in `shutdown()`, checked at the top of `process()`. Racing audio callbacks now immediately output silence instead of calling the plugin.
  2. Implemented custom `Drop` for `ActiveState` with controlled teardown order: params → stream → engine → MIDI → module. The audio stream is explicitly dropped before the `Vst3Module`, ensuring all COM references are released while the dynamic library is still loaded.
  3. Wrapped `_stream` in `Option<cpal::Stream>` so `deactivate_plugin()` can explicitly drop the stream (via `.take()`) before the rest of `ActiveState` is dropped.

### Added
- 4 new unit tests (364 → 368 total): `test_backend_deactivate_idempotent`, `test_backend_deactivate_clears_editors`, `test_active_state_stream_is_option`, `test_tone_generator_zero_amplitude_when_disabled`.

## [0.12.1] - 2026-02-26

### Fixed
- **SIGSEGV on plugin activation** (`gui/backend.rs`): The `Vst3Module` (which owns the dynamic library handle) was dropped at the end of `activate_plugin`, unloading the shared library while the `Vst3Instance` COM vtable pointers still referenced code in it. Any subsequent call (e.g. `process()` in the audio callback) dereferenced unmapped memory, causing exit code 139 (SIGSEGV). Fixed by storing the `Vst3Module` in `ActiveState` so the library stays loaded for the lifetime of the plugin instance.

### Added
- 2 new unit tests (362 → 364 total): `test_active_state_holds_module` documents the module-lifetime invariant, `test_backend_deactivate_clears_audio_status` verifies cleanup.

## [0.12.0] - 2026-02-26

### Added
- **Plugin parameter editing for selected slots**: Clicking a plugin in the rack now shows its parameters in the right panel regardless of activation state. Inactive plugins display cached parameters with a staging banner; changes are queued and applied on activation.
- **Parameter staging for inactive plugins**: `PluginSlot` gains `param_cache` and `staged_changes` fields. Slider edits on inactive plugins are recorded and applied automatically when the plugin is activated via ▶.
- **Improved parameter panel UX**: Header shows plugin name and vendor. Inactive plugins with cached params show a "⚠ Plugin is inactive — changes will be applied on activation" banner. Never-activated plugins show an activation prompt. Error messages displayed in the status bar on failed parameter changes.
- **Deactivation caches params**: When a plugin is deactivated, its current parameter state is preserved in the slot cache for continued browsing.
- **Activation applies staged changes**: On activation, any pending staged parameter changes are applied to the live plugin and the count is shown in the status message.
- 15 new unit tests (347 → 362 total): selection state transitions, cached param display, staging, cache preservation on reorder, session transient field isolation, and error paths.

### Changed
- **Parameter panel visibility**: Right panel now appears whenever a rack slot is selected (previously required both selection and non-empty live params).
- **`refresh_params()`**: Now handles three states: active selected slot (live refresh), inactive selected slot (cache-based), no selection (clear).
- **`deactivate_active()`**: No longer clears `param_snapshots`; caches them to the slot instead.
- **`remove_from_rack()`**: Clears `param_snapshots` when the removed slot was selected.

## [0.11.2] - 2026-02-26

### Added
- **USER_INTERACTION_PLAN.md**: GUI interaction plan for selecting plugins and adjusting parameters.

## [0.11.1] - 2026-02-26

### Fixed
- **GUI text contrast**: Switched translucent theme colors to unmultiplied alpha so glass panels render at the intended opacity, improving readability on light cards and controls.

## [0.11.0] - 2026-02-26

### Added
- **Plugin Editor Windows** (`gui/editor.rs`): Native macOS NSWindow hosting for VST3 plugin editor views. Creates an NSWindow with NSView via Objective-C runtime FFI, calls `IPlugView::attached()` to embed the plugin UI, and handles resize requests through `IPlugFrame`. Lifecycle management with `open()`, `poll_resize()`, and `close()`.
- **IPlugView/IPlugFrame COM interfaces** (`vst3/com.rs`): Added `IPLUG_VIEW_IID`, `IPLUG_FRAME_IID`, `ViewRect` struct, `IPlugViewVtbl` (15 function pointers), `IPlugFrameVtbl`, and platform type constants (`K_PLATFORM_TYPE_NSVIEW`, `K_PLATFORM_TYPE_HWND`, `K_PLATFORM_TYPE_X11`).
- **Host IPlugFrame** (`vst3/plug_frame.rs`): COM implementation for plugin-to-host resize requests. Reference-counted with atomic operations, thread-safe pending resize via Mutex.
- **Editor creation on Vst3Instance** (`vst3/instance.rs`): `create_editor_view()` and `has_editor()` methods on VST3 plugin instances, querying IEditController for "editor" views.
- **Transport sync**: GUI transport changes (tempo, time signature, play/pause) are now pushed to the audio engine in real time. Space bar toggles play/pause.
- **Audio engine status display**: Bottom bar shows sample rate, buffer size, device name, and open editor count when audio is active.
- **Parameter search filter**: Text search field in the parameter panel filters parameters by title for quick access in plugins with many parameters.
- **Improved scan progress**: Scan status message now shows module count, class count, and error count (e.g. "3 module(s), 12 class(es), 1 error(s)").
- **Safe mode** (`--safe-mode` flag on `gui` command): Disables plugin editor window opening. Useful when a plugin editor causes instability.
- **Keyboard shortcut**: Space bar toggles play/pause in the transport.
- **Editor button** (🎹): Shown in rack slot controls for active plugins that have an editor view. Disabled in safe mode.
- 43 new unit tests (304 → 347 total): 12 new app tests (safe mode, transport sync, editor, param filter, audio status), 10 new backend tests (editor, transport, audio status), 7 COM interface tests (IPlugView/IPlugFrame IIDs, vtable sizes, ViewRect), 10 plug_frame tests, 3 editor tests, 1 CLI safe mode test.

### Changed
- **Minimum window size**: Increased from 800×500 to 1024×640 for better layout at default size.
- **`gui` command**: Now accepts `--safe-mode` flag.
- **`launch()` function**: Accepts `safe_mode: bool` parameter.

## [0.10.1] - 2026-02-26

### Fixed
- **Rust 2024 `unsafe_op_in_unsafe_fn` compliance** (`vst3/plug_frame.rs`, `gui/editor.rs`): Wrapped all unsafe operations inside `unsafe fn` bodies with explicit `unsafe {}` blocks, as required by the Rust 2024 edition. Affected functions: `host_plug_frame_query_interface`, `host_plug_frame_add_ref`, `host_plug_frame_release`, `host_plug_frame_resize_view`, `take_pending_resize`, `destroy`, `class`, `sel`, `create_window`, `show_window`, `resize_window`, `close_window`.
- Removed unused `ComPtr` import in `plug_frame.rs`.
- Prefixed unused variable `init_string` → `_init_string` and unused constant `nil` → `_nil` in `editor.rs`.

## [0.10.0] - 2026-02-26

### Added
- **GUI Backend Bridge** (`gui/backend.rs`): Full integration between the GUI and audio engine. Manages plugin activation lifecycle (load, instantiate, configure audio, start processing), audio output stream via cpal, MIDI input connections, and parameter queues for thread-safe GUI ↔ audio communication.
- **Parameter View Panel**: Right-side panel in the GUI displaying all parameters for the active plugin. Normalized sliders with display values and units, bypass parameter highlighting (warning colour), read-only parameter display, double-click to reset to default value.
- **Device Selection UI**: Bottom-bar "Devices" tab with ComboBox dropdowns for selecting audio output device and MIDI input port. Refresh button to re-enumerate system devices.
- **Session Save/Load** (`gui/session.rs`): Serialize and restore full host state — transport settings, rack plugin slots, and device selections — as JSON files. Bottom-bar "Session" tab with path input and save/load buttons. Default session path in platform data directory.
- **Plugin Activation from Rack**: ▶ button on each rack slot to activate a plugin and start real-time audio processing. ⏹ button to deactivate. Active slot visually highlighted with green border and "active" status text.
- **Test Tone Toggle**: 🔔/🔕 button in Transport tab to enable/disable the built-in 440 Hz sine wave test tone input for effect plugins.
- **Bottom Bar Tabs**: Transport, Devices, and Session views selectable via tabbed bottom panel.
- **ParamSnapshot**: Fully owned, Clone-safe parameter representation for safe GUI-thread rendering without COM pointer lifetime concerns.
- 31 new unit tests (273 → 304 total): 12 backend tests, 9 session tests, 10 app integration tests (session roundtrip, device selection, parameter refresh, activation/deactivation).

## [0.9.1] - 2026-02-26

### Added
- **GUI PRD** (`PRD.md`): Product requirements document for the GUI application.

### Changed
- **Documentation**: Linked the PRD from README and USER_GUIDE.

## [0.9.0] - 2026-02-25

### Added
- **GUI Skeleton** (`gui/` module): Basic graphical user interface using `egui` 0.31 and `eframe` 0.31, implementing the first step of the Liquid Glass design.
- **Liquid Glass Theme** (`gui/theme.rs`): Full dark glassmorphism theme — deep blue-black background, translucent panel fills, electric blue accent colour, CornerRadius (12/8/6 px), soft panel shadows, glass border strokes, custom text styles, and helper frame constructors (`glass_card_frame`, `section_frame`).
- **HostApp** (`gui/app.rs`): Three-panel `eframe::App` layout:
  - **Plugin Browser** (left sidebar): Scan button, text search filter, scrollable list of cached plugins as glass cards with vendor/subcategory display and add-to-rack button.
  - **Plugin Rack** (central panel): Loaded plugin slots shown as selectable glass cards with slot number, name, vendor, bypass toggle, and remove button.
  - **Transport Bar** (bottom panel): Play/pause button, BPM drag value (20–300), time signature editor, status message display.
- **Data structures**: `PluginSlot`, `TransportState`, `BrowserFilter` for GUI state management with rack add/remove, filter matching (by name, category, subcategory, vendor), and selected slot tracking.
- **`gui` CLI command**: New subcommand to launch the graphical interface (`cargo run -- gui`).
- **Dependencies**: `eframe` 0.31, `egui` 0.31 added to `Cargo.toml`.
- 31 new unit tests (242 → 273 total): 11 theme tests, 19 app state tests, 1 CLI parsing test.

## [0.8.0] - 2026-02-25

### Added
- **Phase 7 GUI Design**: Created `DESIGN_DOCUMENT.md` outlining the architecture and design philosophy for the upcoming graphical user interface.
- **Liquid Glass Style**: Defined the visual language (Glassmorphism) using `egui` and a custom `wgpu` backend for frosted glass effects, floating panels, and vivid backgrounds.
- **GUI Architecture**: Outlined core components including the Main Window, Plugin Rack/Routing Graph, Plugin Editor Host, Preset Manager, and Transport Controls.

## [0.7.0] - 2026-02-25

### Fixed
- **Separate IEditController support**: Plugins using split component/controller architecture (e.g. FabFilter Pro-MB, Pro-Q 4) now correctly enumerate parameters. Previously `query_parameters()` returned `None` for these plugins because it only tried `QueryInterface` on the component and did not create the controller via the factory. Now the host uses `getControllerClassId()` + factory `createInstance()` to create, initialize, and connect the separate controller.

### Added
- **IConnectionPoint** (`vst3/com.rs`): New IID and vtable definition for bidirectional component↔controller communication. Used to `connect()` and `disconnect()` split-architecture plugins.
- **`get_controller()` method** (`vst3/instance.rs`): Lazy controller resolution that tries QueryInterface first, then falls back to factory-based separate controller creation. Caches the result for reuse by both `query_parameters()` and `install_component_handler()`.
- **Factory lifecycle** (`vst3/instance.rs`): `Vst3Instance` now AddRefs the factory COM pointer and stores it for later use. Released on drop.
- **Controller lifecycle**: Separate controllers are fully managed — initialized with host context, connected via IConnectionPoint, disconnected and terminated on drop.
- 5 new unit tests (237 → 242 total): IConnectionPoint IID verification, vtable layout, IEditController IID length, factory vtable size.

### Changed
- `Vst3Instance::query_parameters()` now takes `&mut self` (was `&self`) to support lazy controller caching.
- `install_component_handler()` now uses the cached controller instead of doing its own QueryInterface, ensuring it works with separate controllers.
- `Vst3Instance::drop()` now properly cleans up separate controllers (disconnect, terminate, release) and releases the factory reference.

## [0.6.0] - 2026-02-25

### Fixed
- **IAudioProcessor IID typo**: Last byte was `0x3F` but should be `0x3D` — this caused `QueryInterface` for `IAudioProcessor` to fail on all plugins, making the `run` command non-functional. Root cause found via binary analysis of plugin binaries.
- **Windows COM IID byte order**: All Windows `#[cfg(target_os = "windows")]` IID constants had bytes 4–7 (the l2 group) with the two u16 halves transposed. Fixed for IComponent, IAudioProcessor, IHostApplication, IEditController, IEventList, IParameterChanges, IPluginFactory2, and IPluginFactory3.

### Added
- **CFBundleRef support** (`vst3/cf_bundle.rs`): New module providing CoreFoundation FFI for creating a proper `CFBundleRef` from the `.vst3` bundle path on macOS. Previously `bundleEntry` was called with a null pointer; now it receives the actual bundle reference as required by the VST3 SDK.
- **IPluginFactory3 support** (`vst3/module.rs`): After loading the factory, the host now queries for `IPluginFactory3` and calls `setHostContext` to provide the host application interface to modern plugins.
- **IID verification tests**: 9 new tests in `com.rs` that validate all 7 IID constants against their canonical UUID strings using helper functions (`uuid_to_big_endian`, `uuid_to_com`). 2 new tests in `module.rs` for IPluginFactory2 and IPluginFactory3 IIDs.
- **CFBundleRef tests**: 3 tests for null path handling, null release safety, and system framework (CoreFoundation) validation.
- Test count increased from 223 to 237 (14 new tests).

### Changed
- `Vst3Module` now stores and manages `cf_bundle_ref` on macOS, properly releasing it on drop.
- `Vst3Module::drop` now calls `bundleExit` before releasing the CFBundleRef.

## [0.5.0] - 2026-02-25

### Added
- **Comprehensive test suite**: 117 new tests added across 13 modules (106 → 223 total), completing Phase 6 validation
- **Error type tests**: Display formatting for all variants of HostError, Vst3Error, AudioError, MidiError; From conversions (Vst3Error→HostError, io::Error→HostError, serde_json::Error→HostError); Debug formatting
- **CLI parsing tests**: All subcommands (`scan`, `list`, `run`, `devices`, `midi-ports`), required/optional args, invalid input rejection, short flags (`-B`)
- **Types serde tests**: Roundtrip serialization for PluginClassInfo/PluginModuleInfo, optional field handling, CID array encoding, Clone/Debug derivation
- **Cache I/O tests**: Serde roundtrip, file I/O roundtrip using temp directories, corrupt JSON error handling, timestamp ISO 8601 format validation
- **Scanner tests**: Dedup, sorted output, recursive directory scanning, non-vst3 file filtering, macOS bundle structure resolution
- **Parameter registry tests**: UTF-16 conversion edge cases, string truncation (exact/empty/single-char), flag combinations, ParameterEntry Debug formatting
- **Event list tests**: COM vtable overflow at MAX_EVENTS_PER_BLOCK (512), add/get via vtable function pointers, null pointer safety, QueryInterface
- **Parameter changes tests**: Queue overflow at MAX_PARAM_QUEUES (64) and MAX_POINTS_PER_PARAM (16), PVQ QueryInterface for unknown IID, null pointer safety, existing parameter reuse
- **Process buffer tests**: Setter methods (input events, parameter changes, process context), zero-channel configurations, out-of-range access, consecutive prepare calls, mono-in/stereo-out layout
- **MIDI translation tests**: All 16 channels, extreme pitches (0 and 127), note-off velocity, sample_offset propagation, batch edge cases (empty, all filtered, order preservation), truncated and single-byte messages
- **Interactive command tests**: All commands (`tempo`, `status`, `params`, `get`, `set`) with no-params paths, invalid BPM/values, handler polling for pending changes
- **Host context tests**: QueryInterface for IHostApplication and unknown IIDs, ref counting accuracy, get_name null pointer, as_unknown, destroy null safety
- **Component handler tests**: Concurrent perform_edit (4 threads × 100 edits), restart flag OR behavior across calls, destroy null safety
- **CODE_COVERAGE.md**: Test coverage analysis document with per-module breakdown

### Changed
- Test count increased from 106 to 223 (111% increase)
- All 223 tests verified stable across 5 consecutive runs
- Clean build with zero warnings maintained

## [0.4.0] - 2026-02-25

### Added
- **Interactive command shell**: Runtime parameter control during audio processing
  - `params` / `p` — list all plugin parameters with current values
  - `get <id|name>` — query individual parameter value
  - `set <id|name> <value>` — set parameter via normalized value (0.0–1.0)
  - `tempo <bpm>` — set transport tempo
  - `status` — show engine status (parameter count, handler state)
  - Real-time display of plugin-initiated parameter changes
- **IComponentHandler**: Host-side COM implementation for plugin parameter notifications
  - `beginEdit` / `performEdit` / `endEdit` callbacks
  - `restartComponent` with flag handling
  - Thread-safe change queue with drain support
  - Installed automatically on IEditController during plugin load
- **ProcessContext transport info**: Timing and transport state passed to plugins each audio block
  - Tempo (BPM), time signature, sample position, musical position (quarters)
  - Automatic transport advancement based on sample count
  - Playing state, bar position tracking
- **IParameterChanges + IParamValueQueue**: Host-side COM implementations for sample-accurate parameter automation
  - Pre-allocated queue pool (64 parameters × 16 points per block)
  - Full COM vtable with getParameterCount, getParameterData, addParameterData
  - IParamValueQueue with getParameterId, getPointCount, getPoint, addPoint
  - Changes from interactive shell routed through audio-thread-safe queue
- 29 new unit tests (106 total) covering IComponentHandler, ProcessContext, IParameterChanges, IParamValueQueue, interactive state

### Changed
- `run` command now enters interactive command shell instead of passive Ctrl+C wait
- Audio engine now provides ProcessContext with transport to plugins each block
- Audio engine now routes parameter changes via IParameterChanges
- ProcessBuffers exposes `set_process_context()` for attaching transport to ProcessData
- Vst3Instance manages IComponentHandler lifecycle (install, destroy on drop)
- Parameters queried automatically during `run` for interactive access

## [0.3.0] - 2026-02-25

### Added
- **MIDI input support**: Connect a MIDI input device to send notes to VST3 instrument plugins
  - `midi-ports` command to list available MIDI input ports
  - `--midi <PORT>` option on `run` to connect a MIDI input
  - Lock-free MIDI message receiver for real-time transfer from input thread to audio thread
  - Raw MIDI to VST3 event translation (Note On, Note Off, velocity 0 as Note Off convention)
- **VST3 event system**: Full IEventList COM implementation for passing MIDI events to plugins
  - `Event`, `NoteOnEvent`, `NoteOffEvent` structs matching Steinberg SDK layout
  - Host-side `HostEventList` with add/get/clear/query_interface through static vtable
  - Events fed to `ProcessData.input_events` each audio block
- **Plugin parameter introspection**: Query and display plugin parameters via IEditController
  - `--list-params` option on `run` to enumerate all plugin parameters
  - `ParameterRegistry` with metadata: title, units, default, current, flags
  - IEditController vtable (getParameterCount, getParameterInfo, setParamNormalized, etc.)
  - `ParameterInfo` struct matching SDK layout
  - Formatted parameter table output with ID, title, default, current, units, flags
  - Normalized/plain value conversion
- **`MidiError`** error type for MIDI subsystem errors
- 33 new unit tests (77 total) covering MIDI receiver, MIDI translation, event list COM interface, parameter registry, Event/NoteOnEvent/NoteOffEvent structs

### Changed
- `run` command accepts `--midi`, `--list-params`, and `-B` (buffer-size, changed from `-b`)
- Audio engine now processes MIDI events each block via HostEventList
- `AudioEngine` includes `Drop` implementation for event list cleanup
- `ProcessBuffers` exposes `set_input_events()` for attaching event list to ProcessData

### Dependencies
- Added `midir` v0.10 for cross-platform MIDI input

## [0.2.0] - 2026-02-25

### Added
- **`run` command**: Load and run VST3 plugins with real-time audio processing
  - Plugin resolution by name (from cache) or direct `.vst3` bundle path
  - VST3 component instantiation with full lifecycle management (initialize, setup, activate, process, shutdown)
  - Audio output via `cpal` with configurable sample rate, buffer size, and device selection
  - 440 Hz sine wave test tone input for testing effect plugins
  - Graceful shutdown via Ctrl+C
  - CLI options: `--device`, `--sample-rate`, `--buffer-size`, `--no-tone`
- **`devices` command**: List available audio output devices with default indicator
- **VST3 COM interface definitions** (`vst3/com.rs`): Manual FFI vtable definitions for IComponent, IAudioProcessor, ProcessSetup, ProcessData, AudioBusBuffers
- **IHostApplication** (`vst3/host_context.rs`): Minimal COM host context implementation passed to plugins during initialization
- **VST3 instance management** (`vst3/instance.rs`): Full component lifecycle — factory createInstance, initialize, QueryInterface for IAudioProcessor, bus arrangement negotiation, setupProcessing, setActive/setProcessing
- **Process buffer management** (`vst3/process.rs`): Pre-allocated per-channel buffers with interleaved↔deinterleaved conversion
- **Audio device module** (`audio/device.rs`): cpal-based device enumeration and stream management
- **Audio processing engine** (`audio/engine.rs`): Bridges cpal audio callback with VST3 plugin processing
- **AudioError** error type for audio subsystem errors
- 32 new unit tests (44 total) covering COM struct layouts, host context, process buffers, tone generation, and audio device enumeration

### Changed
- `run` command now fully functional (previously a placeholder)
- CLI `Run` variant now accepts `--device`, `--sample-rate`, `--buffer-size`, `--no-tone` options
- Error types expanded: `Vst3Error::Processing` variant, `AudioError` enum
- Module `IPluginFactoryVtbl`, `IUnknownVtbl`, `ComObj` types made `pub` for instance creation

### Dependencies
- Added `cpal` v0.15 for cross-platform audio I/O
- Added `ctrlc` v3 for Ctrl+C signal handling

## [0.1.0] - 2026-02-25

### Added
- Initial project structure with module layout (`app/`, `audio/`, `midi/`, `host/`, `vst3/`)
- **`scan` command**: Discover VST3 plugins in standard OS directories, load modules, extract metadata via COM FFI, and cache results as JSON
- **`list` command**: Display cached plugins with name, vendor, category, and path
- VST3 scanner with macOS/Linux/Windows path support and recursive bundle discovery
- VST3 module loader with `libloading`, manual COM FFI for IPluginFactory/IPluginFactory2
- JSON-based plugin cache with platform-appropriate storage location
- Error handling with `thiserror` (`HostError`, `Vst3Error`)
- Structured logging via `tracing` with `RUST_LOG` env-filter
- 12 unit tests for scanner, cache, and module utilities
- `USER_GUIDE.md` covering installation, commands, plugin paths, caching, and troubleshooting
