# Status

## Current Phase: Phase 8 — Process-Per-Plugin Sandboxing

**Milestone M2 achieved**: Single plugin instantiates and initializes.
**Milestone M3 achieved**: Real-time audio callback calls plugin process reliably.
**Milestone M4 achieved**: MIDI note input triggers instrument output.
**Milestone M5 achieved**: Parameter control + stable CLI UX.
**Quality gate achieved**: 347 tests passing, zero warnings, comprehensive coverage of non-RT components.
**Bug fix release**: IAudioProcessor IID corrected, CFBundleRef support added, IPluginFactory3 support added.
**Compatibility fix**: Separate IEditController support — split component/controller plugins (e.g. FabFilter) now expose parameters.
**GUI Design**: Created `DESIGN_DOCUMENT.md` outlining the Liquid Glass style architecture using `egui` + `wgpu`.
**GUI Skeleton**: Basic `egui`/`eframe` GUI window with Liquid Glass theme, plugin browser, plugin rack, and transport controls. New `gui` CLI command.
**GUI PRD**: Added `PRD.md` with product requirements for the GUI application.
**GUI Integration**: Full backend bridge connecting GUI to audio engine — plugin activation/deactivation from rack, parameter view panel with sliders, audio/MIDI device selection, session save/load, test tone toggle.
**GUI Editor Windows**: IPlugView/IPlugFrame COM interfaces, native macOS NSWindow hosting for plugin editors, editor lifecycle management, transport sync, audio status display, parameter search filter, safe mode, keyboard shortcuts.
**Rust 2024 compliance**: Fixed `unsafe_op_in_unsafe_fn` warnings in `plug_frame.rs` and `editor.rs` by wrapping unsafe operations in explicit `unsafe {}` blocks.
**UI readability**: Adjusted glass panel alpha handling for higher text contrast on light cards and controls.
**Validation**: `cargo test` (347 tests) passes after the theme update.
**Interaction plan**: Added a GUI interaction plan for plugin parameter editing workflows.
**Parameter editing for selected slots**: Implemented the interaction plan — clicking a rack slot shows its parameters (cached or live); inactive plugins support staged changes applied on activation. 362 tests passing.
**Bug fix (v0.12.1)**: Fixed SIGSEGV (exit 139) on plugin activation — `Vst3Module` was dropped prematurely, unloading the dynamic library while COM vtable pointers were still in use. Module now kept alive in `ActiveState`. 364 tests passing.
**Bug fix (v0.12.2)**: Fixed SIGSEGV (exit 139) on plugin deactivation (stop button) — race condition between GUI thread calling `shutdown()` and the audio callback calling `process()` on a deactivated VST3 plugin. Added `is_shutdown` guard flag to `AudioEngine`, custom `Drop` for `ActiveState` ensuring correct resource teardown order (params → stream → engine → MIDI → module), and wrapped `_stream` in `Option` for explicit early drop in `deactivate_plugin`. 368 tests passing.
**Plugin sandboxing (v0.13.0)**: Crash-safe plugin isolation using `sigsetjmp`/`siglongjmp` signal handlers. All plugin COM calls (process, shutdown, drop, module exit) are sandboxed — if a plugin crashes with SIGBUS, SIGSEGV, SIGABRT, or SIGFPE, the host recovers gracefully instead of terminating. Crashed plugins are auto-deactivated by the GUI with a status message. 389 tests passing.
**Bug fix (v0.13.1)**: Fixed SIGABRT (exit 134) after sandbox-recovered plugin crash — C++ static destructors in the plugin library ran on corrupted state during library unload. Instance crash status is now communicated to `Vst3Module` via thread-local flag; module skips library unload (`bundleExit` + `CFRelease`) when instance cleanup crashed. `cf_bundle::release` also wrapped in sandbox as defense-in-depth. 407 tests passing.
**Bug fix (v0.13.2)**: Fixed SIGABRT (exit 134) on plugin restart after a crash-recovered deactivation — `siglongjmp` recovery from SIGBUS during COM cleanup left the process heap corrupted; re-loading the same library triggered malloc freelist corruption. Added tainted-path tracking to prevent re-activation of crashed plugins, and split COM cleanup into 5 granular sandbox calls for better crash isolation. 415 tests passing.
**Debug infrastructure (v0.14.0)**: Comprehensive diagnostic tooling for characterising heap corruption — backtrace capture in signal handler, `malloc_zone_check` heap integrity checks, `--malloc-debug` CLI flag, dhat heap profiler (feature-gated), Chrome trace export (feature-gated), performance spans on hot paths, red heap-corruption warning banner in GUI. New `diagnostics.rs` module. 437 tests passing (438 with `--features debug-tools`).
**Debug profiling run (v0.14.0)**: Full diagnostic session executed with `--features debug-tools`, `--malloc-debug`, and `MallocGuardEdges=1 MallocScribble=1 MallocErrorAbort=1`. Results: dhat heap profile written (`dhat-heap.json`), Chrome trace timeline captured (`trace-1772121403013111.json`, 4.8 MB, ~78 seconds of execution). Trace shows 1 SIGABRT crash during `release_controller` sandbox call (FabFilter Pro-Q 4), heap integrity check passed (`heap_corrupted: false`), plugin correctly tainted and subsequent re-activation attempts blocked with user-friendly error. Host remained stable throughout — no secondary crashes. 438 tests passing.
**Bug fix (v0.14.1)**: Fixed malloc heap corruption ("Corruption of tiny freelist") that caused SIGABRT during host termination after a sandbox-recovered plugin crash. Root cause: leaked plugin COM objects (after `siglongjmp` crash recovery) still held pointers to host objects (`HostApplication`, `HostComponentHandler`) that were unconditionally freed in `Vst3Instance::drop` — use-after-free → heap corruption. Fix: (1) call `setComponentHandler(nullptr)` before terminate per VST3 protocol, (2) split terminate/release into separate sandbox calls for better crash isolation, (3) conditionally leak host objects when any crash occurred (< 1 KB, library also stays loaded). 441 tests passing.
**Bug fix (v0.14.2)**: Fixed malloc heap corruption ("Corruption of tiny freelist 0x…: size too small (0/48)") that caused SIGABRT when stopping a plugin (e.g. FabFilter Pro-Q 4). Root cause: `HostPlugFrame::release()` self-destructed (via `Box::from_raw`) when the COM reference count hit zero, but the host also called `HostPlugFrame::destroy()` afterward — double-free of a ~48-byte allocation corrupted the macOS malloc tiny freelist. The crash was masked by `debug.bash` because `dhat::Alloc` (global allocator replacement) and `MallocGuardEdges`/`MallocScribble` changed allocation layout. Fix: removed self-destruct from `host_plug_frame_release()` to match the pattern used by `HostComponentHandler` and `HostApplication`. 443 tests passing.
**Bug fix (v0.19.9)**: Fixed SIGSEGV (exit 139) when scanning JUCE-based VST3 plugins (e.g., Monster.vst3). Root cause: `IPluginFactory3Vtbl` was missing the `getClassInfoUnicode` method — the vtable had `set_host_context` at slot 8 where `getClassInfoUnicode` should be (slot 9). Calling `setHostContext` actually invoked `getClassInfoUnicode` with garbage arguments → segfault. Fix: added `get_class_info_unicode` field and `RawClassInfoW` (PClassInfoW) struct to the vtable definition. Added `utf16_to_string` helper and 7 new tests (vtable layout assertions for Factory/Factory2/Factory3, RawClassInfoW size, UTF-16 conversion). 720 tests passing.
**Heap isolation (v0.15.0)**: Replaced the system allocator with mimalloc as the default global allocator. Since VST3 plugins are loaded C++ code that uses system malloc directly, a buggy plugin corrupting the system malloc heap would previously also corrupt our Rust allocations. With mimalloc, all Rust heap allocations live in a separate heap, isolated from plugin-induced system malloc corruption. `debug-alloc` feature still overrides with dhat for profiling. 445 tests passing.
**Process-per-plugin sandboxing (v0.16.0)**: Each plugin can run in its own child process with POSIX shared memory for zero-copy audio buffers and Unix domain sockets for control IPC. This is the gold standard approach used by DAWs like Bitwig Studio — a crashed plugin only kills its child process, the host continues completely unaffected with no heap corruption risk. New `ipc/` module with `messages.rs` (wire protocol), `shm.rs` (shared memory), `worker.rs` (child process), `proxy.rs` (host proxy). Backend supports both in-process and sandboxed modes via `process_isolation` flag. Hidden `worker` CLI subcommand for internal use. 498 tests passing.
**Memory safety analysis (v0.16.1-dev)**: Diagnosed malloc crash ("pointer being freed was not allocated") during plugin teardown. Root cause: heap domain mismatch — host COM objects allocated via mimalloc are invisible to system malloc; plugins calling `free()` on host pointers during C++ destructors triggers macOS malloc zone validation errors. Created `MEMORY_SAFETY_PLAN.md` with 5-fix plan: (1) system malloc for plugin-facing COM objects, (2) atomic audio shutdown flag, (3) sandboxed host object destruction, (4) per-plugin allocation zones, (5) defensive delay before library unload.
**E2E integration tests (v0.17.3)**: 29 end-to-end tests using real FabFilter VST3 plugins (Pro-MB and Pro-Q 4) in the `vsts/` directory. Tests cover: plugin discovery, bundle resolution, module loading, factory metadata, instance creation, f32 capability, bus arrangements, full processing lifecycle, multi-block sustained processing (100+ blocks), silence-in/signal-through verification, process context with transport, event list and parameter changes, parameter enumeration/set/readback/display/normalized↔plain conversion, component handler installation, latency reporting, various sample rates (44.1/48/96 kHz) and block sizes (32–4096), interleaved I/O roundtrip, AudioEngine integration (with and without test tone), and scan→cache serde pipeline.
**Crash-resilience E2E tests (v0.17.4)**: Converted 6 previously-ignored tests into active crash-resilience tests using subprocess isolation. Tests verify the host survives FabFilter's IEditController/IPlugView COM teardown double-free bug — each crash-prone test runs in a child process with a permanent SIGABRT handler, sandbox-wrapped test body, and retry mechanism (up to 5 attempts). Fixed unsandboxed `HostApplication::destroy()` in `Vst3Module::drop` crash path.
**Multi-plugin lifecycle E2E tests (v0.17.5)**: 10 new end-to-end tests exercising multiple plugin instances loaded simultaneously with random start/stop ordering. Tests cover: forward and reverse shutdown order, interleaved setup (load one plugin while another is processing), stop-and-restart with different settings, duplicate plugin instances, deterministic pseudo-random lifecycle ordering (seeds 42 and 1337 for reproducibility), random start/stop cycles across 5 iterations, concurrent AudioEngine integration, and rapid add/remove stress testing (10 iterations). 618 tests passing, 0 ignored.
**GUI process separation (v0.18.0)**: GUI now runs in a separate child process supervised by the main process. Architecture: supervisor (main process) manages audio engine, plugin backend, and state; spawns GUI as `gui-worker` child process communicating via Unix domain socket with length-prefixed JSON framing. If the GUI crashes (e.g., from plugin editor heap corruption), the supervisor automatically relaunches it while audio continues uninterrupted. New modules: `gui/supervisor.rs` (supervisor loop with crash detection, max 5 rapid restarts in 30s), `gui/gui_worker.rs` (eframe App implementation mirroring HostApp), `gui/ipc.rs` (GuiAction/SupervisorUpdate protocol with DecodeError for cross-platform timeout handling). Fixed startup crash loop caused by macOS socket timeout error misclassification. 546 tests passing.
**Bug fix (v0.18.1)**: Fixed GUI freeze/abort when stopping a plugin that crashes during deactivation. Root cause: plugin SIGBUS during `terminate_controller` was caught by the inner sandbox, but the resulting heap corruption caused `SIGABRT` during subsequent Rust `free()` calls, killing the supervisor process. Fix: wrapped entire `ActiveState` drop in an outer `sandbox_call` — any signal during the drop chain now leaks the state instead of aborting. Also fixed the GUI worker becoming orphaned after supervisor death — added `supervisor_disconnected` detection (broken pipe + EOF) with a red disconnect banner and action suppression. 663 tests passing.
**Performance optimizations (v0.19.3)**: Benchmark-driven optimisations — stereo interleave/deinterleave 3–9× faster via `chunks_exact` fast path, `encode_message` single-allocation, `translate_midi_batch` pre-allocation, leaner `prepare()`. Full cycle stereo 1024 samples: 4.5µs → 1.1µs.
**Divan benchmarks (v0.19.2)**: 11 benchmark files with ~130+ individual benchmarks covering all hot paths (audio engine, process buffers, event list, param changes, MIDI translation, IPC messages, process context, host_alloc, diagnostics, session serde, cache serde). Initial baseline captured in `docs/PERFORMANCE_CHANGELOG.md`.
**Audio process separation (v0.19.0)**: Audio engine and plugin backend now run in a separate child process from the supervisor. The supervisor is a lightweight relay spawning and monitoring both GUI and audio worker child processes. If a plugin crashes the audio process, the supervisor stays alive, restarts the audio worker, restores the cached rack configuration via `RestoreState`, and notifies the GUI with `AudioProcessRestarted`. Three-process architecture: Supervisor (relay) → Audio Worker (HostBackend + plugins) + GUI Worker (eframe/egui). New modules: `gui/audio_worker.rs` (audio worker entry point with `handle_action` logic), rewritten `gui/supervisor.rs` (message relay with `ShadowState` for crash recovery), new `AudioCommand` IPC enum, hidden `audio-worker` CLI subcommand. 678 tests passing (572 lib + 106 binary).
**Bug fix (v0.19.4)**: Fixed plugin editor windows not opening in supervised mode (default). Root cause: `OpenEditor` was handled in the audio worker child process which had no `NSApplication` event loop — `NSWindow` was created in memory but never rendered. Fix: (1) `EditorWindow::open()` now calls `ensure_ns_application()` to initialize the AppKit singleton with `NSApplicationActivationPolicyAccessory`, (2) added `pump_events()` to drain pending AppKit events without blocking, (3) `HostBackend::poll_editors()` now pumps the platform event loop before polling resize/prune, (4) audio worker main loop now calls `poll_editors()` on every iteration. Also preserves `eframe`'s "regular" activation policy when running in-process. 681 tests passing (575 lib + 106 binary).
**Headless GUI tests (v0.19.5)**: New `gui_tests` module with 6 tests exercising the full `HostApp` GUI rendering pipeline headlessly via `egui::Context::run()`. Tests verify the editor view (parameter panel) becomes visible when a plugin is added and selected, render multiple frames for stability, and save CPU software-rasterized PNG screenshots to `target/test-screenshots/`. Custom PNG encoder with barycentric triangle rasterization (no external image dependency). 687 tests passing (581 lib + 106 binary).
**Test script fix (v0.19.6)**: Updated `test.bash` to run all 687 tests correctly. Fixed stale counts, added missing ASan skip (`test_sandbox_sa_nodefer_flag_set`), added benchmark compilation check, stricter Clippy (`-D warnings`). Full suite: 687 lib tests + Clippy + bench compile + 99 Miri Tree Borrows + 70 Miri Stacked Borrows + 671 ASan tests.
**Persistent scan paths (v0.19.7)**: New `config` module for persistent application configuration. Added `scan-paths` CLI subcommand with `add`, `remove`, and `list` actions, allowing users to permanently register custom VST3 plugin directories. Persistent paths are automatically included in every `scan` invocation. Config stored as JSON alongside the plugin cache. 16 new tests (11 config + 5 CLI), 703+ tests passing.
**Exclusive --paths flag (v0.19.8)**: When `--paths` is provided to `scan` or `gui`, only the specified paths are used for plugin scanning — default system paths and persistent config paths are excluded. `--paths` added to `gui` and internal `audio-worker` commands with full propagation through supervisor → audio worker child process. Supports `--paths dir1 dir2` multi-value syntax. 9 new tests, 712 tests passing.

