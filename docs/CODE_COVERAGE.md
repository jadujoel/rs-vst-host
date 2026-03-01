# Code Coverage Report

Last updated: 2026-03-01 (v0.26.2 — Preset Loading & Layout Fix).

## Summary

- **Total tests:** 920 (910 unit + integration)
- **All passing:** ✅ (0 failures)
- **Build warnings:** 0 (dead code warnings from unused graph/perf functions — expected until wired into runtime)
- **Test stability:** Verified
- **Last test run:** 2026-03-01 (920 tests passing, 0 failures, 0 ignored) — Preset Loading & Layout Fix
- **Miri coverage:** 21 miri_tests pass (all migrated to vst3-rs types)
- **ASan coverage:** 671 tests pass under AddressSanitizer (16 skipped: signal/malloc_zone/sigaction conflicts)
- **E2E coverage:** 39 tests pass with real FabFilter VST3 plugins (0 ignored — 6 crash-resilience tests use subprocess isolation, 10 multi-plugin lifecycle tests)
- **GUI integration tests:** 6 headless GUI tests with 12 PNG screenshots saved to `target/test-screenshots/`

## Test Coverage by Module

| Module | Tests | Coverage Level | Notes |
|--------|------:|---------------|-------|
| `src/vst3/com.rs` | 29 | ✅ Full | Struct layouts, IIDs, Event construction, parameter flags, speaker arrangements, UUID-to-bytes verification for all 10 IIDs including IPlugView/IPlugFrame, ViewRect, platform types |
| `src/error.rs` | 20 | ✅ Full | Display formatting, From conversions, Debug for all 4 error types |
| `src/vst3/process.rs` | 20 | ✅ Full | Buffer creation, interleaving, edge cases, setter methods, zero-channel configs |
| `src/midi/translate.rs` | 18 | ✅ Full | Note On/Off, channels, pitches, velocity range, batch, truncation, unsupported |
| `src/vst3/param_changes.rs` | 16 | ✅ Full | COM vtable ops, queue overflow (MAX_PARAM_QUEUES/MAX_POINTS_PER_PARAM), QI, null safety |
| `src/vst3/params.rs` | 16 | ⚠️ Partial | Utility functions (utf16, truncate) + ParameterEntry types, refresh_values null safety + empty params; from_controller requires live plugin |
| `src/vst3/event_list.rs` | 14 | ✅ Full | COM vtable, add/get/clear, overflow (MAX_EVENTS_PER_BLOCK), null pointers, QI |
| `src/app/cli.rs` | 22 | ✅ Full | Parse all subcommands including `gui`, `gui --safe-mode`, `gui --malloc-debug`, `gui --paths`, `audio-worker`, `audio-worker` with flags and paths, `scan --paths` exclusive mode, required/optional args, invalid input rejection |
| `src/app/interactive.rs` | 13 | ⚠️ Partial | State creation, all commands with no-params paths, handler polling; run_interactive requires stdin |
| `src/vst3/host_context.rs` | 13 | ✅ Full | Create/destroy, QI for all IIDs, ref counting, get_name, null safety, system heap verification |\n| `src/vst3/host_alloc.rs` | 8 | ✅ Full | system_alloc/system_free lifecycle, null safety, system malloc zone verification (macOS), drop semantics, alignment, stress test (100 allocs), Box-is-not-system-zone (mimalloc validation) |
| `src/vst3/component_handler.rs` | 13 | ✅ Full | COM vtable (vst3-rs types), perform_edit, restart flags, ref counting, concurrent access, null destroy, system heap verification |
| `src/gui/undo.rs` | 35 | ✅ Full | UndoableAction descriptions (7 variants), inverse operations (7 variants), basic stack ops (push/undo/redo/clear), max depth eviction, parameter coalescing (same param, different param, different slot, timeout, interleaved non-param, multiple coalesces), redo invalidation, history descriptions, mixed action sequences, config validation, edge cases (double undo/redo) |
| `src/audio/graph_engine.rs` | 15 | ✅ Full | Empty graph, serial passthrough, two-plugin serial, bypass, parallel split/mix, plugin failure, zero channels, mono, three-plugin serial, buffer operations (from_interleaved, to_interleaved, mix_add, copy_from, scale, silence) |
| `src/audio/perf.rs` | 18 | ✅ Full | SPSC ring buffer (new, push/pop, FIFO order, drain, producer/consumer threads, wrap-around, capacity rounding, minimum capacity), ParamChangeEntry, XrunTracker (no xrun, normal timing, reset, count accumulation), CpuLoadMonitor (initial zero, measurement, peak tracking, no-start safety), thread priority no-panic |
| `src/audio/delay_line.rs` | 12 | ✅ Full | DelayLine zero delay, single sample, multi-sample, wrap-around, block processing, reset, clamp, minimum capacity, zero-delay block. StereoDelayLine (set_delay, process, reset) |
| `src/gui/editor.rs` | 10 | ⚠️ Partial | Platform constants (NSView, HWND, X11), struct size, result code, sandbox accessibility, unattached close, pump_platform_events no-panic, NSApplication idempotent (macOS); actual editor open/close requires live plugin + display |
| `src/gui/app.rs` | 77 | ✅ Full | TransportState default, HostApp default, safe mode, malloc_debug mode, heap corruption detection, param filter, transport sync, editor open, audio status, rack add/remove, selected slot adjustment, filtered_classes by name/vendor/subcategory/factory_vendor, bypass toggle, status messages, session save/load roundtrip, bottom tab enum, activation/deactivation, param refresh, tone default, param cache/staging, selection state transitions, inactive param display, cache reorder, transient field isolation, undo stack initially empty, add/remove create undo entries, undo/redo add/remove operations, multi-operation undo/redo, redo cleared by new action, no-op undo/redo on empty stack, status messages for undo/redo, preset load from file, preset load nonexistent file, backend refresh_param_values no-op, close button clears selection |
| `src/gui/backend.rs` | 53 | ⚠️ Partial | Backend construction, device enumeration, parameter snapshots (empty), set_parameter (no active), handler changes (empty), tone control, device selection, editor count, active_has_editor, poll/close editors, set_tempo/playing/time_signature, open_editor, audio status, module-lifetime invariant, deactivate audio status, deactivate idempotency, stream option type, tainted paths (initially empty, blocks activation, non-tainted not blocked, bypassed in sandboxed mode), DEACTIVATION_CRASHED flag, deactivation without crash does not taint, heap corruption flag, process_isolation flag (default false, can be set), sandboxed state initially none, param_value_string sandboxed none, sandbox-wrapped deactivation, refresh_param_values no-op, set/get component/controller state without active plugin; activation requires real .vst3 plugins |
| `src/gui/theme.rs` | 18 | ✅ Full | Colour palette validation, corner radius uniformity, shadow values, frame construction, theme apply, opaque panel fill, semantic colour distinctness, accent button frame, bottom bar frame, input frame, badge background, secondary background, warm accent, widget visibility, card shadow |
| `src/ipc/messages.rs` | 18 | ✅ Full | Serialization roundtrip (all HostMessage/WorkerResponse variants), encode/decode wire protocol, length-prefix framing, oversized message rejection (16 MB limit), empty stream handling, MidiEvent/ParamChange/TransportState serde |
| `src/ipc/shm.rs` | 12 | ✅ Full | Create/open shared memory, input/output channel access, read/write audio data, header layout, sample count, ready flag, channel count validation, POSIX cleanup (`shm_unlink`) |
| `src/ipc/worker.rs` | 12 | ⚠️ Partial | WorkerState creation, all message handlers (load/configure/activate/deactivate/process/set_parameter/query_parameters/get_state/set_state/has_editor/shutdown/ping) tested without real plugins; full integration requires actual VST3 bundles |
| `src/ipc/proxy.rs` | 6 | ⚠️ Partial | TransportState default, read_output_interleaved (no shm), process silence (shutdown), pending_param_queue, dummy process construction; spawn() requires child process + real plugins |
| `src/vst3/sandbox.rs` | 28 | ✅ Full | SandboxResult methods (is_ok, is_crashed, is_panicked, ok, unwrap), PluginCrash Display and Error (incl. backtrace/heap_corrupted fields), signal name lookup, panic message extraction (str, String, other), normal/unit/side-effect calls, panic recovery, nested calls, nested inner panic, signal recovery (SIGBUS, SIGSEGV, SIGABRT via raise()), crash-then-normal recovery cycle, handler refcount cleanup, backtrace capture/symbolication, heap integrity check, crash display with frames |
| `src/diagnostics.rs` | 9 | ✅ Full | heap_check returns bool, check_malloc_env detection, recommended_env_vars non-empty, print_malloc_debug_instructions output, init_profiler/shutdown_profiler (feature-gated), malloc env not set by default, active_allocator_name, global allocator smoke test |
| `src/vst3/plug_frame.rs` | 13 | ✅ Full | HostPlugFrame creation, as_ptr, pending resize, QI for IPlugFrame/FUnknown/unknown IID, ref counting add/release, destroy, resize_view, release-does-not-self-destruct regression, editor close lifecycle regression, system heap verification |
| `src/vst3/types.rs` | 10 | ✅ Full | Serde roundtrip, optional fields, CID serialization, Debug, Clone |
| `src/miri_tests.rs` | 21 | ✅ Full | Miri-targeted: COM vtable lifecycle, event byte roundtrip, ProcessBuffers pointer chain, MIDI→ProcessData integration, `Send` safety, lifecycle stress |
| `src/asan_tests.rs` | 46 | ✅ Full | ASan-targeted: host_alloc lifecycle, COM object lifecycle, ProcessBuffers, shared memory, event bytes, MIDI→ProcessData, sandbox non-crash, IPC messages, concurrent COM, full mock process |
| `src/vst3/scanner.rs` | 10 | ✅ Full | Default paths, discover/dedup/sort, recursive scan, non-vst3 filtering, bundle resolution |
| `src/vst3/process_context.rs` | 10 | ✅ Full | Transport, tempo, time sig, advance, bar position, state flags |
| `src/vst3/cache.rs` | 9 | ✅ Full | Epoch date math, serde roundtrip, save/load roundtrip, corrupt JSON, timestamp format |
| `src/gui/session.rs` | 19 | ✅ Full | Capture, restore, serde roundtrip, file roundtrip, empty rack, invalid JSON, missing file, sessions_dir, version constant (v2.0), CID preservation, capture with state blobs, state serde roundtrip, state file roundtrip, v1 backward compat, encode/decode helpers, large state blob (1 MB), mixed slots with/without state |
| `src/midi/device.rs` | 7 | ⚠️ Partial | MidiReceiver push/drain/pending; MidiDevice needs hardware |
| `src/vst3/instance.rs` | 21 | ⚠️ Partial | IID constants, IConnectionPoint vtable layout, factory vtable size, LAST_DROP_CRASHED thread-local flag (default/set/reset, set on crash, not set on success), DEACTIVATION_CRASHED flag (default, set/read, independence from LAST_DROP_CRASHED), DEACTIVATION_HEAP_CORRUPTED flag, host object leak on crash (prevents use-after-free), host object destroy on clean shutdown, crash flags set together on COM crash; create_editor_view/has_editor require real COM objects |
| `src/vst3/module.rs` | 16 | ⚠️ Partial | UTF-8/UTF-16 utilities, vtable layout assertions (Factory/Factory2/Factory3 slot counts, RawClassInfoW size), IPluginFactory2/3 IID UUID verification, module-drop crash flag read-and-reset, full crash→flag→skip integration; module loading requires real .vst3 bundles |
| `src/e2e_tests.rs` | 39 | ✅ Full | E2E tests with real FabFilter Pro-MB and Pro-Q 4 plugins: discovery, metadata, instance, bus config, process lifecycle, multi-block, silence/signal, context, events, params, component handler, latency, sample rates, block sizes, interleaved I/O, AudioEngine, scan-cache pipeline. 6 crash-resilience tests use subprocess isolation with permanent SIGABRT handler (0 ignored). 10 multi-plugin lifecycle tests: forward/reverse shutdown, interleaved setup, stop-and-restart, duplicate instances, deterministic random ordering (seeds 42/1337), random start/stop cycles, concurrent AudioEngine, rapid add/remove stress. |
| `src/audio/engine.rs` | 8+4 | ✅ Full | TestToneGenerator (basic, disabled, fill_buffer, custom_params, phase_wrap, zero_amplitude_disabled), shutdown flag (initial state, cross-thread propagation); E2E: AudioEngine with real plugins (Pro-Q 4 tone on/off, Pro-MB engine) |
| `src/gui/ipc.rs` | 21 | ✅ Full | GuiAction serde roundtrip (all 24 variants), SupervisorUpdate roundtrip (all 14 variants), AudioCommand serde roundtrip (all 4 variants), encode/decode wire protocol, DecodeError timeout classification, CapturePluginState/LoadPreset/SavePreset/ListPresets serde, PluginStateCaptured/PresetList serde, RackSlotState with/without state blobs, backward compat (no state fields), PresetInfo serde, state captured encode/decode |
| `src/gui/supervisor.rs` | 12 | ✅ Full | ShadowState (new, update_from FullState/RackUpdated/PluginModulesUpdated, ignores others, to_restore_command), LoopResult variants, AudioCommand encode/decode, RestoreState roundtrip, AudioProcessRestarted roundtrip, check_gui_exit (clean shutdown, nonzero exit, waits for delayed exit); full supervisor loop requires child processes |
| `src/gui/audio_worker.rs` | 25 | ✅ Full | AudioWorkerState (safe_mode, normal), audio_status_state conversion, build_full_state structure, handle_action dispatch (ping, shutdown, set_tone, add_to_rack, remove_from_rack, select_slot, stage_parameter, set_transport, add_invalid_index, refresh_devices, capture_plugin_state, capture_invalid_index, list_presets_empty, load_preset_missing, save_preset_no_active), AudioCommand serialize roundtrip, state blob preservation, new variant serialize |
| `src/gui/gui_worker.rs` | 18 | ✅ Full | Default state, apply_full_state, incremental updates (status, heap corruption, editor availability, audio status, audio process restarted), rack update, params update, devices update, filtered_classes (empty, with modules, search), transport change detection, send_action to paired socket, send_shutdown_action_on_close, supervisor disconnect (default false, mark disconnected, idempotent, send_action noop when disconnected, broken pipe detection, poll_updates EOF detection, poll_updates noop when disconnected) |
| `src/gui/editor.rs` | 9 | ⚠️ Partial | Platform constant, struct size, result constant, sandbox import, NSApplication init idempotency, pump_events main-thread guard, pump_platform_events no-panic; open/close/poll require real NSWindow + IPlugView |
| `src/gui/routing.rs` | 5 | ✅ Full | Routing overview smoke test, editor smoke test, empty graph, bezier points correctness, node center calculation |
| `src/gui_tests.rs` | 6 | ✅ Full | Headless GUI integration: add plugin to rack with screenshot, open editor view and verify visible, full editor workflow (add→select→switch→deselect) with 9 screenshots, parameter types (automatable/bypass/read-only), multi-frame stability (10 frames), editor open without active plugin. CPU software-rasterized PNG screenshots saved to `target/test-screenshots/`. |
| `src/vst3/ibstream.rs` | 6 | ✅ Full | IBStream COM implementation (vst3-rs types): create/destroy, write/read roundtrip, seek/tell, from_data, take_data, query_interface, ref counting |
| `src/vst3/presets.rs` | 12 | ✅ Full | Preset serde roundtrip for binary state, no-state preset, base64 encoding, file I/O roundtrip, invalid JSON, missing file, filename sanitization, presets_dir, empty listing, listing with files, backward compat, large state (1 MB) |
| `src/vst3/cf_bundle.rs` | 3 | ⚠️ Partial | Null path handling, null release safety, system framework validation; full testing requires .vst3 bundles |
| `src/audio/device.rs` | 3 | ⚠️ Partial | Device enumeration (hardware-dependent); stream building untestable in CI |
| `src/audio/graph.rs` | 27 | ✅ Full | New/empty graph, add/remove nodes, connect/disconnect edges, cycle detection (direct and indirect), topological sort (empty/single/serial/parallel), serial chain construction from slots, insert/remove in chain, slot index adjustment after remove, predecessors/successors, rebuild serial chain from new slots, graph error display, serde roundtrip, parallel routing with split/mix nodes |

