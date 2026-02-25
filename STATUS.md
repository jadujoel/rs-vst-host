# Status

## Current Phase: Phase 3 — Audio Engine Integration (Complete)

**Milestone M2 achieved**: Single plugin instantiates and initializes.
**Milestone M3 achieved**: Real-time audio callback calls plugin process reliably.

### Completed

#### Phase 0 — Technical Decisions
- **CLI**: `clap` v4 with derive macros
- **Error handling**: `thiserror` v2 + `anyhow` v1
- **Logging**: `tracing` + `tracing-subscriber` with env-filter
- **Serialization**: `serde` + `serde_json` for plugin cache
- **Dynamic loading**: `libloading` v0.8 (manual COM FFI for VST3 factory access)
- **Platform dirs**: `dirs` v6
- **Audio I/O**: `cpal` v0.15 for cross-platform audio
- **Signal handling**: `ctrlc` v3 for graceful shutdown
- VST3 interop approach: Manual COM vtable FFI for scanning, component instantiation, and audio processing
- MIDI: `midir` planned for Phase 4

#### Phase 1 — Project Foundations
- Module structure: `app/`, `audio/`, `midi/`, `host/`, `vst3/`, `error.rs`
- Error types: `HostError`, `Vst3Error`, `AudioError` with `thiserror`
- Logging: `tracing` with env-filter subscriber
- CLI: `scan`, `list`, `run`, `devices` subcommands via `clap`

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
- **CLI options**: `--device`, `--sample-rate`, `--buffer-size`, `--no-tone` flags on `run` command

### Test Results
- 44 unit tests passing (scanner, cache, module utilities, COM struct layouts, host context, process buffers, tone generator, audio device)
- Clean build with zero warnings
- Successfully scans real VST3 plugins on macOS (tested with FabFilter Pro-MB, Pro-Q 4)
- IPluginFactory2 extended metadata (subcategories, vendor, version) retrieved correctly
- Audio device enumeration working (cpal)

### Documentation
- `USER_GUIDE.md` — end-user guide covering installation, all CLI commands, plugin search paths, cache details, logging, and troubleshooting

### Next Steps (Phase 4 — MIDI, Parameters, and Basic Automation)
- Add `midir` for MIDI device input
- Translate MIDI events to VST3 event structures
- Enumerate plugin parameters (IEditController)
- Build host-side parameter registry
- Apply parameter updates with sample-accurate timing
