# rs-vst-host

A minimal VST3 plugin host written in Rust. Discover, load, and run VST3 audio plugins from the command line.

## Features

- **Plugin scanning** — Discover VST3 plugins in standard OS directories with metadata extraction via manual COM FFI
- **Plugin cache** — JSON-based cache for instant plugin listing without re-scanning
- **Real-time audio** — Load and run plugins with real-time audio processing via `cpal`
- **Audio devices** — Enumerate and select audio output devices
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

# List audio output devices
rs-vst-host devices
```

## Commands

| Command | Description |
|---------|-------------|
| `scan [--paths <DIR>...]` | Discover VST3 plugins and cache metadata |
| `list` | Display cached plugins |
| `run <PLUGIN> [OPTIONS]` | Load a plugin and process audio in real time |
| `devices` | List available audio output devices |

### `run` Options

| Option | Description |
|--------|-------------|
| `-d, --device <NAME>` | Audio output device (default: system default) |
| `-s, --sample-rate <HZ>` | Sample rate in Hz |
| `-b, --buffer-size <FRAMES>` | Buffer size in frames |
| `--no-tone` | Disable the 440 Hz test tone input |

## Architecture

```
src/
├── main.rs          # Entry point, CLI dispatch
├── error.rs         # Error types (HostError, Vst3Error, AudioError)
├── app/
│   ├── cli.rs       # CLI argument definitions (clap derive)
│   └── commands.rs  # Command implementations
├── audio/
│   ├── device.rs    # cpal audio device management
│   └── engine.rs    # Audio processing engine, test tone generator
├── host/
│   └── mod.rs       # Host-side abstractions
├── midi/
│   └── mod.rs       # MIDI support (planned)
└── vst3/
    ├── cache.rs     # JSON plugin cache
    ├── com.rs       # VST3 COM vtable definitions (IComponent, IAudioProcessor)
    ├── host_context.rs  # IHostApplication COM implementation
    ├── instance.rs  # VST3 component lifecycle management
    ├── module.rs    # Dynamic library loading, IPluginFactory FFI
    ├── process.rs   # Process buffer management (interleaved ↔ deinterleaved)
    ├── scanner.rs   # Plugin directory scanning
    └── types.rs     # Shared types
```

### VST3 Interop

This project uses **manual COM FFI** rather than the `vst3-sys` crate. All VST3 interface vtables are defined as `#[repr(C)]` structs with function pointers, matching the Steinberg SDK binary layout. This gives full control over the host–plugin boundary without external binding dependencies.

### Audio Pipeline

1. `cpal` opens an output stream on the selected device
2. The audio callback locks the shared `AudioEngine`
3. Input buffers are filled (test tone or silence)
4. Interleaved samples are deinterleaved into per-channel VST3 buffers
5. The VST3 plugin's `process()` is called
6. Output is interleaved back for `cpal`

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
| `libloading` 0.8 | Dynamic library loading |
| `serde` / `serde_json` | Plugin cache serialization |
| `thiserror` / `anyhow` | Error handling |
| `tracing` | Structured logging |
| `dirs` 6 | Platform-specific directories |

## Testing

```sh
cargo test
```

44 unit tests covering scanner, cache, COM struct layouts, host context, process buffers, tone generation, and audio device enumeration.

## Documentation

- [USER_GUIDE.md](USER_GUIDE.md) — Detailed usage guide with examples and troubleshooting
- [PLAN.md](PLAN.md) — Development roadmap and phased implementation plan
- [STATUS.md](STATUS.md) — Current project status and progress
- [CHANGELOG.md](CHANGELOG.md) — Version history

## Roadmap

- [x] Phase 1 — Project foundations
- [x] Phase 2 — VST3 plugin discovery and loading (M1)
- [x] Phase 3 — Audio engine integration (M2, M3)
- [ ] Phase 4 — MIDI input, parameters, automation (M4)
- [ ] Phase 5 — Host UX polish (M5)
- [ ] Phase 6 — Validation and quality gates
- [ ] Phase 7 — Beyond MVP (editor windows, presets, routing)

## License

See [LICENSE](LICENSE) for details.