### Completed

#### Phase 0 — Technical Decisions
- **CLI**: `clap` v4 with derive macros
- **Error handling**: `thiserror` v2 + `anyhow` v1
- **Logging**: `tracing` + `tracing-subscriber` with env-filter
- **Serialization**: `serde` + `serde_json` for plugin cache
- **Dynamic loading**: `libloading` v0.8 (manual COM FFI for VST3 factory access)
- **Platform dirs**: `dirs` v6
- **Audio I/O**: `cpal` v0.15 for cross-platform audio
- **MIDI input**: `midir` v0.10 for cross-platform MIDI
- **Signal handling**: `ctrlc` v3 for graceful shutdown
- VST3 interop approach: Manual COM vtable FFI for scanning, component instantiation, audio processing, events, and parameters

#### Phase 1 — Project Foundations
- Module structure: `app/`, `audio/`, `midi/`, `host/`, `vst3/`, `error.rs`
- Error types: `HostError`, `Vst3Error`, `AudioError`, `MidiError` with `thiserror`
- Logging: `tracing` with env-filter subscriber
- CLI: `scan`, `list`, `run`, `devices`, `midi-ports` subcommands via `clap`

#### Phase 2 — VST3 Plugin Discovery and Loading
- **Scanner** (`vst3/scanner.rs`): Searches macOS/Linux/Windows standard VST3 paths, discovers `.vst3` bundles recursively, resolves platform-specific binary paths
- **Module loader** (`vst3/module.rs`): Dynamic loading via `libloading`, manual COM FFI for IPluginFactory, IPluginFactory2, and IPluginFactory3, platform-specific `bundleEntry`/`ModuleEntry` handling with proper CFBundleRef
- **Cache** (`vst3/cache.rs`): JSON-based plugin cache in platform data directory
- **CLI commands** (`app/commands.rs`): `scan` discovers+loads+caches, `list` displays cached plugins

