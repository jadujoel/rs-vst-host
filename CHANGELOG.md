# Changelog

All notable changes to this project will be documented in this file.

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
