# rs-vst-host

A minimal VST3 plugin host written in Rust. Discover, load, and run VST3 audio plugins from the command line.

## Features

- **Plugin scanning** — Discover VST3 plugins in standard OS directories with metadata extraction via the `vst3` crate (coupler-rs/vst3-rs)
- **Plugin cache** — JSON-based cache for instant plugin listing without re-scanning
- **Real-time audio** — Load and run plugins with real-time audio processing via `cpal`
- **MIDI input** — Connect MIDI devices to send notes to instrument plugins via `midir`
- **Parameter introspection** — Enumerate and display plugin parameters via IEditController (supports both single-component and split component/controller architectures)
- **Audio devices** — Enumerate and select audio output devices
- **MIDI devices** — Enumerate and select MIDI input ports
- **Test tone** — Built-in 440 Hz sine wave generator for testing effect plugins
- **Plugin crash sandbox** — Signal-handler-based crash isolation: if a plugin crashes (SIGBUS/SIGSEGV/SIGABRT), the host recovers gracefully and continues running. Crashed plugins are tainted and blocked from re-activation to prevent heap corruption on reload
- **Process-per-plugin sandboxing** — Optional process isolation mode where each plugin runs in its own child process with POSIX shared memory for audio and Unix sockets for control IPC. A crashed plugin only kills its child process — the host is completely unaffected (like Bitwig Studio)
- **Debug & profiling** — Optional feature-gated diagnostics: heap integrity checks (`malloc_zone_check`), backtrace capture in signal handler, dhat heap profiler, Chrome trace export, `--malloc-debug` CLI flag
- **Cross-platform** — macOS, Linux, and Windows support
- **Graphical interface** — GUI using `egui`/`eframe` with plugin browser, rack, parameter view (with staging for inactive plugins), device selection, session save/load, and improved text contrast on glass panels
- **Plugin state persistence** — Full plugin state save/restore via VST3 IBStream COM interfaces. Plugin state (component + controller) is captured on session save and restored on plugin activation from a loaded session. Session format v2.0 with base64-encoded binary state blobs, backward compatible with v1.0 sessions
- **Preset management** — Save, load, and list user presets per plugin. Presets are stored as JSON files in `~/.rs-vst-host/presets/<plugin-name>/` with base64-encoded state blobs. GUI preset toolbar with previous/next navigation, save dialog, init reset, and one-click preset loading
- **Multi-plugin routing graph** — DAG-based audio routing model with topological sort, cycle detection, and serial chain helpers. Visual routing editor with compact chain overview and advanced 2D node editor with Bézier curve connections. Graph-aware audio engine processes multi-plugin chains in topological order
- **Drag-and-drop rack reordering** — Grip handle on each rack slot for drag-based reorder, insertion markers, undo integration
- **Performance monitoring** — Lock-free SPSC ring buffer for param changes, xrun (buffer underrun) detection, CPU load monitoring with EMA smoothing, real-time thread priority (macOS QOS_CLASS_USER_INTERACTIVE, Linux SCHED_FIFO)
- **Latency compensation** — Sample-accurate delay lines for compensating plugin processing latency in multi-plugin chains
- **Plugin compatibility** — Bus arrangement fallback chain (stereo → mono → default), robust bus setup for maximum plugin compatibility
- **CI/CD & Distribution** — GitHub Actions CI for macOS/Linux/Windows (check, clippy, test, fmt, bench), macOS .app bundle creation, bundle script
- **GUI crash isolation** — The GUI runs in a separate child process by default, supervised by the main process. If a plugin crashes the GUI, the supervisor relaunches it automatically while audio continues uninterrupted. Use `--in-process` for legacy single-process mode
- **Audio process isolation** — The audio engine and plugin backend run in a separate child process from the supervisor. If a plugin crashes the audio process, the supervisor stays alive, restarts the audio worker, and restores the rack configuration. The GUI is notified and remains functional throughout

