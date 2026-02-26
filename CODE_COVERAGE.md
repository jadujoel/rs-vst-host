# Code Coverage Report

Last updated: 2026-02-26 (v0.14.0 — debug & profiling infrastructure).

## Summary

- **Total tests:** 437 (438 with `--features debug-tools`)
- **All passing:** ✅
- **Build warnings:** 0
- **Test stability:** Verified (multiple consecutive clean runs)
- **Last test run:** 2026-02-26 (437 tests, 0 warnings; 438 with debug-tools)

## Test Coverage by Module

| Module | Tests | Coverage Level | Notes |
|--------|------:|---------------|-------|
| `src/vst3/com.rs` | 29 | ✅ Full | Struct layouts, IIDs, Event construction, parameter flags, speaker arrangements, UUID-to-bytes verification for all 10 IIDs including IPlugView/IPlugFrame, ViewRect, platform types |
| `src/error.rs` | 20 | ✅ Full | Display formatting, From conversions, Debug for all 4 error types |
| `src/vst3/process.rs` | 20 | ✅ Full | Buffer creation, interleaving, edge cases, setter methods, zero-channel configs |
| `src/midi/translate.rs` | 18 | ✅ Full | Note On/Off, channels, pitches, velocity range, batch, truncation, unsupported |
| `src/vst3/param_changes.rs` | 16 | ✅ Full | COM vtable ops, queue overflow (MAX_PARAM_QUEUES/MAX_POINTS_PER_PARAM), QI, null safety |
| `src/vst3/params.rs` | 14 | ⚠️ Partial | Utility functions (utf16, truncate) + ParameterEntry types; from_controller requires live plugin |
| `src/vst3/event_list.rs` | 14 | ✅ Full | COM vtable, add/get/clear, overflow (MAX_EVENTS_PER_BLOCK), null pointers, QI |
| `src/app/cli.rs` | 16 | ✅ Full | Parse all subcommands including `gui`, `gui --safe-mode`, `gui --malloc-debug`, required/optional args, invalid input rejection |
| `src/app/interactive.rs` | 13 | ⚠️ Partial | State creation, all commands with no-params paths, handler polling; run_interactive requires stdin |
| `src/vst3/host_context.rs` | 12 | ✅ Full | Create/destroy, QI for all IIDs, ref counting, get_name, null safety |
| `src/vst3/component_handler.rs` | 12 | ✅ Full | COM vtable, perform_edit, restart flags, ref counting, concurrent access, null destroy |
| `src/gui/app.rs` | 60 | ✅ Full | TransportState default, HostApp default, safe mode, malloc_debug mode, heap corruption detection, param filter, transport sync, editor open, audio status, rack add/remove, selected slot adjustment, filtered_classes by name/vendor/subcategory/factory_vendor, bypass toggle, status messages, session save/load roundtrip, bottom tab enum, activation/deactivation, param refresh, tone default, param cache/staging, selection state transitions, inactive param display, cache reorder, transient field isolation |
| `src/gui/backend.rs` | 36 | ⚠️ Partial | Backend construction, device enumeration, parameter snapshots (empty), set_parameter (no active), handler changes (empty), tone control, device selection, editor count, active_has_editor, poll/close editors, set_tempo/playing/time_signature, open_editor, audio status, module-lifetime invariant, deactivate audio status, deactivate idempotency, stream option type, tainted paths (initially empty, blocks activation, non-tainted not blocked), DEACTIVATION_CRASHED flag, deactivation without crash does not taint, heap corruption flag (default false, set on deactivation crash, propagated from thread-local); activation requires real .vst3 plugins |
| `src/gui/theme.rs` | 11 | ✅ Full | Colour palette validation, corner radius uniformity, shadow values, frame construction, theme apply, translucency, semantic colour distinctness |
| `src/vst3/sandbox.rs` | 28 | ✅ Full | SandboxResult methods (is_ok, is_crashed, is_panicked, ok, unwrap), PluginCrash Display and Error (incl. backtrace/heap_corrupted fields), signal name lookup, panic message extraction (str, String, other), normal/unit/side-effect calls, panic recovery, nested calls, nested inner panic, signal recovery (SIGBUS, SIGSEGV, SIGABRT via raise()), crash-then-normal recovery cycle, handler refcount cleanup, backtrace capture/symbolication, heap integrity check, crash display with frames |
| `src/diagnostics.rs` | 7 | ✅ Full | heap_check returns bool, check_malloc_env detection, recommended_env_vars non-empty, print_malloc_debug_instructions output, init_profiler/shutdown_profiler (feature-gated), malloc env not set by default |
| `src/vst3/plug_frame.rs` | 10 | ✅ Full | HostPlugFrame creation, as_ptr, pending resize, QI for IPlugFrame/FUnknown/unknown IID, ref counting add/release, destroy, resize_view |
| `src/vst3/types.rs` | 10 | ✅ Full | Serde roundtrip, optional fields, CID serialization, Debug, Clone |
| `src/vst3/scanner.rs` | 10 | ✅ Full | Default paths, discover/dedup/sort, recursive scan, non-vst3 filtering, bundle resolution |
| `src/vst3/process_context.rs` | 10 | ✅ Full | Transport, tempo, time sig, advance, bar position, state flags |
| `src/vst3/cache.rs` | 9 | ✅ Full | Epoch date math, serde roundtrip, save/load roundtrip, corrupt JSON, timestamp format |
| `src/gui/session.rs` | 9 | ✅ Full | Capture, restore, serde roundtrip, file roundtrip, empty rack, invalid JSON, missing file, sessions_dir, version constant, CID preservation |
| `src/midi/device.rs` | 7 | ⚠️ Partial | MidiReceiver push/drain/pending; MidiDevice needs hardware |
| `src/vst3/instance.rs` | 15 | ⚠️ Partial | IID constants, IConnectionPoint vtable layout, factory vtable size, LAST_DROP_CRASHED thread-local flag (default/set/reset, set on crash, not set on success), DEACTIVATION_CRASHED flag (default, set/read, independence from LAST_DROP_CRASHED), DEACTIVATION_HEAP_CORRUPTED flag; create_editor_view/has_editor require real COM objects |
| `src/vst3/module.rs` | 9 | ⚠️ Partial | UTF-8 utilities, IPluginFactory2/3 IID UUID verification, module-drop crash flag read-and-reset, full crash→flag→skip integration; module loading requires real .vst3 bundles |
| `src/audio/engine.rs` | 6 | ⚠️ Partial | TestToneGenerator (basic, disabled, fill_buffer, custom_params, phase_wrap, zero_amplitude_disabled); AudioEngine requires live Vst3Instance |
| `src/gui/editor.rs` | 3 | ⚠️ Partial | Platform constant, struct size, result constant; open/close/poll require real NSWindow + IPlugView |
| `src/vst3/cf_bundle.rs` | 3 | ⚠️ Partial | Null path handling, null release safety, system framework validation; full testing requires .vst3 bundles |
| `src/audio/device.rs` | 3 | ⚠️ Partial | Device enumeration (hardware-dependent); stream building untestable in CI |