#### Phase 3 — Audio Engine Integration
- **COM interface definitions** (`vst3/com.rs`): Manual FFI vtable definitions for IComponent, IAudioProcessor, with ProcessSetup, ProcessData, AudioBusBuffers structs; verified layout with struct size tests; IID correctness verified against UUIDs from VST3 SDK
- **Host context** (`vst3/host_context.rs`): Minimal IHostApplication COM object implementation with `getName` and stub `createInstance`; reference counted; passed to plugin `initialize()`
- **CFBundleRef support** (`vst3/cf_bundle.rs`): CoreFoundation FFI for creating proper CFBundleRef from `.vst3` bundle path, passed to `bundleEntry` on macOS
- **IPluginFactory3 support** (`vst3/module.rs`): Queries for IPluginFactory3 and calls `setHostContext` for modern plugin compatibility
- **Instance management** (`vst3/instance.rs`): Full VST3 component lifecycle — factory `createInstance`, `initialize` with host context, `QueryInterface` for IAudioProcessor, bus arrangement negotiation, `setupProcessing`, `setActive`/`setProcessing`, and clean shutdown
- **Process buffers** (`vst3/process.rs`): Pre-allocated buffer management for VST3 `process()` — per-channel sample buffers, interleaved↔deinterleaved conversion, stable pointer management for real-time safety
- **Audio device** (`audio/device.rs`): `cpal`-based audio device enumeration, output stream configuration, and stream building
- **Processing engine** (`audio/engine.rs`): Bridges cpal audio callback with VST3 plugin processing; includes 440 Hz sine wave test tone generator for effect plugin testing
- **`run` command** (`app/commands.rs`): Full implementation — resolves plugin by name or path, instantiates component, configures audio device, sets up processing (sample rate, block size, bus arrangements), runs real-time audio loop, handles Ctrl+C for clean shutdown
- **`devices` command**: Lists available audio output devices with default indicator