## Requirements

- Rust 2024 edition (1.85+)
- One or more VST3 plugins installed in a standard location

## Quick Start

```sh
# Build
cargo build --release

# Scan for installed VST3 plugins
cargo run -- scan

# Scan only specific directories (skip default system paths)
cargo run -- scan --paths ./vsts /other/dir

# Add a custom directory to scan permanently
cargo run -- scan-paths add /path/to/my/plugins

# List persistent scan paths
cargo run -- scan-paths list

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

# Launch GUI scanning only specific plugin directories
cargo run -- gui --paths ./vsts
```

## Commands

| Command | Description |
|---------|-------------|
| `scan [--paths <DIR>...]` | Discover VST3 plugins and cache metadata. When `--paths` is used, only those directories are scanned (defaults excluded) |
| `scan-paths add <DIR>` | Add a directory to the persistent scan path list |
| `scan-paths remove <DIR>` | Remove a directory from the persistent scan path list |
| `scan-paths list` | Show all persistent scan paths |
| `list` | Display cached plugins |
| `run <PLUGIN> [OPTIONS]` | Load a plugin and process audio in real time |
| `devices` | List available audio output devices |
| `midi-ports` | List available MIDI input ports |
| `gui [--paths <DIR>...] [--safe-mode] [--malloc-debug] [--in-process]` | Launch the graphical user interface |

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
├── e2e_tests.rs     # End-to-end integration tests using real VST3 plugins
├── app/
│   ├── cli.rs       # CLI argument definitions (clap derive)
│   ├── commands.rs  # Command implementations
│   ├── config.rs    # Persistent configuration (extra scan paths)
│   └── interactive.rs # Interactive command shell for runtime parameter control
├── audio/
│   ├── device.rs    # cpal audio device management
│   └── engine.rs    # Audio processing engine, test tone generator
├── gui/
│   ├── app.rs       # HostApp eframe::App — plugin browser, rack, transport, parameter view, editor buttons
│   ├── backend.rs   # Host backend — bridges GUI with audio engine, MIDI, and plugin editors (supports in-process and sandboxed modes)
│   ├── editor.rs    # Native OS window management for VST3 plugin editor views (macOS NSWindow)
│   ├── audio_worker.rs # Audio worker child process — runs HostBackend + AudioEngine in isolated process
│   ├── gui_worker.rs # GUI child process — eframe App receiving state from supervisor via IPC
│   ├── ipc.rs       # IPC protocol — GuiAction/SupervisorUpdate/AudioCommand message types, DecodeError
│   ├── session.rs   # Session save/load — serialize/restore host state as JSON
│   ├── supervisor.rs # Supervisor — spawns/monitors both GUI and audio worker children, relays messages, handles crash recovery
│   └── theme.rs     # Theme — colours, corner radii, shadows, styling
├── host/
│   └── mod.rs       # Host-side abstractions
├── ipc/
│   ├── messages.rs  # IPC protocol — serializable host↔worker message types, wire encoding
│   ├── shm.rs       # POSIX shared memory for zero-copy audio buffer exchange
│   ├── worker.rs    # Child process entry point — loads and runs a single VST3 plugin
│   └── proxy.rs     # Host-side proxy — spawns child process, manages IPC communication
├── midi/
│   ├── device.rs    # MIDI device enumeration and input via midir
│   └── translate.rs # MIDI to VST3 event translation
└── vst3/
    ├── cache.rs     # JSON plugin cache
    ├── cf_bundle.rs # CoreFoundation CFBundleRef FFI (macOS)
    ├── com.rs       # VST3 type re-exports from vst3-rs crate (IComponent, IAudioProcessor, IEditController, IEventList, IPlugView, IPlugFrame)
    ├── component_handler.rs # IComponentHandler COM for parameter notifications
    ├── event_list.rs    # IEventList COM implementation for MIDI events
    ├── host_context.rs  # IHostApplication COM implementation
    ├── ibstream.rs  # IBStream COM implementation for plugin state transfer
    ├── instance.rs  # VST3 component lifecycle management (incl. editor view creation)
    ├── module.rs    # Dynamic library loading, IPluginFactory FFI
    ├── param_changes.rs # IParameterChanges + IParamValueQueue COM implementations
    ├── params.rs    # Parameter registry via IEditController
    ├── plug_frame.rs # IPlugFrame COM implementation for editor resize requests
    ├── presets.rs   # Preset file management (save/load/list, base64 state blobs)
    ├── process.rs   # Process buffer management (interleaved ↔ deinterleaved)
    ├── process_context.rs # ProcessContext transport timing
    ├── sandbox.rs   # Plugin crash sandbox (sigsetjmp/siglongjmp signal recovery)
    ├── scanner.rs   # Plugin directory scanning
    └── types.rs     # Shared types
