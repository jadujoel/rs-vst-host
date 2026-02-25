# rs-vst-host User Guide

A minimal VST3 plugin host written in Rust. Discover, inspect, and run VST3 audio plugins from the command line.

---

## Table of Contents

- [Requirements](#requirements)
- [Installation](#installation)
- [Quick Start](#quick-start)
- [Commands](#commands)
  - [scan](#scan)
  - [list](#list)
  - [run](#run)
  - [devices](#devices)
  - [midi-ports](#midi-ports)
- [Interactive Mode](#interactive-mode)
- [Plugin Search Paths](#plugin-search-paths)
- [Plugin Cache](#plugin-cache)
- [Verbose Logging](#verbose-logging)
- [Examples](#examples)
- [Troubleshooting](#troubleshooting)

---

## Requirements

- **Rust** 2024 edition (1.85+)
- **Operating system**: macOS, Linux, or Windows
- One or more VST3 plugins installed in a standard location

## Installation

Clone the repository and build:

```sh
git clone <repository-url>
cd rs-vst-host
cargo build --release
```

The binary will be at `target/release/rs-vst-host`.

You can also run directly with Cargo during development:

```sh
cargo run -- <command>
```

---

## Quick Start

1. **Scan** for installed VST3 plugins:

   ```sh
   rs-vst-host scan
   ```

2. **List** the discovered plugins:

   ```sh
   rs-vst-host list
   ```

3. **Run** a plugin with real-time audio processing:

   ```sh
   rs-vst-host run "Plugin Name"
   ```

4. **List** available audio output devices:

   ```sh
   rs-vst-host devices
   ```

---

## Commands

### scan

Searches VST3 plugin directories, loads each discovered bundle, extracts metadata (name, vendor, category), and saves the results to a local cache file.

```
rs-vst-host scan [OPTIONS]
```

**Options:**

| Option | Description |
|--------|-------------|
| `-p, --paths <DIR>...` | Additional directories to search for plugins (can be repeated) |

**What it does:**

1. Builds a list of search directories (platform defaults + any extra paths you provide).
2. Recursively discovers all `.vst3` bundles in those directories.
3. Loads each bundle and reads its plugin factory metadata (class names, categories, vendor info).
4. Saves the results to a JSON cache file so `list` can display them instantly.

**Example output:**

```
Scanning for VST3 plugins...

Search paths:
  /Library/Audio/Plug-Ins/VST3
  /Users/you/Library/Audio/Plug-Ins/VST3

Found 3 VST3 bundle(s).

  Loading FabFilter Pro-Q 4... OK (1 class(es))
    - FabFilter Pro-Q 4 [Audio Module Class | Fx]
  Loading FabFilter Pro-MB... OK (1 class(es))
    - FabFilter Pro-MB [Audio Module Class | Fx]
  Loading Surge XT... OK (2 class(es))
    - Surge XT [Audio Module Class | Instrument|Synth]
    - Surge XT Effects [Audio Module Class | Fx]

Scan complete: 3 module(s), 4 plugin class(es) cached.
```

### list

Displays all plugins from the most recent scan cache. No disk scanning is performed — this reads from the cached JSON file created by `scan`.

```
rs-vst-host list
```

**Example output:**

```
Cached plugins (scanned 2026-02-25T10:30:00Z):

    1. FabFilter Pro-Q 4 (FabFilter)
       Category: Audio Module Class | Fx
       Path: /Library/Audio/Plug-Ins/VST3/FabFilter Pro-Q 4.vst3

    2. FabFilter Pro-MB (FabFilter)
       Category: Audio Module Class | Fx
       Path: /Library/Audio/Plug-Ins/VST3/FabFilter Pro-MB.vst3

    3. Surge XT (Surge Synth Team)
       Category: Audio Module Class | Instrument|Synth
       Path: /Library/Audio/Plug-Ins/VST3/Surge XT.vst3
```

If no cache exists, you will see:

```
No plugin cache found. Run 'scan' first.
```

### run

Loads a VST3 plugin and starts real-time audio processing with an interactive command shell. The plugin receives a 440 Hz sine wave test tone as input (for effect plugins) and outputs audio through the selected audio device.

```
rs-vst-host run [OPTIONS] <PLUGIN>
```

**Arguments:**

| Argument | Description |
|----------|-------------|
| `PLUGIN` | Plugin name (as shown in `list`) or path to a `.vst3` bundle |

**Options:**

| Option | Description |
|--------|-------------|
| `-d, --device <NAME>` | Audio output device name (uses system default if not specified) |
| `-m, --midi <PORT>` | MIDI input port name (use `midi-ports` to list) |
| `-s, --sample-rate <HZ>` | Sample rate in Hz (uses device default if not specified) |
| `-B, --buffer-size <FRAMES>` | Buffer size in frames (uses device default if not specified) |
| `--no-tone` | Disable the test tone input signal (silence input) |
| `--list-params` | List plugin parameters after loading |

**What it does:**

1. Resolves the plugin by name (from cache) or by direct `.vst3` bundle path.
2. Loads the plugin module and creates a VST3 component instance.
3. Installs a component handler for plugin parameter notifications.
4. Opens the audio output device and configures processing (sample rate, block size, stereo bus arrangement).
5. Optionally connects a MIDI input port for instrument plugins.
6. Activates the plugin and starts real-time audio processing with transport info.
7. Enters an interactive command shell for runtime parameter control.
8. Type `quit` or press **Ctrl+C** to stop processing and cleanly shut down.

**Example:**

```
$ rs-vst-host run "FabFilter Pro-Q 4" --list-params
Loading plugin: FabFilter Pro-Q 4
Audio device: MacBook Pro Speakers
Audio config: 44100 Hz, 2 ch, buffer: default
Test tone: 440 Hz sine wave

Plugin parameters (28):
  ...

Processing audio. Type 'help' for commands, 'quit' to stop.

> params
  ...
> set 0 0.75
  Frequency = 1500.00 Hz [normalized: 0.7500]
> quit
Stopping...
Done.
```

**Run by path:**

```sh
rs-vst-host run /Library/Audio/Plug-Ins/VST3/MyPlugin.vst3
```

**Run with custom audio settings:**

```sh
rs-vst-host run "My Plugin" --device "BlackHole 2ch" --sample-rate 48000 --buffer-size 256
```

### devices

Lists all available audio output devices on the system.

```
rs-vst-host devices
```

**Example output:**

```
Audio output devices:

    1. BlackHole 2ch
    2. MacBook Pro Speakers (default)
    3. Microsoft Teams Audio
    4. Aggregate Device
```

### midi-ports

Lists all available MIDI input ports on the system.

```
rs-vst-host midi-ports
```

**Example output:**

```
MIDI input ports:

    1. IAC Driver Bus 1
    2. Arturia KeyLab Essential 49
```

---

## Interactive Mode

When running a plugin with `run`, an interactive command shell is available for runtime control. Commands are typed at the `>` prompt while audio is processing.

### Available Commands

| Command | Description |
|---------|-------------|
| `params`, `p` | List all plugin parameters with current values |
| `get <id\|name>` | Get a parameter's current value (by ID or name) |
| `set <id\|name> <value>` | Set a parameter (0.0–1.0 normalized) |
| `tempo <bpm>` | Set tempo in BPM |
| `status` | Show engine status (parameter count, handler state) |
| `help`, `h`, `?` | Show available commands |
| `quit`, `q`, `exit` | Stop audio and exit |

### Parameter Control

Parameters can be addressed by their numeric ID or by name (partial, case-insensitive match):

```
> get 0
  Frequency (ID 0): 1000.00 Hz [normalized: 0.5000]

> set frequency 0.75
  Frequency = 1500.00 Hz [normalized: 0.7500]

> set 0 0.0
  Frequency = 20.00 Hz [normalized: 0.0000]
```

When a plugin changes its own parameters (e.g., via its UI), the change is displayed:

```
  [plugin] Gain = -6.0 dB [normalized: 0.3750]
```

---

## Plugin Search Paths

The scanner automatically checks platform-specific default directories:

### macOS

| Path | Scope |
|------|-------|
| `/Library/Audio/Plug-Ins/VST3` | System-wide plugins |
| `~/Library/Audio/Plug-Ins/VST3` | User-installed plugins |

### Linux

| Path | Scope |
|------|-------|
| `/usr/lib/vst3` | System packages |
| `/usr/local/lib/vst3` | Locally installed |
| `~/.vst3` | User-installed plugins |

### Windows

| Path | Scope |
|------|-------|
| `%ProgramFiles%\Common Files\VST3` | Standard install location |

To scan additional directories, use the `--paths` flag:

```sh
rs-vst-host scan --paths /my/custom/plugins --paths /another/folder
```

---

## Plugin Cache

Scan results are stored as a JSON file in your platform's data directory:

| Platform | Cache location |
|----------|---------------|
| macOS | `~/Library/Application Support/rs-vst-host/plugin-cache.json` |
| Linux | `~/.local/share/rs-vst-host/plugin-cache.json` |
| Windows | `C:\Users\<user>\AppData\Roaming\rs-vst-host\plugin-cache.json` |

The cache includes:
- Scan timestamp
- For each module: file path, factory vendor/URL, and all plugin classes with their names, categories, and subcategories

Re-running `scan` overwrites the cache with fresh results.

---

## Verbose Logging

rs-vst-host uses the `RUST_LOG` environment variable for structured logging via the `tracing` framework.

```sh
# Show info-level logs
RUST_LOG=info rs-vst-host scan

# Show debug-level logs (bundle resolution, cache I/O details)
RUST_LOG=debug rs-vst-host scan

# Show trace-level logs (maximum detail)
RUST_LOG=trace rs-vst-host scan

# Filter to a specific module
RUST_LOG=rs_vst_host::vst3=debug rs-vst-host scan
```

---

## Examples

**Scan default paths:**

```sh
rs-vst-host scan
```

**Scan with an extra plugin folder:**

```sh
rs-vst-host scan --paths ~/Downloads/VST3-Plugins
```

**List cached plugins:**

```sh
rs-vst-host list
```

**List audio output devices:**

```sh
rs-vst-host devices
```

**Run a plugin by name:**

```sh
rs-vst-host run "FabFilter Pro-Q 4"
```

**Run a plugin by path:**

```sh
rs-vst-host run /Library/Audio/Plug-Ins/VST3/MyPlugin.vst3
```

**Run with custom audio settings:**

```sh
rs-vst-host run "My Plugin" --sample-rate 48000 --buffer-size 512
```

**Run on a specific audio device:**

```sh
rs-vst-host run "My Plugin" --device "BlackHole 2ch"
```

**Run without the test tone (silence input):**

```sh
rs-vst-host run "My Plugin" --no-tone
```

**Run with MIDI input:**

```sh
rs-vst-host run "Surge XT" --midi "IAC Driver Bus 1"
```

**Run with parameter listing:**

```sh
rs-vst-host run "FabFilter Pro-Q 4" --list-params
```

**List MIDI input ports:**

```sh
rs-vst-host midi-ports
```

**Scan with debug output:**

```sh
RUST_LOG=debug rs-vst-host scan
```

**Run with debug logging (shows VST3 lifecycle details):**

```sh
RUST_LOG=debug rs-vst-host run "My Plugin"
```

**Show help:**

```sh
rs-vst-host --help
rs-vst-host run --help
```

---

## Troubleshooting

### "No plugin cache found. Run 'scan' first."

You need to run `rs-vst-host scan` before `list` or `run` (by name) will work. The cache file is created by `scan`.

### "No VST3 plugins found."

- Verify that `.vst3` bundles exist in one of the [default search paths](#plugin-search-paths).
- Use `--paths` to point to a custom directory if your plugins are installed elsewhere.
- Run with `RUST_LOG=debug` to see which directories are being checked and whether they exist.

### A plugin shows "load error"

- The plugin's binary may not be compatible with your CPU architecture (e.g., x86_64 plugin on an ARM Mac).
- The `.vst3` bundle may be corrupted or incomplete.
- Check debug logs (`RUST_LOG=debug`) for the specific error message.

### A plugin shows "metadata error"

- The plugin loaded successfully but its factory did not return valid metadata.
- This is uncommon; check debug logs for details and consider reporting the issue.

### "No audio output device available"

- Make sure your system has an audio output device connected and enabled.
- Use `rs-vst-host devices` to see what devices are available.
- On headless systems, consider installing a virtual audio device.

### Plugin fails to initialize during `run`

- Some plugins require additional host interfaces not yet implemented.
- If you see "QueryInterface for IAudioProcessor failed", ensure you're running the latest version — this was caused by an IID constant typo fixed in v0.6.0.
- Try running with `RUST_LOG=debug` to see the exact failure point.
- Report the plugin name and error message as an issue.

### Audio glitches or dropouts

- Try increasing the buffer size: `--buffer-size 1024` or `--buffer-size 2048`.
- Close other audio-intensive applications.
- Use a dedicated audio device if available.

### Cache seems stale

Re-run `rs-vst-host scan` to refresh the cache. The `list` command always shows the timestamp of the last scan so you can tell when it was generated.