#### Phase 4 — MIDI, Parameters, and Basic Automation
- **MIDI device** (`midi/device.rs`): `midir`-based MIDI port enumeration, connection opening, lock-free message receiver (`MidiReceiver`) with push/drain for inter-thread MIDI transfer
- **MIDI translation** (`midi/translate.rs`): Raw MIDI to VST3 event conversion — Note On, Note Off (including vel=0 convention), channel extraction; batch translation for per-block processing
- **VST3 event structures** (`vst3/com.rs`): Event, NoteOnEvent, NoteOffEvent `#[repr(C)]` structs matching SDK layout; IEventList vtable
- **Host event list** (`vst3/event_list.rs`): IEventList COM object implementation — add/get/clear events, QueryInterface for IEventList and FUnknown IIDs; static vtable with proper COM calling convention
- **Engine MIDI integration** (`audio/engine.rs`): Audio engine drains MIDI receiver each block, translates to VST3 events, passes via HostEventList to ProcessData.input_events
- **IEditController query** (`vst3/instance.rs`): `query_parameters()` — QueryInterface for IEditController from component; for split-architecture plugins, creates a separate controller via factory `createInstance`, initializes it, and connects component↔controller via IConnectionPoint
- **Parameter registry** (`vst3/params.rs`): Enumerates all plugin parameters via IEditController, stores metadata (title, units, default, current, flags), converts normalized/plain values, formats display strings
- **IEditController vtable** (`vst3/com.rs`): Full vtable definition with getParameterCount, getParameterInfo, setParamNormalized, getParamStringByValue, normalizedParamToPlain, etc.
- **ParameterInfo struct** (`vst3/com.rs`): Matches SDK layout with id, title, short_title, units, step_count, default_normalized_value, flags
- **`midi-ports` command**: Lists available MIDI input ports
- **CLI options**: `--midi`, `--list-params` flags on `run` command; `-B` for buffer-size

