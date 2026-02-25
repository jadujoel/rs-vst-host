# rs-vst-host User Guide

A minimal VST3 plugin host written in Rust. This tool lets you discover, inspect, and (in future releases) run VST3 audio plugins from the command line.

---

## Table of Contents

- [Requirements](#requirements)
- [Installation](#installation)
- [Quick Start](#quick-start)
- [Commands](#commands)
  - [scan](#scan)
  - [list](#list)
  - [run](#run)
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

3. **Run** a plugin (coming in a future release):

   ```sh
   rs-vst-host run "Plugin Name"
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

Loads a plugin and starts audio processing. **This command is planned for a future release** (Phase 3+) and is not yet functional.

```
rs-vst-host run <PLUGIN>
```

**Arguments:**

| Argument | Description |
|----------|-------------|
| `PLUGIN` | Plugin name (as shown in `list`) or path to a `.vst3` bundle |

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

**Scan with debug output:**

```sh
RUST_LOG=debug rs-vst-host scan
```

**Show help:**

```sh
rs-vst-host --help
rs-vst-host scan --help
```

---

## Troubleshooting

### "No plugin cache found. Run 'scan' first."

You need to run `rs-vst-host scan` before `list` will show anything. The `list` command reads from the cache file, which is created by `scan`.

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

### Cache seems stale

Re-run `rs-vst-host scan` to refresh the cache. The `list` command always shows the timestamp of the last scan so you can tell when it was generated.
