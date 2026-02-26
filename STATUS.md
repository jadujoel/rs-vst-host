# Status

## Current Phase: Phase 7 — GUI Design and Implementation (Step 3: Editor Windows & PRD Features)

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
- 347 unit tests passing (error Display/From, CLI parsing incl. safe-mode, types serde, scanner edge cases, cache I/O, parameter utilities, event list COM, parameter changes COM, process buffers, MIDI translation, interactive commands, host context COM, component handler concurrency, process context, COM struct layouts, IID UUID verification (incl. IPlugView/IPlugFrame), tone generator, audio device, MIDI receiver, CFBundleRef creation, IConnectionPoint IID, factory vtable layout, GUI theme colours/constants/apply, GUI app state/rack/filter/browser/backend integration/session roundtrip/device selection/parameter refresh/safe mode/transport sync/editor/param filter/audio status, plug_frame COM ref counting/QI/resize, editor module)
- Clean build with zero warnings
- Test stability verified across multiple consecutive runs
- Successfully loads and runs real VST3 plugins on macOS (tested with FabFilter Pro-MB, Pro-Q 4)
- Parameter enumeration works for both single-component and split component/controller plugins
- MIDI port enumeration working (midir, CoreMIDI)
- Audio device enumeration working (cpal)

### Documentation
- `USER_GUIDE.md` — end-user guide covering installation, all CLI commands, plugin search paths, cache details, logging, and troubleshooting
- `README.md` — project overview, architecture, dependencies, and roadmap
- `CHANGELOG.md` — version history
- `CODE_COVERAGE.md` — test coverage analysis by module

### Next Steps (Phase 8 — Beyond MVP)
- Full integration testing of plugin editor windows with real VST3 plugins
- Preset/program management
- Multiple simultaneous plugin instances and simple routing graph
- Undo/redo for parameter changes
- Plugin state persistence (component state save/load)
- Drag-and-drop plugin reordering in rack
- Linux/Windows editor window implementations