## Coverage Analysis

### Fully Tested (✅) — 26 modules
All public APIs and edge cases covered by unit tests. COM vtable methods tested through both direct API and vtable function pointer calls. IID constants verified against canonical UUID strings.

### Partially Tested (⚠️) — 12 modules
These modules have tests for pure-logic components but cannot be fully unit-tested because they depend on:
- **Live VST3 plugins** (`instance.rs`, `module.rs`, `params.rs from_controller`, `ipc/worker.rs` full integration)
- **Audio hardware** (`audio/device.rs`, `audio/engine.rs`)
- **MIDI hardware** (`midi/device.rs`)
- **Interactive stdin** (`interactive.rs run_interactive`)
- **CoreFoundation / .vst3 bundles** (`cf_bundle.rs` full path)
- **Native GUI / ObjC runtime** (`gui/editor.rs` open/close/poll)
- **Plugin editor views / IPlugView** (`gui/backend.rs` full activation)
- **Child process spawning** (`ipc/proxy.rs` spawn, requires running host binary)

### Not Testable in CI (❌) — 1 module
- `app/commands.rs` — Heavy I/O orchestration requiring both plugins and hardware

### Estimated Line Coverage
Based on module-level analysis:
- **Pure logic modules:** ~95% line coverage (all testable paths exercised)
- **Hardware-dependent modules:** ~40-60% (utility functions tested, I/O paths require integration testing)
- **Overall estimated:** ~80-85% of testable code

