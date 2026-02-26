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
- **Plugin crash sandbox** — Signal-handler-based crash isolation: if a plugin crashes (SIGBUS/SIGSEGV/SIGABRT), the host recovers gracefully and continues running. Crashed plugins are tainted and blocked from re-activation to prevent heap corruption on reload
- **Debug & profiling** — Optional feature-gated diagnostics: heap integrity checks (`malloc_zone_check`), backtrace capture in signal handler, dhat heap profiler, Chrome trace export, `--malloc-debug` CLI flag
- **Cross-platform** — macOS, Linux, and Windows support
- **Graphical interface** — Liquid Glass style GUI using `egui`/`eframe` with plugin browser, rack, parameter view (with staging for inactive plugins), device selection, session save/load, and improved text contrast on glass panels

## Requirements

- Rust 2024 edition (1.85+)
- One or more VST3 plugins installed in a standard location

## Quick Start

```sh
# Build
cargo build --release

# Scan for installed VST3 plugins
cargo run -- scan

# List discovered plugins
cargo run -- list

# Run a plugin with real-time audio
cargo run -- run "Plugin Name"

# Run with MIDI input
cargo run -- run "Plugin Name" --midi "IAC Driver Bus 1"

# List audio output devices
cargo run -- devices

# List MIDI input ports
cargo run -- midi-ports

# Launch the graphical interface
cargo run -- gui
```

## Commands

| Command | Description |
|---------|-------------|
| `scan [--paths <DIR>...]` | Discover VST3 plugins and cache metadata |
| `list` | Display cached plugins |
| `run <PLUGIN> [OPTIONS]` | Load a plugin and process audio in real time |
| `devices` | List available audio output devices |
| `midi-ports` | List available MIDI input ports |
| `gui [--safe-mode] [--malloc-debug]` | Launch the graphical user interface |

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
├── main.rs          # Entry point, CLI dispatch, tracing init
├── error.rs         # Error types (HostError, Vst3Error, AudioError, MidiError)
├── diagnostics.rs   # Heap integrity checks, malloc env detection, dhat profiler (feature-gated)
├── app/
│   ├── cli.rs       # CLI argument definitions (clap derive)
│   ├── commands.rs  # Command implementations
│   └── interactive.rs # Interactive command shell for runtime parameter control
├── audio/
│   ├── device.rs    # cpal audio device management
│   └── engine.rs    # Audio processing engine, test tone generator
├── gui/
│   ├── app.rs       # HostApp eframe::App — plugin browser, rack, transport, parameter view, editor buttons
│   ├── backend.rs   # Host backend — bridges GUI with audio engine, MIDI, and plugin editors
│   ├── editor.rs    # Native OS window management for VST3 plugin editor views (macOS NSWindow)
│   ├── session.rs   # Session save/load — serialize/restore host state as JSON
│   └── theme.rs     # Liquid Glass theme — colours, corner radii, shadows, styling
├── host/
│   └── mod.rs       # Host-side abstractions
├── midi/
│   ├── device.rs    # MIDI device enumeration and input via midir
│   └── translate.rs # MIDI to VST3 event translation
└── vst3/
    ├── cache.rs     # JSON plugin cache
    ├── cf_bundle.rs # CoreFoundation CFBundleRef FFI (macOS)
    ├── com.rs       # VST3 COM vtable definitions (IComponent, IAudioProcessor, IEditController, IEventList, IPlugView, IPlugFrame)
    ├── component_handler.rs # IComponentHandler COM for parameter notifications
    ├── event_list.rs    # IEventList COM implementation for MIDI events
    ├── host_context.rs  # IHostApplication COM implementation
    ├── instance.rs  # VST3 component lifecycle management (incl. editor view creation)
    ├── module.rs    # Dynamic library loading, IPluginFactory FFI
    ├── param_changes.rs # IParameterChanges + IParamValueQueue COM implementations
    ├── params.rs    # Parameter registry via IEditController
    ├── plug_frame.rs # IPlugFrame COM implementation for editor resize requests
    ├── process.rs   # Process buffer management (interleaved ↔ deinterleaved)
    ├── process_context.rs # ProcessContext transport timing
    ├── sandbox.rs   # Plugin crash sandbox (sigsetjmp/siglongjmp signal recovery)
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
| `libc` 0.2 | Low-level signal handling for plugin sandbox |
| `backtrace` 0.3 | Backtrace capture for crash diagnostics |
| `eframe` / `egui` 0.31 | Graphical user interface |
| `dhat` 0.3 | Heap profiler (optional, `debug-alloc` feature) |
| `tracing-chrome` 0.7 | Chrome trace export (optional, `debug-trace` feature) |

## Testing

```sh
cargo test
```

