# Changelog

All notable changes to this project will be documented in this file.

## [0.20.1] - 2026-02-28

### Fixed
- **All remaining clippy warnings** (26 errors under `-D warnings`): Codebase now passes `cargo clippy --lib --tests -- -D warnings` cleanly.
  - Removed unused import `SandboxResult` in `src/gui/editor.rs`
  - Removed unused import `PluginSlot` in `src/gui_tests.rs`
  - Removed unnecessary `unsafe` block in `src/vst3/instance.rs` test
  - Replaced 3 irrefutable `let Err(e) = result else { unreachable!() }` with `result.unwrap_err()` in `src/vst3/sandbox.rs`
  - Replaced 2 `assert_eq!(x, true)` with `assert!(x)` in `src/gui/session.rs`
  - Added `#[allow(clippy::assertions_on_constants)]` to theme constant test in `src/gui/theme.rs`
  - Added `#[allow(clippy::too_many_arguments)]` to `point_in_triangle` in `src/gui_tests.rs`
  - Replaced `changes.len() > 0` with `!changes.is_empty()` in `src/asan_tests.rs`
  - Converted 4 needless range loops to `iter_mut().enumerate()` in `src/e2e_tests.rs`
  - Refactored 12 `Default::default()` + field reassignment patterns to struct initializer syntax in `src/gui/app.rs`

### Changed
- Version bumped to 0.20.1
- 726 tests passing (all green)

## [0.20.0] - 2026-02-28

### Added
- **IBStream COM implementation** (`src/vst3/ibstream.rs`): Full `IBStream` interface for VST3 plugin state transfer — `read`, `write`, `seek`, `tell` over a `Vec<u8>` backing buffer. System-heap allocated via `host_alloc::system_alloc` for COM compatibility. `HostBStream::new()`, `from_data()`, `as_ptr()`, `take_data()`, `destroy()`. 6 new tests.
- **Split-architecture controller state sync** (`src/vst3/instance.rs`): New `sync_component_state_to_controller()` method calls `IComponent::getState()` → `IEditController::setComponentState()` after controller initialization. This is required by the VST3 spec for split-component plugins (e.g., JUCE-based plugins like Monster) where the controller needs the component's state before it can create editor views.

### Fixed
- **Monster VST editor not opening**: JUCE-based plugins with separate component/controller architecture require `setComponentState()` to be called on the controller before `createView()` will succeed. Added IBStream implementation and state sync call to the plugin initialization flow.
- **Editor window opens behind main window** (`src/gui/editor.rs`): Plugin editor windows now call `orderFrontRegardless` after `makeKeyAndOrderFront:` to ensure they appear in front of the main application window on macOS.
- **Plugin list shows non-audio classes**: Both `app.rs` and `gui_worker.rs` now filter `filtered_classes()` to only show entries with `category == "Audio Module Class"`, hiding Component Controller and Plugin Compatibility classes that aren't directly loadable.
- **30+ Clippy lint warnings**: Fixed all warnings across 15+ files — collapsed nested `if`s, removed identical branches, replaced manual Default impls with `#[derive(Default)]`, added `# Safety` docs to unsafe functions, used `matches!` macro, replaced `b"..."` literals with `c"..."`, removed unnecessary unsafe blocks and casts, suppressed false positives with targeted `#[allow]` attributes.
- **Tautological boolean assertions** (`src/e2e_tests.rs`): Replaced `assert!(x || !x)` with `let _ = x;` in 2 E2E tests.

### Changed
- Version bumped to 0.20.0
- 726 tests passing (687 unit + 39 E2E)

## [0.19.9] - 2026-02-28

