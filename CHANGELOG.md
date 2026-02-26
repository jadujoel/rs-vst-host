# Changelog

All notable changes to this project will be documented in this file.

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