## Coverage Analysis

### Fully Tested (✅) — 20 modules
All public APIs and edge cases covered by unit tests. COM vtable methods tested through both direct API and vtable function pointer calls. IID constants verified against canonical UUID strings.

### Partially Tested (⚠️) — 10 modules
These modules have tests for pure-logic components but cannot be fully unit-tested because they depend on:
- **Live VST3 plugins** (`instance.rs`, `module.rs`, `params.rs from_controller`)
- **Audio hardware** (`audio/device.rs`, `audio/engine.rs`)
- **MIDI hardware** (`midi/device.rs`)
- **Interactive stdin** (`interactive.rs run_interactive`)
- **CoreFoundation / .vst3 bundles** (`cf_bundle.rs` full path)
- **Native GUI / ObjC runtime** (`gui/editor.rs` open/close/poll)
- **Plugin editor views / IPlugView** (`gui/backend.rs` full activation)

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

## v0.11.0 Test Additions (Editor Windows & PRD Features)

43 new tests added (304 → 347 total):

| Area | New Tests | Description |
|------|----------|-------------|
| GUI app | 12 | Safe mode constructor, param filter default, prev transport defaults, sync transport, open editor (no slot, no active), editor count, audio status default |
| GUI backend | 10 | Audio status default/initial, editor count, active_has_editor, poll/close editors empty, set_tempo/playing/time_signature no active, open_editor no active |
| VST3 plug_frame | 10 | Creation, as_ptr, pending resize, QI (IPlugFrame/FUnknown/unknown), add_ref/release, destroy, resize_view |
| VST3 com | 7 | IPlugView/IPlugFrame IID lengths, UUID verification, IPlugViewVtbl/IPlugFrameVtbl sizes, ViewRect width/height |
| GUI editor | 3 | Platform constant, struct size, result constant |
| CLI parsing | 1 | `gui --safe-mode` flag |

## v0.13.0 Test Additions (Plugin Crash Sandbox)

