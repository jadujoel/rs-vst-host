# rs-vst-host

A minimal VST3 plugin host written in Rust. Discover, load, and run VST3 audio plugins from the command line.

## Features

- **Plugin scanning** — Discover VST3 plugins in standard OS directories with metadata extraction via manual COM FFI
- **Plugin cache** — JSON-based cache for instant plugin listing without re-scanning
- **Real-time audio** — Load and run plugins with real-time audio processing via `cpal`
- **MIDI input** — Connect MIDI devices to send notes to instrument plugins via `midir`
- **Parameter introspection** — Enumerate and display plugin parameters via IEditController (supports both single-component and split component/controller architectures)
- **Audio devices** — Enumerate and select audio output devices
- **MIDI devices** — Enumerate and select MIDI input ports
- **Test tone** — Built-in 440 Hz sine wave generator for testing effect plugins
- **Cross-platform** — macOS, Linux, and Windows support

## Requirements

- Rust 2024 edition (1.85+)
- One or more VST3 plugins installed in a standard location

## Quick Start

```sh
# Build
cargo build --release

# Scan for installed VST3 plugins
rs-vst-host scan

# List discovered plugins
rs-vst-host list

# Run a plugin with real-time audio
rs-vst-host run "Plugin Name"

# Run with MIDI input
rs-vst-host run "Plugin Name" --midi "IAC Driver Bus 1"

# List audio output devices
rs-vst-host devices

# List MIDI input ports
rs-vst-host midi-ports
```

## Commands

| Command | Description |
|---------|-------------|
| `scan [--paths <DIR>...]` | Discover VST3 plugins and cache metadata |
| `list` | Display cached plugins |
| `run <PLUGIN> [OPTIONS]` | Load a plugin and process audio in real time |
| `devices` | List available audio output devices |
| `midi-ports` | List available MIDI input ports |

### `run` Options

| Option | Description |
|--------|-------------|
| `-d, --device <NAME>` | Audio output device (default: system default) |
| `-m, --midi <PORT>` | MIDI input port name |
| `-s, --sample-rate <HZ>` | Sample rate in Hz |
| `-B, --buffer-size <FRAMES>` | Buffer size in frames |
| `--no-tone` | Disable the 440 Hz test tone input |
| `--list-params` | List plugin parameters after loading |

## Architecture

```
src/
├── main.rs          # Entry point, CLI dispatch
├── error.rs         # Error types (HostError, Vst3Error, AudioError, MidiError)
├── app/
│   ├── cli.rs       # CLI argument definitions (clap derive)
│   ├── commands.rs  # Command implementations
│   └── interactive.rs # Interactive command shell for runtime parameter control
├── audio/
│   ├── device.rs    # cpal audio device management
│   └── engine.rs    # Audio processing engine, test tone generator
├── host/
│   └── mod.rs       # Host-side abstractions
├── midi/
│   ├── device.rs    # MIDI device enumeration and input via midir
│   └── translate.rs # MIDI to VST3 event translation
└── vst3/
    ├── cache.rs     # JSON plugin cache
    ├── cf_bundle.rs # CoreFoundation CFBundleRef FFI (macOS)
    ├── com.rs       # VST3 COM vtable definitions (IComponent, IAudioProcessor, IEditController, IEventList)
    ├── component_handler.rs # IComponentHandler COM for parameter notifications
    ├── event_list.rs    # IEventList COM implementation for MIDI events
    ├── host_context.rs  # IHostApplication COM implementation
    ├── instance.rs  # VST3 component lifecycle management
    ├── module.rs    # Dynamic library loading, IPluginFactory FFI
    ├── param_changes.rs # IParameterChanges + IParamValueQueue COM implementations
    ├── params.rs    # Parameter registry via IEditController
    ├── process.rs   # Process buffer management (interleaved ↔ deinterleaved)
    ├── process_context.rs # ProcessContext transport timing
    ├── scanner.rs   # Plugin directory scanning
    └── types.rs     # Shared types
```

### VST3 Interop

This project uses **manual COM FFI** rather than the `vst3-sys` crate. All VST3 interface vtables are defined as `#[repr(C)]` structs with function pointers, matching the Steinberg SDK binary layout. This gives full control over the host–plugin boundary without external binding dependencies.

### Audio Pipeline

1. `cpal` opens an output stream on the selected device
2. The audio callback locks the shared `AudioEngine`
3. Input buffers are filled (test tone or silence)
4. MIDI messages are drained from the lock-free receiver and translated to VST3 events
5. Interleaved samples are deinterleaved into per-channel VST3 buffers
6. The VST3 plugin's `process()` is called with audio buffers and event list
7. Output is interleaved back for `cpal`

## Plugin Search Paths

| Platform | Paths |
|----------|-------|
| macOS | `/Library/Audio/Plug-Ins/VST3`, `~/Library/Audio/Plug-Ins/VST3` |
| Linux | `/usr/lib/vst3`, `/usr/local/lib/vst3`, `~/.vst3` |
| Windows | `%ProgramFiles%\Common Files\VST3` |

Additional paths can be added with `scan --paths <DIR>`.

## Logging

```sh
RUST_LOG=debug rs-vst-host run "My Plugin"
RUST_LOG=rs_vst_host::vst3=trace rs-vst-host scan
```

## Dependencies

| Crate | Purpose |
|-------|---------|
| `clap` 4 | CLI argument parsing |
| `cpal` 0.15 | Cross-platform audio I/O |
| `ctrlc` 3 | Ctrl+C signal handling |
| `midir` 0.10 | Cross-platform MIDI input |
| `libloading` 0.8 | Dynamic library loading |
| `serde` / `serde_json` | Plugin cache serialization |
| `thiserror` / `anyhow` | Error handling |
| `tracing` | Structured logging |
| `dirs` 6 | Platform-specific directories |

## Testing

```sh
cargo test
```

237 unit tests covering error types, CLI parsing, scanner, cache I/O, COM struct layouts, IID UUID verification, host context, process buffers, tone generation, audio device enumeration, MIDI receiver, MIDI-to-VST3 translation, event list COM, parameter registry, parameter changes, component handler, process context, interactive commands, CFBundleRef, and concurrency.

See [CODE_COVERAGE.md](CODE_COVERAGE.md) for detailed per-module coverage analysis.

## Documentation

- [USER_GUIDE.md](USER_GUIDE.md) — Detailed usage guide with examples and troubleshooting
- [PLAN.md](PLAN.md) — Development roadmap and phased implementation plan
- [STATUS.md](STATUS.md) — Current project status and progress
- [CHANGELOG.md](CHANGELOG.md) — Version history
- [CODE_COVERAGE.md](CODE_COVERAGE.md) — Test coverage analysis by module

## Roadmap

- [x] Phase 1 — Project foundations
- [x] Phase 2 — VST3 plugin discovery and loading (M1)
- [x] Phase 3 — Audio engine integration (M2, M3)
- [x] Phase 4 — MIDI input, parameters, automation (M4)
- [x] Phase 5 — Host UX (MVP CLI) (M5)
- [x] Phase 6 — Validation and quality gates (223 tests)
- [x] Phase 7 — Bug fixes and compatibility (IID fix, CFBundleRef, IPluginFactory3)
- [ ] Phase 8 — Beyond MVP (editor windows, presets, routing)

## License

See [LICENSE](LICENSE) for details.