#### Phase 5 — Host UX (MVP CLI)
- **IComponentHandler** (`vst3/component_handler.rs`): COM implementation for plugin-to-host parameter change notifications; beginEdit/performEdit/endEdit callbacks; restartComponent with flag handling; thread-safe change queue with drain support; reference-counted with static vtable
- **ProcessContext** (`vst3/process_context.rs`): Transport timing struct matching VST3 SDK layout; tempo, time signature, sample position, musical position (quarters), bar position; automatic transport advancement per audio block; playing state control
- **IParameterChanges + IParamValueQueue** (`vst3/param_changes.rs`): Host-side COM implementations for sample-accurate parameter automation; pre-allocated queue pool (64 params × 16 points); full COM vtable with getParameterCount/getParameterData/addParameterData and getParameterId/getPointCount/getPoint/addPoint
- **Interactive command shell** (`app/interactive.rs`): Runtime parameter control during audio processing — `params`, `get`, `set`, `tempo`, `status`, `help`, `quit` commands; parameter lookup by ID or name; plugin-initiated change display
- **Engine integration**: AudioEngine manages ProcessContext (transport, tempo, bar position), HostParameterChanges (control-to-audio param queue), and routes changes each audio block
- **Instance integration**: Vst3Instance creates and manages IComponentHandler lifecycle (install on IEditController via setComponentHandler, destroy on drop)
- **Command integration**: `run` command installs component handler, queries params for interactive access, captures param queue, runs interactive loop