## v0.17.2 Test Additions (AddressSanitizer)

46 new tests added (533 → 579 total):

| Area | New Tests | Description |
|------|----------|-------------|
| ASan host_alloc | 7 | system_alloc/system_free lifecycle, null safety, varying sizes, concurrent threads, rapid cycle stress, drop semantics |
| ASan COM objects | 5 | HostApplication, HostComponentHandler, HostPlugFrame create→use→destroy, rapid create/destroy |
| ASan ProcessBuffers | 5 | Full pointer chain, varying block sizes, cross-thread transfer, zero channels, interleave roundtrip |
| ASan shared memory | 5 | Create/write/read, boundary writes, host↔worker roundtrip, zero channels, rapid create/destroy |
| ASan events | 3 | Note on/off byte-level roundtrip, event clone safety |
| ASan MIDI pipeline | 3 | Batch translate, all 16 channels, full MIDI→ProcessData pipeline |
| ASan sandbox | 6 | Normal call, heap alloc, system_alloc, panic recovery, nested calls, sequential stress |
| ASan IPC | 1 | Encode/decode roundtrip for all message variants |
| ASan full process | 2 | All COM objects wired into ProcessData, multi-block session |
| ASan concurrency | 2 | Multi-threaded handler edits (COM vtable), concurrent object create/destroy |
| ASan zone check | 1 | system_alloc pointer validation under ASan |

