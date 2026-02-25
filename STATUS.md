# Status

## Current Phase: Phase 5 — Host UX (MVP CLI) (Complete)

**Milestone M2 achieved**: Single plugin instantiates and initializes.
**Milestone M3 achieved**: Real-time audio callback calls plugin process reliably.
**Milestone M4 achieved**: MIDI note input triggers instrument output.
**Milestone M5 achieved**: Parameter control + stable CLI UX.

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
- **Module loader** (`vst3/module.rs`): Dynamic loading via `libloading`, manual COM FFI for IPluginFactory and IPluginFactory2, platform-specific `bundleEntry`/`ModuleEntry` handling
- **Cache** (`vst3/cache.rs`): JSON-based plugin cache in platform data directory
- **CLI commands** (`app/commands.rs`): `scan` discovers+loads+caches, `list` displays cached plugins

#### Phase 3 — Audio Engine Integration
- **COM interface definitions** (`vst3/com.rs`): Manual FFI vtable definitions for IComponent, IAudioProcessor, with ProcessSetup, ProcessData, AudioBusBuffers structs; verified layout with struct size tests
- **Host context** (`vst3/host_context.rs`): Minimal IHostApplication COM object implementation with `getName` and stub `createInstance`; reference counted; passed to plugin `initialize()`
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
- **IEditController query** (`vst3/instance.rs`): `query_parameters()` — QueryInterface for IEditController from component, with fallback to separate controller class ID detection
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

### Test Results
- 106 unit tests passing (scanner, cache, module, COM structs, host context, process buffers, tone generator, audio device, MIDI receiver, MIDI translation, event list, parameters, component handler, process context, parameter changes, interactive state)
- Clean build with zero warnings
- Successfully scans real VST3 plugins on macOS (tested with FabFilter Pro-MB, Pro-Q 4)
- MIDI port enumeration working (midir, CoreMIDI)
- Audio device enumeration working (cpal)

### Documentation
- `USER_GUIDE.md` — end-user guide covering installation, all CLI commands, plugin search paths, cache details, logging, and troubleshooting
- `README.md` — project overview, architecture, dependencies, and roadmap
- `CHANGELOG.md` — version history

### Next Steps (Phase 6 — Validation and Quality Gates)
- Automated tests for non-RT components
- Manual test matrix (multiple sample rates/block sizes, synth + effect plugins)
- Performance checks (xrun/dropout measurement, CPU usage baseline)
- Cross-platform validation (Linux, Windows)