### Fixed
- **SIGSEGV on plugin scan with JUCE-based plugins** (`src/vst3/module.rs`): Fixed segmentation fault (exit 139) when scanning JUCE-based VST3 plugins (e.g., Monster.vst3). Root cause: `IPluginFactory3Vtbl` was missing the `getClassInfoUnicode` virtual method. The vtable placed `set_host_context` at slot 8 (where `getClassInfoUnicode` belongs) instead of slot 9, so calling `setHostContext` actually dispatched to the plugin's `getClassInfoUnicode` implementation with a garbage `FUnknown*` pointer as the index parameter.
  - Added `get_class_info_unicode` field to `IPluginFactory3Vtbl` (now correctly 10 function pointers)
  - Added `RawClassInfoW` struct matching the VST3 SDK's `PClassInfoW` (UTF-16 name/vendor/version fields)
  - Added `utf16_to_string` helper for converting null-terminated UTF-16 buffers
  - 7 new tests: vtable layout assertions for `IPluginFactoryVtbl` (7 ptrs), `IPluginFactory2Vtbl` (8 ptrs), `IPluginFactory3Vtbl` (10 ptrs), `RawClassInfoW` size (1208 bytes), UTF-16 string conversion (basic, empty, unicode)
  - 720 tests passing

## [0.19.8] - 2026-02-28

### Changed
- **Exclusive `--paths` flag**: When `--paths` is provided to `scan` or `gui`, ONLY the specified paths are used for plugin scanning — default system paths and persistent config paths are excluded. This allows targeted scanning of specific directories (e.g., `cargo run -- scan --paths ./vsts`).
  - Added `--paths` to the `gui` command — custom paths are propagated through the supervisor to the audio worker child process
  - Added `--paths` to the internal `audio-worker` command for process isolation support
  - All three scan callsites updated: CLI `scan`, in-process `HostApp::scan_plugins`, and supervised `audio_worker::handle_action`
  - Supports multiple paths: `--paths ./vsts /other/dir`
  - 9 new tests (4 CLI parsing + 3 HostApp + 2 AudioWorkerState), 712 tests passing

## [0.19.7] - 2026-02-28

### Added
- **Persistent scan paths** (`src/app/config.rs`): New `config` module providing persistent configuration storage. Custom VST3 scan directories are saved to `config.json` (alongside the plugin cache) and automatically included on every subsequent `scan` invocation.
  - New CLI command: `scan-paths add <DIR>` — permanently add a directory to the scan path list
  - New CLI command: `scan-paths remove <DIR>` — remove a directory from the persistent list
  - New CLI command: `scan-paths list` — display all persistent scan paths and the config file location
  - `scan` now loads persistent paths from config and merges them with defaults and any one-time `--paths` arguments
  - Config file: `~/Library/Application Support/rs-vst-host/config.json` (macOS), `~/.local/share/rs-vst-host/config.json` (Linux), `%APPDATA%\rs-vst-host\config.json` (Windows)
  - Duplicate path detection via filesystem canonicalization
  - 16 new tests (11 config + 5 CLI parsing), 703 tests passing

## [0.19.6] - 2026-02-28

### Fixed
- **Updated `test.bash` to run all tests**: Corrected stale test counts (579→687 lib, 564→671 ASan, 109→99 Miri Tree Borrows). Added `test_sandbox_sa_nodefer_flag_set` to ASan skip list (uses `sigaction` inspection incompatible with ASan). Added `cargo bench --no-run` step to verify all 11 benchmark files compile. Clippy now uses `-D warnings` for stricter lint checking.

## [0.19.5] - 2026-02-28

### Added
- **Headless GUI integration tests with screenshot capture** (`src/gui_tests.rs`): New test module exercising the full `HostApp` GUI rendering pipeline through `egui::Context::run()` without opening a native window. Tests verify:
  - Plugin browser, rack, and parameter panels render correctly in headless mode
  - Adding a plugin to the rack and selecting it shows the editor view (parameter panel)
  - The parameter panel becomes visible when a slot is selected and disappears on deselection
  - Multiple frame rendering is stable without panics or accumulated errors
  - All edge cases: empty rack, different parameter types (bypass, read-only, automatable), editor open without active plugin
  - CPU software-rasterized PNG screenshots saved to `target/test-screenshots/` (12 screenshots across 6 tests)
  - Custom uncompressed PNG encoder (no external image dependency) with barycentric triangle rasterization
  - Made panel renderer methods `pub(crate)` for headless test access: `show_browser`, `show_param_panel`, `show_rack`, `show_bottom_bar`
  - 6 new tests, 687 tests passing (581 lib + 106 binary).