All 46 tests also pass under AddressSanitizer when run with:
```bash
RUSTFLAGS="-Z sanitizer=address" cargo +nightly test --target aarch64-apple-darwin --lib -- asan_tests
```

## v0.17.1 Test Additions (Miri Dynamic Analysis)

21 new tests added (512 → 533 total):

| Area | New Tests | Description |
|------|----------|-------------|
| Miri tests | 21 | COM vtable lifecycle (event list, param changes), event byte roundtrip (NoteOn/NoteOff), uninitialized memory check, extreme values, capacity stress (512 events), ProcessBuffers pointer chain, prepare stability, interleave roundtrip, zero/asymmetric channels, ProcessContext in ProcessData, MIDI→ProcessData integration, full mock process call, `Send` across threads, COM lifecycle stress (50 cycles), event clone, null destroy safety |

**Miri coverage:** 109 tests (from 7 modules) pass under `MIRIFLAGS="-Zmiri-tree-borrows" cargo +nightly miri test --lib`. See `DYNAMIC_ANALYSIS.md` for details.

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

## v0.14.1 Test Additions (Heap Corruption Fix)

4 new tests added (437 → 441 total):

| Area | New Tests | Description |
|------|----------|-------------|
| VST3 instance | 4 | Host objects leaked on crash (prevents use-after-free), host objects destroyed on clean shutdown, DEACTIVATION_HEAP_CORRUPTED flag, crash flags set together on COM crash (SIGBUS → LAST_DROP_CRASHED + DEACTIVATION_CRASHED + heap check) |

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