```

### VST3 Interop

This project uses the **[vst3](https://crates.io/crates/vst3) crate** (v0.3.0, coupler-rs/vst3-rs) for VST3 COM type definitions. All interface vtables (`IComponent`, `IAudioProcessor`, `IEditController`, `IEventList`, etc.) come from the crate's auto-generated bindings, ensuring binary-compatible `#[repr(C)]` layouts matching the Steinberg SDK. Host-side COM objects (event lists, parameter changes, component handler, plug frame, IBStream, host context) implement these vtable types with custom Rust logic. The `com.rs` module centralizes all vst3-rs re-exports and provides convenience helpers for IID constants, type conversions, and event construction.

### VST3 Plugin Class Types

A VST3 module (`.vst3` bundle) can expose multiple COM classes through its `IPluginFactory`. The host only displays **Audio Module Class** entries in the plugin browser—the others are internal and used behind the scenes:

| Class Category | Purpose |
|---|---|
| **Audio Module Class** | The main plugin class implementing `IComponent` (audio processor). This is what users select and load. It handles audio I/O, parameter state, and bus configuration. |
| **Component Controller Class** | Implements `IEditController` (parameter UI + editor views). In *single-component* plugins, the audio component also implements this interface directly. In *split-architecture* plugins (common with JUCE-based plugins), it is a separate COM object. The host creates it automatically when needed for parameter introspection or opening the plugin editor. |
| **Plugin Compatibility Class** | A compatibility shim that allows older VST2-era plugin IDs to be recognized and mapped to the VST3 version. DAWs use this for session recall when a user migrates from VST2 to VST3. It is never loaded directly. |

When a split-architecture plugin is loaded, the host automatically:
1. Queries the component for `IEditController` (single-component check)
2. Falls back to `getControllerClassId()` → `factory.createInstance()` to create the separate controller
3. Calls `setComponentState()` on the controller with the component’s state (required for JUCE plugins)
4. Connects both via `IConnectionPoint` for bidirectional communication

### Audio Pipeline

**In-process mode** (default):

1. `cpal` opens an output stream on the selected device
2. The audio callback locks the shared `AudioEngine`
3. Input buffers are filled (test tone or silence)
4. MIDI messages are drained from the lock-free receiver and translated to VST3 events
5. Interleaved samples are deinterleaved into per-channel VST3 buffers
6. The VST3 plugin's `process()` is called with audio buffers and event list
7. Output is interleaved back for `cpal`

**Sandboxed mode** (`process_isolation = true`):

1. `cpal` opens an output stream on the selected device
2. The audio callback locks the shared `PluginProcess` proxy
3. The proxy sends a `Process` message over the Unix socket (with MIDI events, parameter changes, transport state)
4. The child process receives the message, calls the VST3 plugin's `process()`, writes output to shared memory
5. The proxy reads output from shared memory and interleaves it back for `cpal`

## Plugin Search Paths