#### Phase 6 — Validation and Quality Gates
- **Comprehensive test suite**: 117 new tests added across 13 modules (106 → 223 total)
- **Error type tests** (`error.rs`): Display formatting for all 4 error enums (HostError, Vst3Error, AudioError, MidiError), From conversions, Debug formatting
- **CLI parsing tests** (`app/cli.rs`): All subcommands parsed, required/optional args, invalid input rejection, short flags
- **Types serde tests** (`vst3/types.rs`): Roundtrip serialization for PluginClassInfo/PluginModuleInfo, optional fields, CID encoding, Clone/Debug
- **Cache I/O tests** (`vst3/cache.rs`): Serde roundtrip, file I/O roundtrip with temp dir, corrupt JSON error handling, timestamp format validation
- **Scanner tests** (`vst3/scanner.rs`): Dedup, sorted output, recursive directory scanning, non-vst3 filtering, macOS bundle structure resolution
- **Parameter registry tests** (`vst3/params.rs`): UTF-16 conversion edge cases, string truncation, flag combinations, ParameterEntry Debug
- **Event list tests** (`vst3/event_list.rs`): COM vtable overflow (MAX_EVENTS_PER_BLOCK), add/get via vtable, null pointer safety, QI
- **Parameter changes tests** (`vst3/param_changes.rs`): Queue overflow (MAX_PARAM_QUEUES/MAX_POINTS_PER_PARAM), PVQ QI, null safety, existing parameter reuse
- **Process buffer tests** (`vst3/process.rs`): Setter methods, zero-channel configurations, out-of-range access, consecutive prepare calls, mono-in/stereo-out
- **MIDI translation tests** (`midi/translate.rs`): All 16 channels, extreme pitches, note-off velocity, batch edge cases, truncated/unsupported messages
- **Interactive tests** (`app/interactive.rs`): All commands with no-params paths, tempo parsing, handler polling, invalid value handling
- **Host context tests** (`vst3/host_context.rs`): QI for all IIDs, ref counting accuracy, null safety, destroy null
- **Component handler tests** (`vst3/component_handler.rs`): Concurrent perform_edit (4 threads), restart flag OR behavior, null safety
- **Test stability**: All 223 tests pass consistently across 5 consecutive runs
- **Clean build**: Zero warnings

