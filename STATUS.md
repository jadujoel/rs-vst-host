# Status

## Current Phase: Phase 2 — VST3 Plugin Discovery and Loading (Complete)

**Milestone M1 achieved**: Scanner + plugin metadata listing works.

### Completed

#### Phase 0 — Technical Decisions
- **CLI**: `clap` v4 with derive macros
- **Error handling**: `thiserror` v2 + `anyhow` v1
- **Logging**: `tracing` + `tracing-subscriber` with env-filter
- **Serialization**: `serde` + `serde_json` for plugin cache
- **Dynamic loading**: `libloading` v0.8 (manual COM FFI for VST3 factory access)
- **Platform dirs**: `dirs` v6
- VST3 interop approach: Manual COM vtable FFI for scanning; `vst3-sys` planned for Phase 3+
- Audio: `cpal` planned for Phase 3
- MIDI: `midir` planned for Phase 4

#### Phase 1 — Project Foundations
- Module structure: `app/`, `audio/`, `midi/`, `host/`, `vst3/`, `error.rs`
- Error types: `HostError`, `Vst3Error` with `thiserror`
- Logging: `tracing` with env-filter subscriber
- CLI: `scan`, `list`, `run` subcommands via `clap`

#### Phase 2 — VST3 Plugin Discovery and Loading
- **Scanner** (`vst3/scanner.rs`): Searches macOS/Linux/Windows standard VST3 paths, discovers `.vst3` bundles recursively, resolves platform-specific binary paths
- **Module loader** (`vst3/module.rs`): Dynamic loading via `libloading`, manual COM FFI for IPluginFactory and IPluginFactory2, platform-specific `bundleEntry`/`ModuleEntry` handling
- **Cache** (`vst3/cache.rs`): JSON-based plugin cache in platform data directory
- **CLI commands** (`app/commands.rs`): `scan` discovers+loads+caches, `list` displays cached plugins

### Test Results
- 12 unit tests passing (scanner, cache, module utilities)
- Successfully scans real VST3 plugins on macOS (tested with FabFilter Pro-MB, Pro-Q 4)
- IPluginFactory2 extended metadata (subcategories, vendor, version) retrieved correctly

### Documentation
- `USER_GUIDE.md` — end-user guide covering installation, all CLI commands, plugin search paths, cache details, logging, and troubleshooting

### Next Steps (Phase 3 — Audio Engine Integration)
- Add `cpal` for audio device setup
- Implement VST3 process setup negotiation (bus arrangements, sample rate, block size)
- Build real-time processing loop
- Implement `run` command to load plugin and process audio