441 unit tests covering error types, GUI theme, GUI app state (safe mode, transport sync, editor integration, parameter search, parameter staging for inactive plugins), GUI backend (editor lifecycle, audio status, transport push), GUI session, plugin editor window management, IPlugFrame COM, CLI parsing (incl. safe-mode, malloc-debug), scanner, cache I/O, COM struct layouts, IID UUID verification (incl. IPlugView/IPlugFrame), host context, process buffers, tone generation, audio device enumeration, MIDI receiver, MIDI-to-VST3 translation, event list COM, parameter registry, parameter changes, component handler, process context, interactive commands, CFBundleRef, plugin sandbox (signal recovery, crash isolation, nested sandboxing, crash-safe library unload, backtrace capture, heap integrity checks), diagnostics module (heap check, malloc env, profiler), crash-safe host object lifecycle (conditional leak/destroy), and concurrency. 442 tests with `--features debug-tools`.

See [CODE_COVERAGE.md](CODE_COVERAGE.md) for detailed per-module coverage analysis.

## Debugging

The project includes optional diagnostic features for investigating heap corruption and performance issues, gated behind Cargo feature flags (zero-cost when disabled).

### Heap Isolation (mimalloc)

By default, all Rust allocations use **mimalloc** instead of the system allocator. Since VST3 plugins are loaded C++ code that uses system malloc directly, this isolates the host's heap from plugin-induced corruption. If a buggy plugin corrupts the system malloc heap, our Rust data structures remain intact.

The `debug-alloc` feature overrides mimalloc with `dhat` for heap profiling.

### Feature Flags

| Flag | Description |
|------|-------------|
| `debug-alloc` | Enable `dhat` heap profiler as global allocator (replaces mimalloc) |
| `debug-trace` | Enable Chrome trace export via `tracing-chrome` |
| `debug-tools` | Enable both `debug-alloc` and `debug-trace` |

```sh
# Build with all debug features
cargo build --features debug-tools

# Run with heap profiler
cargo run --features debug-alloc -- gui

# Run with Chrome trace export
cargo run --features debug-trace -- gui
# → produces a trace file viewable in chrome://tracing
```

### Heap Integrity Checks

On macOS, the host calls `malloc_zone_check(NULL)` at key points to detect heap corruption:
- After sandbox crash recovery (in the signal handler recovery path)
- During `Vst3Instance::Drop` after a crash
- Periodically in the GUI update loop (when `--malloc-debug` is active)

### `--malloc-debug` Flag

Launch the GUI with `--malloc-debug` to enable enhanced heap diagnostics:

```sh
cargo run -- gui --malloc-debug
```

This prints instructions for setting macOS malloc environment variables (`MallocGuardEdges`, `MallocScribble`, `MallocCheckHeapStart`, etc.) and enables periodic heap integrity checking in the GUI. If corruption is detected, a red warning banner appears at the top of the window.

### Backtrace Capture

When a plugin crashes inside the sandbox, the signal handler captures a backtrace (up to 64 frames) using the signal-safe `backtrace()` function before performing `siglongjmp`. The frames are symbolicated after recovery and included in the `PluginCrash` diagnostic output.

## Documentation

- [USER_GUIDE.md](USER_GUIDE.md) — Detailed usage guide with examples and troubleshooting
- [PLAN.md](PLAN.md) — Development roadmap and phased implementation plan
- [STATUS.md](STATUS.md) — Current project status and progress
- [CHANGELOG.md](CHANGELOG.md) — Version history
- [CODE_COVERAGE.md](CODE_COVERAGE.md) — Test coverage analysis by module
- [DEBUGGING.md](DEBUGGING.md) — Debug and profiling infrastructure plan
- [PRD.md](PRD.md) — Product requirements for the GUI application
- [USER_INTERACTION_PLAN.md](USER_INTERACTION_PLAN.md) — GUI interaction plan for plugin parameter editing

## Roadmap

- [x] Phase 1 — Project foundations
- [x] Phase 2 — VST3 plugin discovery and loading (M1)
- [x] Phase 3 — Audio engine integration (M2, M3)
- [x] Phase 4 — MIDI input, parameters, automation (M4)
- [x] Phase 5 — Host UX (MVP CLI) (M5)
- [x] Phase 6 — Validation and quality gates (223 tests)
- [x] Phase 7 — Bug fixes and compatibility (IID fix, CFBundleRef, IPluginFactory3)
- [x] Phase 7 Step 1 — GUI skeleton (plugin browser, rack, transport controls)
- [x] Phase 7 Step 2 — Live audio integration in GUI (backend bridge, parameter view, device selection, session save/load)
- [x] Phase 7 Step 3 — Plugin editor windows, transport sync, audio status, parameter search, safe mode
- [ ] Phase 8 — Beyond MVP (presets, routing, multi-instance)

## License

See [LICENSE](LICENSE) for details.