#### Phase 7 — GUI Implementation (Step 1: Skeleton)
- **GUI module** (`gui/mod.rs`): New top-level module with `app`, `theme`, `editor`, `backend`, `session` submodules
- **Liquid Glass theme** (`gui/theme.rs`): Full egui 0.31 theme — color palette (BG_BASE, PANEL_FILL, ACCENT, etc.), CornerRadius constants (card 12px, button 8px, small 6px), Shadow, Margin constants, widget/selection/window visuals, text styles, glass_card_frame() and section_frame() helpers
- **HostApp** (`gui/app.rs`): `eframe::App` implementation with three-panel layout:
  - Left sidebar: Plugin browser with scan button, search filter, scrollable list of glass-card plugin entries with add-to-rack buttons
  - Central panel: Plugin rack showing loaded plugin slots as glass cards with bypass toggle, remove button, and selection highlight
  - Bottom bar: Transport controls (play/pause, BPM drag value, time signature), status message display
- **Data structures**: `PluginSlot`, `TransportState`, `BrowserFilter`, `HostApp` state management with add/remove/filter operations
- **CLI integration**: `gui` subcommand added to `clap` CLI, launches the eframe window from `main.rs`
- **Dependencies**: Added `eframe` 0.31 and `egui` 0.31 to `Cargo.toml`

#### Phase 7 — GUI Implementation (Step 3: Editor Windows & PRD Features)
- **IPlugView/IPlugFrame COM** (`vst3/com.rs`): Added IIDs, vtable structs, ViewRect, platform type constants for editor view support
- **Host IPlugFrame** (`vst3/plug_frame.rs`): COM implementation for plugin resize requests with atomic ref counting and thread-safe pending resize
- **Editor window management** (`gui/editor.rs`): Native macOS NSWindow creation via ObjC runtime FFI, IPlugView attach/detach lifecycle, resize propagation
- **Editor on Vst3Instance** (`vst3/instance.rs`): `create_editor_view()` and `has_editor()` methods
- **Backend editor integration** (`gui/backend.rs`): Editor window tracking, open/close/poll methods, AudioStatus struct, transport sync methods (set_tempo, set_playing, set_time_signature)
- **Transport sync** (`gui/app.rs`): GUI transport state changes pushed to audio engine in real time
- **Audio engine status**: Sample rate, buffer size, device name displayed in transport bar
- **Parameter search**: Text filter in parameter panel for quick param lookup
- **Safe mode**: `--safe-mode` CLI flag disables plugin editor windows
- **Keyboard shortcuts**: Space bar toggles play/pause
- **Improved scan progress**: Shows module count, class count, and error count

### Test Results
- 602 tests passing (579 unit + 23 E2E integration; 6 ignored due to plugin COM teardown crashes)
- E2E tests exercise real FabFilter VST3 plugins (Pro-MB, Pro-Q 4): discovery, loading, metadata, processing, parameters, AudioEngine, scan-cache pipeline
- Clean build with zero warnings
- Test stability verified across multiple consecutive runs
- Successfully loads and runs real VST3 plugins on macOS (tested with FabFilter Pro-MB, Pro-Q 4)
- Parameter enumeration works for both single-component and split component/controller plugins
- MIDI port enumeration working (midir, CoreMIDI)
- Audio device enumeration working (cpal)

### Documentation
- `USER_GUIDE.md` — end-user guide covering installation, all CLI commands, plugin search paths, cache details, logging, and troubleshooting
- `README.md` — project overview, architecture, dependencies, and roadmap
- `docs/` — all documentation moved to `docs/` directory (except README.md and CLAUDE.md)
- `docs/CHANGELOG.md` — version history
- `docs/CODE_COVERAGE.md` — test coverage analysis by module
- `docs/PHASE_8.md` — detailed Phase 8 plan (9 sub-phases)

### Next Steps (Phase 8 — Beyond MVP)

See [PHASE_8.md](PHASE_8.md) for the full detailed plan. Summary:
- **8.1** Plugin state persistence (IBStream, component state save/load)
- **8.2** Preset/program management (IUnitInfo, preset files, browser panel)
- **8.3** Multi-plugin routing graph (serial/parallel chains, visual editor)
- **8.4** Undo/redo system (command pattern, parameter coalescing)
- **8.5** Drag-and-drop rack reordering
- **8.6** Cross-platform plugin editor windows (Linux X11/Wayland, Windows Win32)
- **8.7** Performance hardening (lock-free queues, xrun tracking, CPU monitoring)
- **8.8** Plugin compatibility improvements (bus fallbacks, latency compensation)
- **8.9** Distribution and packaging (app bundles, CI/CD, installers)