## [0.19.4] - 2026-02-28

### Fixed
- **Plugin editor windows not opening in supervised mode** (`gui/editor.rs`, `gui/backend.rs`, `gui/audio_worker.rs`): In the default three-process architecture, clicking the 🎹 editor button did nothing. Root cause: `OpenEditor` was handled in the audio worker child process which has no `NSApplication` event loop — `NSWindow` was created in memory but never rendered on screen. Fix:
  - `EditorWindow::open()` now calls `ensure_ns_application()` to initialise the AppKit singleton before creating an `NSWindow`. Uses `NSApplicationActivationPolicyAccessory` (no dock icon) for the audio worker, but preserves `eframe`'s `NSApplicationActivationPolicyRegular` in in-process mode.
  - New `pump_events()` function drains all pending AppKit events (via `nextEventMatchingMask:untilDate:inMode:dequeue:` + `sendEvent:` + `updateWindows`) without blocking, allowing plugin editor UIs to render and respond in the audio worker's socket-based message loop.
  - `HostBackend::poll_editors()` now pumps the platform event loop before polling resize requests and pruning closed windows.
  - Audio worker main loop (`run_audio_loop`) now calls `state.backend.poll_editors()` on every iteration (~50ms cadence), ensuring editor windows remain responsive.
  - 3 new unit tests: `test_pump_platform_events_does_not_panic`, `test_ensure_ns_application_idempotent`, `test_pump_events_requires_main_thread`.
  - 681 tests passing (575 lib + 106 binary).

## [0.19.3] - 2026-02-28

### Changed
- **Stereo interleave/deinterleave fast path** (`vst3/process.rs`): Special-cased stereo (2-channel) interleave/deinterleave using `chunks_exact`/`chunks_exact_mut`, eliminating the inner channel loop and per-sample bounds checks. `write_input_interleaved_stereo` **9.6× faster** (2,228→231 ns at 1024 samples), `read_output_interleaved_stereo` **3.2× faster** (2,228→687 ns), `full_cycle_stereo` **4.2× faster** (4,541→1,072 ns).
- **Leaner `prepare()` in ProcessBuffers** (`vst3/process.rs`): Removed full `update_ptrs()` call from `prepare()` — only the two self-referential pointers (`process_data.inputs`/`outputs`) are refreshed each block instead of rebuilding all channel pointer arrays. Vec heap pointers are stable after construction.
- **Single-allocation `encode_message`** (`ipc/messages.rs`): Replaced two-allocation serialisation (temp Vec + final Vec) with `serde_json::to_writer` into a single buffer. `encode_processed_response` **1.76× faster** (48.8→27.7 ns), `encode_load_plugin` **1.53× faster** (214→140 ns), `encode_transport_state` **1.19× faster** (152→128 ns).
- **Pre-allocated `translate_midi_batch`** (`midi/translate.rs`): Replaced `.filter_map().collect()` with `Vec::with_capacity(messages.len())` + manual loop, avoiding dynamic reallocation. Batch notes **1.9× faster** at 16 events (171→90 ns), **1.43× faster** at 256 events (1,551→1,083 ns).
- **Direct array cast in event_list `query_interface`** (`vst3/event_list.rs`): Replaced `std::slice::from_raw_parts(iid, 16)` fat-pointer slice comparison with direct `*(iid as *const [u8; 16])` 16-byte array comparison, matching the pattern used in `param_changes.rs`.

## [0.19.2] - 2026-02-28