21 new tests added (368 → 389 total):

| Area | New Tests | Description |
|------|----------|-------------|
| VST3 sandbox | 21 | SandboxResult is_ok/is_crashed/is_panicked/ok, PluginCrash Display+Error, signal_name (known+unknown), panic_message (str/String/other), sandbox normal/unit/side-effect, panic recovery, nested calls, nested inner panic, catches raised SIGBUS, catches raised SIGSEGV, catches SIGABRT, recovery allows subsequent calls, handler refcount cleanup |

## v0.14.0 Test Additions (Debug & Profiling Infrastructure)

22 new tests added (415 → 437 total, 438 with `--features debug-tools`):

| Area | New Tests | Description |
|------|----------|-------------|
| Diagnostics | 7 | heap_check returns bool, check_malloc_env detection, recommended_env_vars non-empty, print instructions output, init/shutdown profiler (feature-gated), malloc env not set by default |
| VST3 sandbox | 7 | Backtrace capture in signal handler, symbolicate_crash_backtrace, check_heap_after_recovery, PluginCrash Display with backtrace frames, PluginCrash Display with heap corruption, crash struct field defaults |
| CLI parsing | 2 | `gui --malloc-debug`, `gui --safe-mode --malloc-debug` combined flags |
| GUI app | 4 | HostApp::new with malloc_debug, heap_check_counter initial value, heap_corruption_detected default, with_safe_mode delegates correctly |
| GUI backend | 3 | heap_corruption_detected default false, set on deactivation crash, propagated from DEACTIVATION_HEAP_CORRUPTED thread-local |

## v0.13.1 Test Additions (Crash-Safe Library Unload)

18 new tests added (389 → 407 total):

| Area | New Tests | Description |
|------|----------|-------------|
| VST3 instance | 5 | LAST_DROP_CRASHED thread-local default, set/reset, set on sandbox crash, not set on success, read-and-reset pattern |
| VST3 module | 3 | Module-side flag read-and-reset, crash→flag→skip integration, post-skip sandbox recovery |
| GUI app | 10 | Additional rack/param/session state management tests |

## v0.10.0 Test Additions (GUI Live Integration)

31 new tests added (273 → 304 total):

| Area | New Tests | Description |
|------|----------|-------------|
| GUI backend | 12 | Backend construction, no-active params/handler/set_parameter, deactivate-when-none, refresh devices, device selection, tone control, param snapshot clone/debug, param value string |
| GUI session | 9 | Capture, restore, serde roundtrip, file roundtrip, empty rack, invalid JSON, missing file, sessions_dir, version constant, CID preservation |
| GUI app | 10 | BottomTab default/variants, deactivate_active, activate_slot invalid, refresh_params no active, session_path default, save/load session roundtrip, load nonexistent, tone_enabled default |

## v0.9.0 Test Additions (GUI Skeleton)

31 new tests added (242 → 273 total):

| Area | New Tests | Description |
|------|----------|-------------|
| GUI theme | 11 | Colour palette validation, corner radius, shadow, frame construction, theme apply, translucency, semantic colour distinctness |
| GUI app | 19 | Transport default, HostApp default, add/remove rack slots, selected slot adjustment, filtered classes by name/vendor/subcategory/factory vendor, bypass toggle, status messages, multiple adds |
| CLI parsing | 1 | Parse `gui` subcommand |

## v0.8.0 Test Additions

No new tests added in this release (GUI Design Phase).

## v0.7.0 Test Additions

5 new tests added (237 → 242 total):

| Area | New Tests | Description |
|------|----------|-------------|
| COM IID verification | 1 | IConnectionPoint IID UUID-to-bytes validation |
| COM IID lengths | 1 | IConnectionPoint IID is 16 bytes |
| Instance vtable layouts | 2 | IConnectionPointVtbl (5 pointers), IPluginFactoryVtbl (7 pointers) size verification |
| Instance IID | 1 | IEditController IID is 16 bytes |

## v0.6.0 Test Additions

14 new tests added (223 → 237 total):

| Area | New Tests | Description |
|------|----------|-------------|
| COM IID verification | 9 | UUID-to-bytes validation for all 7 IIDs (IComponent, IAudioProcessor, IHostApplication, FUnknown, IEditController, IEventList, IParameterChanges) plus helper function tests |
| Module IID verification | 2 | UUID-to-bytes validation for IPluginFactory2 and IPluginFactory3 IIDs |
| CFBundleRef | 3 | Null path handling, null release safety, system framework (CoreFoundation) validation |
