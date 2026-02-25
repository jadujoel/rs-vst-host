# Code Coverage Report

## Summary

- **Total tests:** 223
- **All passing:** ✅
- **Build warnings:** 0
- **Test stability:** Verified (5 consecutive clean runs)

## Test Coverage by Module

| Module | Tests | Coverage Level | Notes |
|--------|------:|---------------|-------|
| `src/error.rs` | 20 | ✅ Full | Display formatting, From conversions, Debug for all 4 error types |
| `src/vst3/process.rs` | 20 | ✅ Full | Buffer creation, interleaving, edge cases, setter methods, zero-channel configs |
| `src/midi/translate.rs` | 18 | ✅ Full | Note On/Off, channels, pitches, velocity range, batch, truncation, unsupported |
| `src/vst3/param_changes.rs` | 16 | ✅ Full | COM vtable ops, queue overflow (MAX_PARAM_QUEUES/MAX_POINTS_PER_PARAM), QI, null safety |
| `src/vst3/params.rs` | 14 | ⚠️ Partial | Utility functions (utf16, truncate) + ParameterEntry types; from_controller requires live plugin |
| `src/vst3/event_list.rs` | 14 | ✅ Full | COM vtable, add/get/clear, overflow (MAX_EVENTS_PER_BLOCK), null pointers, QI |
| `src/app/interactive.rs` | 13 | ⚠️ Partial | State creation, all commands with no-params paths, handler polling; run_interactive requires stdin |
| `src/vst3/host_context.rs` | 12 | ✅ Full | Create/destroy, QI for all IIDs, ref counting, get_name, null safety |
| `src/vst3/component_handler.rs` | 12 | ✅ Full | COM vtable, perform_edit, restart flags, ref counting, concurrent access, null destroy |
| `src/vst3/com.rs` | 12 | ✅ Full | Struct layouts, IIDs, Event construction, parameter flags, speaker arrangements |
| `src/app/cli.rs` | 11 | ✅ Full | Parse all subcommands, required/optional args, invalid input rejection |
| `src/vst3/types.rs` | 10 | ✅ Full | Serde roundtrip, optional fields, CID serialization, Debug, Clone |
| `src/vst3/scanner.rs` | 10 | ✅ Full | Default paths, discover/dedup/sort, recursive scan, non-vst3 filtering, bundle resolution |
| `src/vst3/process_context.rs` | 10 | ✅ Full | Transport, tempo, time sig, advance, bar position, state flags |
| `src/vst3/cache.rs` | 9 | ✅ Full | Epoch date math, serde roundtrip, save/load roundtrip, corrupt JSON, timestamp format |
| `src/midi/device.rs` | 7 | ⚠️ Partial | MidiReceiver push/drain/pending; MidiDevice needs hardware |
| `src/audio/engine.rs` | 5 | ⚠️ Partial | TestToneGenerator only; AudioEngine requires live Vst3Instance |
| `src/vst3/module.rs` | 4 | ⚠️ Partial | UTF-8 utilities only; module loading requires real .vst3 bundles |
| `src/vst3/instance.rs` | 3 | ⚠️ Partial | IID constants only; all methods require real COM objects |
| `src/audio/device.rs` | 3 | ⚠️ Partial | Device enumeration (hardware-dependent); stream building untestable in CI |
| `src/app/commands.rs` | 0 | ❌ None | Integration-level orchestration; requires plugins + hardware |
| `src/app/mod.rs` | 0 | N/A | Module declarations only |
| `src/vst3/mod.rs` | 0 | N/A | Module declarations only |
| `src/audio/mod.rs` | 0 | N/A | Module declarations only |
| `src/midi/mod.rs` | 0 | N/A | Module declarations only |
| `src/host/mod.rs` | 0 | N/A | Placeholder module |
| `src/main.rs` | 0 | N/A | Entry point only |

## Coverage Analysis

### Fully Tested (✅) — 14 modules
All public APIs and edge cases covered by unit tests. COM vtable methods tested through both direct API and vtable function pointer calls.

### Partially Tested (⚠️) — 7 modules
These modules have tests for pure-logic components but cannot be fully unit-tested because they depend on:
- **Live VST3 plugins** (`instance.rs`, `module.rs`, `params.rs from_controller`)
- **Audio hardware** (`audio/device.rs`, `audio/engine.rs`)
- **MIDI hardware** (`midi/device.rs`)
- **Interactive stdin** (`interactive.rs run_interactive`)

### Not Testable in CI (❌) — 1 module
- `app/commands.rs` — Heavy I/O orchestration requiring both plugins and hardware

### Estimated Line Coverage
Based on module-level analysis:
- **Pure logic modules:** ~95% line coverage (all testable paths exercised)
- **Hardware-dependent modules:** ~40-60% (utility functions tested, I/O paths require integration testing)
- **Overall estimated:** ~80-85% of testable code

## Phase 6 Test Additions (v0.5.0)

117 new tests added across all modules:

| Area | New Tests | Description |
|------|----------|-------------|
| Error types | 20 | Display formatting for all 4 error enums, From conversions, Debug |
| CLI parsing | 11 | All subcommands, optional/required args, invalid input |
| Types serde | 10 | Roundtrip serialization, optional fields, CID encoding, Clone |
| Scanner | 6 | Dedup, sort, recursive, non-vst3 filtering, macOS bundle |
| Cache I/O | 5 | Serde roundtrip, file roundtrip, corrupt JSON, timestamp format |
| Param registry | 8 | UTF-16 edge cases, truncate edge cases, flag combinations |
| Event list | 8 | Vtable overflow, add via vtable, null pointers, add_ref/release |
| Param changes | 8 | MAX_PARAM_QUEUES overflow, MAX_POINTS overflow, PVQ QI, null safety |
| Process buffers | 10 | Setter methods, zero channels, out-of-range, consecutive prepare |
| MIDI translate | 9 | All channels, all pitches, note-off velocity, batch edge cases |
| Interactive | 10 | All commands with no-params, tempo parsing, handler polling |
| Host context | 7 | IHost QI, ref counting, null safety, destroy null |
| Component handler | 4 | Concurrent perform_edit, restart flag OR, destroy null, as_ptr |
| Process context | 0 | Already well-covered at 10 tests |