### Added
- **Divan performance benchmark suite**: 11 benchmark files with ~130+ individual benchmarks covering all hot paths in the audio processing pipeline. Benchmarks use the [Divan](https://github.com/nvzqz/divan) framework with `harness = false` for accurate measurement.
  - `benches/audio_engine.rs` — TestToneGenerator: sample generation, buffer fill at 44.1/96 kHz, sustained multi-block
  - `benches/process_buffers.rs` — ProcessBuffers: creation, prepare, interleave/deinterleave, full cycle for stereo/8ch
  - `benches/event_list.rs` — HostEventList: add/clear/cycle, COM vtable operations (get_event_count, get_event, add_event)
  - `benches/param_changes.rs` — HostParameterChanges: single/multi-param, worst-case linear scan, block cycle
  - `benches/midi_translate.rs` — MIDI translation: single events, batch (4–256), receiver push/drain
  - `benches/ipc_messages.rs` — IPC serialization: encode/decode all message types, param lists, roundtrip
  - `benches/process_context.rs` — ProcessContext: advance, transport, tempo, time signature, full block update
  - `benches/host_alloc.rs` — system_alloc vs Box (mimalloc): small/medium/large, batch alloc/free
  - `benches/diagnostics.rs` — heap_check, malloc env, allocator name, recommended vars
  - `benches/session_serde.rs` — Session: capture, restore, serde roundtrip (1–16 rack slots)
  - `benches/cache_serde.rs` — ScanCache: class/module/cache serde, roundtrip (4–64 modules)
- **Performance changelog**: New `docs/PERFORMANCE_CHANGELOG.md` tracking baseline benchmark results

## [0.19.1] - 2026-02-28

### Added
- **Phase 8 detailed plan** (`docs/PHASE_8.md`): Comprehensive plan for Phase 8 (Beyond MVP) with 9 sub-phases covering plugin state persistence, preset management, multi-plugin routing graph, undo/redo, drag-and-drop rack reordering, cross-platform editor windows, performance hardening, plugin compatibility improvements, and distribution/packaging. Includes milestones M6–M13, priority ordering, risk mitigations, and dependency list.

### Changed
- **Documentation reorganization**: All documentation files (except `README.md` and `CLAUDE.md`) moved from project root into `docs/` directory. Updated all cross-references in `README.md`, `CLAUDE.md`, and `docs/STATUS.md` to reflect new paths.

## [0.19.0] - 2026-02-28

### Added
- **Audio process separation** — The audio engine and plugin backend now run in a separate child process (`audio-worker`) from the supervisor, in addition to the GUI already running in its own child process. This provides full three-process crash isolation:
  - **Supervisor** (main process): Lightweight relay that spawns and monitors both child processes, relays IPC messages bidirectionally, caches shadow state for crash recovery
  - **Audio Worker** (child process): Runs `HostBackend`, `AudioEngine`, and all plugin instances. If a plugin corrupts the heap and crashes this process, the supervisor stays alive
  - **GUI Worker** (child process): Runs the eframe/egui window (unchanged from v0.18.0)
  - On audio worker crash: supervisor restarts it, restores cached rack configuration, and notifies the GUI. Active plugins need re-activation but the rack config is preserved
  - On GUI crash: supervisor restarts it and syncs full state from the audio worker (unchanged)
  - `AudioCommand` enum for supervisor→audio communication: `Action(GuiAction)`, `RequestFullState`, `RestoreState`, `Shutdown`
  - `SupervisorUpdate::AudioProcessRestarted` variant notifies the GUI when audio is restarted
  - `ShadowState` in supervisor caches rack config, plugin modules, transport, and settings for recovery
  - Hidden `audio-worker` CLI subcommand for internal use by the supervisor
  - New `gui/audio_worker.rs` module (audio worker entry point and message handling)
- 678 tests passing (572 lib + 106 binary), 0 ignored

### Changed
- `supervisor.rs` rewritten as a thin message relay (was previously running the HostBackend directly in-process)
- `handle_action()` moved from supervisor to audio worker process
- GUI worker now handles `AudioProcessRestarted` update: clears active plugin state and shows restart message

## [0.18.1] - 2026-02-27

### Fixed
- **GUI freeze after plugin deactivation crash** — When a plugin crashed during deactivation (e.g. SIGBUS in `terminate_controller`), the sandbox caught the signal via `siglongjmp` but left the process heap corrupted. Subsequent `free()` calls from Rust's allocator (dropping `AudioEngine` buffers) hit the corrupted malloc metadata, triggering `SIGABRT` ("Corruption of tiny freelist") and killing the supervisor process. The GUI worker became orphaned, spinning with "Broken pipe" errors. Fix: wrapped `drop(active)` in an outer `sandbox_call` so the entire `ActiveState` drop chain is crash-protected — if any signal fires during teardown (SIGBUS from the plugin or SIGABRT from corrupted malloc), `siglongjmp` returns to the outer sandbox and the entire `ActiveState` is leaked instead of aborted.
- **GUI worker orphaned after supervisor crash** — When the supervisor process died, the GUI worker kept running indefinitely, logging "Failed to send action to supervisor: Broken pipe" on every frame. Added `supervisor_disconnected` flag with detection on both send (broken pipe) and receive (EOF) paths. The GUI now shows a prominent red banner ("Supervisor process died — please close and restart") and stops trying to send actions.

## [0.18.0] - 2026-02-27

### Added
- **GUI process separation** — The GUI now runs in a separate child process by default, supervised by the main process. If a plugin crashes the GUI process, the supervisor automatically relaunches the GUI within seconds while audio continues uninterrupted. Key components:
  - `gui/supervisor.rs` — Supervisor loop: spawns GUI child, accepts Unix socket connections, sends full state, handles GUI actions, monitors for crashes, and relaunches (up to 5 rapid restarts in 30s)
  - `gui/gui_worker.rs` — GUI child process: connects to supervisor via Unix socket, renders the full eframe/egui interface, sends user actions back to the supervisor
  - `gui/ipc.rs` — IPC protocol: `GuiAction` (20 variants) and `SupervisorUpdate` (11 variants) enums with `DecodeError` for proper timeout/error discrimination
  - `gui-worker` hidden CLI subcommand for internal use by the supervisor
  - `--in-process` flag on `gui` command for legacy single-process mode
- `DecodeError` enum in GUI IPC with `is_timeout()` method for robust cross-platform error handling (macOS `EAGAIN` vs Linux `EWOULDBLOCK`)

### Fixed
- **GUI crash loop on startup** — The supervisor's message loop incorrectly treated macOS socket timeout errors ("Resource temporarily unavailable") as real errors, killing and restarting the GUI child 5 times before giving up. Fixed by introducing `DecodeError::Timeout` variant with proper cross-platform error string matching.

### Changed
- `ParamSnapshot` now derives `PartialEq`, `Serialize`, and `Deserialize` for IPC transport and change detection
- Default `gui` command now uses supervised mode (separate process); use `--in-process` for legacy behavior

## [0.17.5] - 2026-02-27

### Added
- **Multi-plugin lifecycle E2E tests** (`e2e_tests.rs`): 10 new end-to-end tests that load multiple different plugins and start/stop them in random orders:
  - `e2e_multi_plugin_load_process_shutdown_forward` — Both plugins processed, shutdown in load order
  - `e2e_multi_plugin_load_process_shutdown_reverse` — Both plugins processed, shutdown in reverse order
  - `e2e_multi_plugin_interleaved_setup` — Load MB first, start processing, then add Q4; shutdown Q4 while MB continues
  - `e2e_multi_plugin_stop_and_restart` — Stop Q4, reload with different sample rate (96 kHz), continue alongside MB
  - `e2e_multi_plugin_duplicate_plugin` — Two Pro-Q 4 instances at different sample rates/block sizes simultaneously
  - `e2e_multi_plugin_random_lifecycle_seed_42` — 4 instances (2×MB, 2×Q4) with deterministic pseudo-random load/start/process/shutdown ordering (seed 42)
  - `e2e_multi_plugin_random_lifecycle_seed_1337` — Same structure, different seed for broader permutation coverage
  - `e2e_multi_plugin_random_start_stop_cycles` — 5 cycles of random plugin selection, settings, and shutdown order
  - `e2e_multi_plugin_audio_engine_concurrent` — Both plugins run through AudioEngine simultaneously
  - `e2e_multi_plugin_rapid_add_remove` — Stress test: 10 rapid load→process→shutdown iterations with random plugin/settings
- Uses a built-in `SimpleRng` (linear congruential generator) for deterministic shuffles without external dependencies
- `PluginSlot` helper struct for managing named plugin instance slots in multi-plugin tests
- Total: 618 tests passing (608 → 618), 0 ignored

## [0.17.4] - 2026-02-27

### Fixed
- **Unsandboxed `HostApplication::destroy()` in `Vst3Module::drop` crash path** (`vst3/module.rs`): When instance cleanup crashed (IEditController teardown double-free), the module drop path called `HostApplication::destroy(self.host_context)` — a `libc::free()` on potentially corrupted heap — without sandbox protection. Now wrapped in `sandbox_call("module_destroy_host_context_post_crash", ...)` for safe crash recovery.

### Changed
- **Crash-resilience E2E tests** (`e2e_tests.rs`): Converted 6 previously-ignored tests (editor queries, parameter operations, component handler for both Pro-Q 4 and Pro-MB) into active crash-resilience tests using subprocess isolation:
  - Each crash-prone test runs in a child process spawned via `run_in_subprocess()`
  - Permanent SIGABRT handler (`install_subprocess_abort_handler()`) catches deferred heap corruption after sandbox signal handlers are restored
  - Entire test body (module load + API calls + Drop) wrapped in `sandbox_call()` for inner crash recovery
  - Retry mechanism with up to 5 subprocess attempts for non-deterministic crash timing
  - `subprocess_exit()` uses `libc::write()` + `libc::_exit(0)` for zero-allocation exit from corrupted heap state
  - Parent process checks for `E2E_PASS` marker in child stderr to verify test logic passed
- Total: 608 tests passing, **0 ignored** (was 6 ignored)

## [0.17.3] - 2026-02-27

### Added
- **End-to-end integration tests** (`src/e2e_tests.rs`): 29 tests exercising the full host pipeline against real FabFilter VST3 plugins (Pro-MB and Pro-Q 4). Coverage includes:
  - Plugin discovery and bundle binary resolution
  - Module loading and factory metadata verification for both plugins
  - Instance creation, f32 capability, bus arrangement negotiation
  - Full processing lifecycle (setup → activate → process → shutdown)
  - Multi-block sustained processing (100+ blocks with sine tone)
  - Silence-in / signal-through verification
  - Process context with tempo, time signature, and transport
  - Event list and parameter changes integration
  - Parameter enumeration, set/readback, display strings, normalized↔plain roundtrip (Pro-Q 4)
  - Component handler installation
  - Latency reporting
  - Various sample rates (44.1/48/96 kHz) and block sizes (32–4096)
  - Interleaved I/O roundtrip
  - AudioEngine integration (tone enabled and disabled)
  - Scan → cache → serde roundtrip pipeline
- Total: 608 tests passing (579 → 608), 0 ignored

## [0.16.1-dev] - 2026-02-27

### Analysis
- **Memory safety plan** (`MEMORY_SAFETY_PLAN.md`): Diagnosed and documented the root cause of "pointer being freed was not allocated" crash during plugin deactivation. Primary cause: heap domain mismatch between mimalloc (Rust global allocator) and system malloc (plugin C++ allocator). Host COM objects (HostApplication, HostComponentHandler, HostPlugFrame) allocated via mimalloc are invisible to system malloc zones; a plugin calling `free()` on a host pointer during COM teardown triggers macOS malloc zone validation → `abort()`. Secondary cause: audio thread race condition — potential for in-flight CoreAudio callback during stream drop. Plan includes 5 fixes across 3 phases.

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
- **GUI Skeleton** (`gui/` module): Basic graphical user interface using `egui` 0.31 and `eframe` 0.31, implementing the first step of the design.
- **Theme** (`gui/theme.rs`): Full dark glassmorphism theme — transparent background, translucent panel fills, electric blue accent colour, CornerRadius (12/8/6 px), soft panel shadows, glass border strokes, custom text styles, and helper frame constructors (`glass_card_frame`, `section_frame`).
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