## Performance Benchmarks (Divan)

11 benchmark suites with ~130+ individual benchmarks. Run with `cargo bench`.

| Benchmark File | Target Module | Benchmarks | Description |
|---------------|--------------|:----------:|-------------|
| `benches/audio_engine.rs` | `audio/engine.rs` | ~15 | Tone generation, buffer fill at 44.1/96 kHz, sustained multi-block |
| `benches/process_buffers.rs` | `vst3/process.rs` | ~12 | Buffer creation, prepare, interleave/deinterleave, full cycle |
| `benches/event_list.rs` | `vst3/event_list.rs` | ~10 | Event add/clear, COM vtable get_event_count/get_event/add_event |
| `benches/param_changes.rs` | `vst3/param_changes.rs` | ~12 | Single/multi-param, worst-case linear scan, block cycle |
| `benches/midi_translate.rs` | `midi/translate.rs` | ~12 | Single MIDI events, batch 4–256, receiver push/drain |
| `benches/ipc_messages.rs` | `ipc/messages.rs` | ~14 | Encode/decode all message types, param lists, roundtrip |
| `benches/process_context.rs` | `vst3/process_context.rs` | ~12 | Advance, transport, tempo, time signature, full block update |
| `benches/host_alloc.rs` | `vst3/host_alloc.rs` | ~10 | system_alloc vs Box (mimalloc), small/medium/large, batch |
| `benches/diagnostics.rs` | `diagnostics.rs` | ~8 | heap_check, malloc env, allocator name, recommended vars |
| `benches/session_serde.rs` | `gui/session.rs` | ~10 | Session capture/restore/serde roundtrip (1–16 rack slots) |
| `benches/cache_serde.rs` | `vst3/cache.rs` | ~12 | Class/module/cache serde, roundtrip (4–64 modules) |

See [PERFORMANCE_CHANGELOG.md](PERFORMANCE_CHANGELOG.md) for baseline results.