| Platform | Paths |
|----------|-------|
| macOS | `/Library/Audio/Plug-Ins/VST3`, `~/Library/Audio/Plug-Ins/VST3` |
| Linux | `/usr/lib/vst3`, `/usr/local/lib/vst3`, `~/.vst3` |
| Windows | `%ProgramFiles%\Common Files\VST3` |

### Adding Custom Scan Paths (Persistent)

To permanently add a directory so it is scanned every time you run `scan`:

```sh
# Add a custom directory
rs-vst-host scan-paths add /path/to/my/plugins

# Add another one
rs-vst-host scan-paths add ~/Music/VST3

# View all persistent paths
rs-vst-host scan-paths list

# Remove a path you no longer need
rs-vst-host scan-paths remove /path/to/my/plugins

# Now scan — persistent paths are included automatically
rs-vst-host scan
```

Persistent paths are stored in the config file at:
- **macOS**: `~/Library/Application Support/rs-vst-host/config.json`
- **Linux**: `~/.local/share/rs-vst-host/config.json`
- **Windows**: `%APPDATA%\rs-vst-host\config.json`

### One-Time Extra Paths

To scan an additional directory without saving it permanently, use `--paths`:

```sh
rs-vst-host scan --paths /tmp/test-plugins
```

These paths are only used for that single scan invocation.

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
| `vst3` 0.3 | VST3 COM type definitions (coupler-rs/vst3-rs) |
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

Run the full test suite (unit tests + clippy + Miri + ASan):

```sh
bash test.bash
```

Or run just the standard unit tests:

```sh
cargo test --lib
```

Run E2E integration tests against real VST3 plugins (requires plugins in `vsts/`):

```sh
cargo test --lib e2e_tests -- --test-threads=1
```

Test screenshots end up in target/test-screenshots/

1414 tests (763 unit + 651 binary/integration) covering error types, GUI theme, GUI app state (safe mode, transport sync, editor integration, parameter search, parameter staging for inactive plugins), GUI backend (editor lifecycle, audio status, transport push, process isolation mode), GUI session (v2.0 format with state blobs, v1 backward compat), plugin state persistence (IBStream capture/restore, base64 encoding), preset management (file I/O, directory management, sanitization), plugin editor window management (NSApplication initialization, AppKit event pumping, activation policy preservation), IPlugFrame COM, IBStream COM (state transfer for split-architecture plugins), CLI parsing (incl. safe-mode, malloc-debug), scanner, cache I/O, COM struct layouts, IID UUID verification (incl. IPlugView/IPlugFrame), host context, process buffers, tone generation, audio device enumeration, MIDI receiver, MIDI-to-VST3 translation, event list COM, parameter registry, parameter changes, component handler, process context, interactive commands, CFBundleRef, plugin sandbox (signal recovery, crash isolation, nested sandboxing, crash-safe library unload, backtrace capture, heap integrity checks), diagnostics module (heap check, malloc env, profiler), crash-safe host object lifecycle (conditional leak/destroy), IPC messages (serialization, wire protocol), shared memory (create/open, audio transfer), worker process (state management, message handling), plugin process proxy (transport, shutdown), Miri dynamic analysis (COM vtable lifecycle, event byte roundtrip, buffer pointer chains, MIDI→ProcessData integration, thread safety), ASan memory safety (host_alloc lifecycle, COM objects, ProcessBuffers, shared memory, events, MIDI pipeline, sandbox non-crash, IPC, concurrent COM, full mock process), and concurrency. All VST3 COM types use vst3-rs crate bindings.

109 of these tests also pass under [Miri](https://github.com/rust-lang/miri) for dynamic undefined behavior detection. 564 pass under [AddressSanitizer](https://clang.llvm.org/docs/AddressSanitizer.html) for native memory error detection. See [DYNAMIC_ANALYSIS.md](docs/DYNAMIC_ANALYSIS.md) for the full guide.

See [CODE_COVERAGE.md](docs/CODE_COVERAGE.md) for detailed per-module coverage analysis.

## Benchmarks

Performance benchmarks use the [Divan](https://github.com/nvzqz/divan) framework. Run all benchmarks:

```sh
cargo bench
```

Run a specific benchmark:

```sh
cargo bench --bench audio_engine
cargo bench --bench process_buffers
cargo bench --bench event_list
```

11 benchmark suites covering all hot paths:

| Benchmark | Module | What it measures |
|-----------|--------|------------------|
| `audio_engine` | `audio/engine.rs` | Tone generation, buffer fill at 44.1/96 kHz |
| `process_buffers` | `vst3/process.rs` | Buffer creation, interleave/deinterleave, full cycle |
| `event_list` | `vst3/event_list.rs` | Event add/clear, COM vtable operations |
| `param_changes` | `vst3/param_changes.rs` | Parameter queueing, multi-param, worst-case scan |
| `midi_translate` | `midi/translate.rs` | MIDI→VST3 translation, batch processing |
| `ipc_messages` | `ipc/messages.rs` | Serialization encode/decode, roundtrip |
| `process_context` | `vst3/process_context.rs` | Transport advance, tempo, time signature |
| `host_alloc` | `vst3/host_alloc.rs` | system_alloc vs mimalloc Box allocation |
| `diagnostics` | `diagnostics.rs` | Heap check, malloc env inspection |
| `session_serde` | `gui/session.rs` | Session capture/restore/serde roundtrip |
| `cache_serde` | `vst3/cache.rs` | ScanCache serde, module roundtrip |

See [PERFORMANCE_CHANGELOG.md](docs/PERFORMANCE_CHANGELOG.md) for baseline results and regression tracking.

## Debugging

The project includes optional diagnostic features for investigating heap corruption and performance issues, gated behind Cargo feature flags (zero-cost when disabled).

### Heap Isolation (mimalloc)

By default, all Rust allocations use **mimalloc** instead of the system allocator. Since VST3 plugins are loaded C++ code that uses system malloc directly, this isolates the host's heap from plugin-induced corruption. If a buggy plugin corrupts the system malloc heap, our Rust data structures remain intact.

Plugin-facing COM objects (`HostApplication`, `HostComponentHandler`, `HostPlugFrame`) are allocated on the **system** malloc heap via `libc::malloc` (see `host_alloc.rs`). This ensures that even if a plugin incorrectly calls `free()` on a host object instead of using COM `Release()`, the pointer is recognised by macOS system malloc and the process does not abort.

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

- [USER_GUIDE.md](docs/USER_GUIDE.md) — Detailed usage guide with examples and troubleshooting
- [PLAN.md](docs/PLAN.md) — Development roadmap and phased implementation plan
- [STATUS.md](docs/STATUS.md) — Current project status and progress
- [CHANGELOG.md](docs/CHANGELOG.md) — Version history
- [CODE_COVERAGE.md](docs/CODE_COVERAGE.md) — Test coverage analysis by module
- [DYNAMIC_ANALYSIS.md](docs/DYNAMIC_ANALYSIS.md) — Guide to Miri-based dynamic analysis of unsafe code
- [DEBUGGING.md](docs/DEBUGGING.md) — Debug and profiling infrastructure plan
- [PRD.md](docs/PRD.md) — Product requirements for the GUI application
- [USER_INTERACTION_PLAN.md](docs/USER_INTERACTION_PLAN.md) — GUI interaction plan for plugin parameter editing
- [PERFORMANCE_CHANGELOG.md](docs/PERFORMANCE_CHANGELOG.md) — Benchmark results and regression tracking
- [PHASE_8.md](docs/PHASE_8.md) — Detailed plan for Phase 8 (Beyond MVP)

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
- [ ] Phase 8 — Beyond MVP (presets, routing, multi-instance) — [detailed plan](docs/PHASE_8.md)

## License

See [LICENSE](LICENSE) for details.
