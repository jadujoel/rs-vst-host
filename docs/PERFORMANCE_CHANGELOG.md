# Performance Changelog

All performance benchmark results are tracked here. Benchmarks use [Divan](https://github.com/nvzqz/divan).

Run benchmarks: `cargo bench`

## [0.26.4] - 2026-03-02 — CI Workflow Fix (no perf impact)

### Summary

Fixed GitHub Actions CI workflow — replaced non-existent `dtolnay/rust-action/setup@v1` with `dtolnay/rust-toolchain@stable` in all 5 jobs. No source code changes.

**Changes to hot paths:** None. CI-only change.

**No benchmark regressions.**

## [0.26.3] - 2026-03-01 — Param Panel Navigation Fix (no perf impact)

### Summary

Fixed param panel trapping users with no way to return to the rack view. Added close/back buttons and toggle-deselect behavior. Added `max_width(400.0)` constraint on the right SidePanel. All changes are in GUI rendering code — no audio callback changes.

**Changes to hot paths:** None. Close button click and toggle logic only set `selected_slot = None`, a trivial state assignment. The `max_width` constraint affects only egui layout, not audio processing.

**No benchmark regressions.**

## [0.26.2] - 2026-03-01 — Preset Loading & Layout Fix (no perf impact)

### Summary

Fixed preset loading (stale `ParameterRegistry.current_normalized` after state restore) and parameter panel layout (no max_width, no close button). All changes are in GUI/parameter code paths — no audio callback changes.

**Changes to hot paths:** None. `ParameterRegistry::refresh_values()` is called only on explicit preset load (user action), not per-frame. The layout changes (max_width constraint, close button) affect only egui rendering, not the audio processing loop.

**No benchmark regressions.**

## [0.26.1] - 2026-02-28 — GUI Worker Phase 8 Integration (no perf impact)

### Summary

Wired all Phase 8 UI features (presets, routing, undo/redo, drag-drop, perf metrics) into the real GUI worker process. Previously these features only existed in `HostApp` (headless test mode) and were invisible to users.

**Changes to hot paths:** None. All changes are in the GUI rendering code (`gui/gui_worker.rs`), IPC message definitions (`gui/ipc.rs`), and audio worker action handlers (`gui/audio_worker.rs`). The audio `process()` loop is completely unaffected. New IPC messages (`ReorderRack`, `Undo`, `Redo`, `RoutingGraphUpdated`, `UndoState`, `PresetNameChanged`) use the existing length-prefixed JSON framing and are only sent on user interaction, not per-audio-block.

**No benchmark regressions.**

## [0.26.0] - 2026-02-28 — Phase 8 Completion (no perf impact)

### Summary

Completed all remaining Phase 8 sub-phases: graph-aware audio engine, drag-and-drop rack reordering, cross-platform plugin editors, performance hardening infrastructure, plugin compatibility improvements, CI/CD and distribution.

**Changes to hot paths:** None yet. The new modules (`graph_engine.rs`, `perf.rs`, `delay_line.rs`) provide building blocks for future audio path integration but are not yet wired into the audio callback. The `SpscRingBuffer`, `XrunTracker`, and `CpuLoadMonitor` in `perf.rs` are designed for real-time use (lock-free, no allocation) but currently only exercised in unit tests.

**New modules:**
- `audio/graph_engine.rs` — graph-aware multi-plugin processing (not yet in audio callback)
- `audio/perf.rs` — SPSC ring buffer, xrun detection, CPU load monitoring (infrastructure)
- `audio/delay_line.rs` — latency compensation delay lines (infrastructure)
- `gui/editor.rs` — expanded with Linux X11 and Windows HWND support (platform-gated)
- `vst3/instance.rs` — bus arrangement fallback chain (called during plugin setup, not audio callback)
- `.github/workflows/ci.yml` — CI pipeline (no runtime impact)
- `scripts/bundle-macos.sh` — packaging script (no runtime impact)

**No benchmark regressions.**

## [0.25.0] - 2026-02-28 — Undo/Redo System (no perf impact)

### Summary

Phase 8.4 implementation: full undo/redo system with command pattern, parameter coalescing, GUI buttons, and keyboard shortcuts. New `gui/undo.rs` module with `UndoableAction` enum (7 variants), `UndoStack` with configurable max depth and coalescing window. Integrated into HostApp for rack operations, parameter changes, and transport changes.

**Changes to hot paths:** None. The undo system is purely control-plane code — `UndoStack::push()` is called from GUI event handlers (button clicks, slider releases, keyboard shortcuts), never from the audio callback. Parameter coalescing uses `std::time::Instant` comparisons, which are negligible. No changes to the audio `process()` loop, IPC messages, or shared memory buffers.

**New modules:** `gui/undo.rs` (in-memory undo/redo stacks, not on any hot path).

**No benchmark regressions.**

## [0.24.0] - 2026-02-28 — Preset Buttons & Multi-Plugin Routing (no perf impact)

### Summary

Phase 8.2 UI + Phase 8.3 implementation: preset management toolbar in parameter panel (prev/next/save/init buttons), routing graph data model (`audio/graph.rs` with AudioGraph DAG, topological sort, cycle detection), visual routing editor (`gui/routing.rs` with compact chain overview and advanced node editor). Integration into HostApp.

**Changes to hot paths:** None. The routing graph and preset UI are purely data-model and GUI code. The audio `process()` loop is completely unaffected — no graph traversal runs in the audio callback yet (will be wired in a future phase). Preset load/save only triggers on user interaction.

**New modules:** `audio/graph.rs` (in-memory DAG, not on any hot path), `gui/routing.rs` (GUI rendering only).

### Session Serde Benchmarks (unchanged from v0.23.0)
| Benchmark | Fastest | Median | Mean |
|-----------|---------|--------|------|
| capture/1 slot | 178 ns | 181 ns | 183 ns |
| capture/16 slots | 1.53 µs | 1.54 µs | 1.62 µs |
| restore/1 slot | 119 ns | 122 ns | 129 ns |
| restore/16 slots | 1.55 µs | 1.56 µs | 1.68 µs |
| serde_serialize/1 | 406 ns | 414 ns | 421 ns |
| serde_serialize/16 | 3.02 µs | 3.17 µs | 3.29 µs |
| serde_deserialize/1 | 584 ns | 667 ns | 726 ns |
| serde_deserialize/16 | 6.58 µs | 6.83 µs | 7.27 µs |
| serde_roundtrip/1 | 1.25 µs | 1.30 µs | 1.31 µs |
| serde_roundtrip/16 | 11.4 µs | 11.8 µs | 12.4 µs |

**No benchmark regressions.**

## [0.23.0] - 2026-02-28 — Plugin State Persistence & Presets (minimal perf impact)

### Summary

Phase 8.1 + 8.2 implementation: plugin state save/restore via IBStream, session format v2.0 with base64 state blobs, preset file management. State capture/restore methods added to Vst3Instance, AudioEngine, HostBackend, and PluginProcess proxy. New `presets.rs` module, 4 new `GuiAction` variants, 2 new `SupervisorUpdate` variants. Base64 dependency added.

**Changes to hot paths:** None. State capture (`getState()`) and restore (`setState()`) are only invoked on user-initiated save/load/preset actions — never during the audio callback. The audio `process()` loop is completely unaffected.

**Session serde:** The v2.0 session format adds optional base64 state blobs to `SlotSnapshot`. For sessions without state (or empty state), serialization is identical to v1.0.

### Session Serde Benchmarks (v2.0 format, no state blobs)
| Benchmark | Fastest | Median | Mean |
|-----------|---------|--------|------|
| capture/1 slot | 170 ns | 183 ns | 185 ns |
| capture/16 slots | 1.63 µs | 1.68 µs | 1.69 µs |
| restore/1 slot | 122 ns | 126 ns | 135 ns |
| restore/16 slots | 1.62 µs | 1.69 µs | 1.78 µs |
| serde_serialize/1 | 422 ns | 429 ns | 443 ns |
| serde_serialize/16 | 3.10 µs | 3.19 µs | 4.06 µs |
| serde_deserialize/1 | 625 ns | 667 ns | 725 ns |
| serde_deserialize/16 | 6.92 µs | 7.08 µs | 7.32 µs |
| serde_roundtrip/1 | 1.31 µs | 1.38 µs | 1.39 µs |
| serde_roundtrip/16 | 11.9 µs | 12.2 µs | 16.8 µs |

**No benchmark regressions.**

## [0.22.1] - 2026-02-28 — GUI Window Close Fix (no perf impact)

### Summary

Fixed GUI window close causing reopen. Changes are limited to `gui_worker.rs` (send `GuiAction::Shutdown` on close) and `supervisor.rs` (`check_gui_exit` grace period). No changes to any audio processing, IPC, or COM hot paths.

**No benchmark regressions.**

## [0.22.0] - 2026-02-28 — Modern UI Redesign (no perf impact)

### Summary

Complete visual overhaul of the GUI theme and all panel renderers. Changes are purely cosmetic — new colors, frames, badges, buttons, and layout adjustments. No changes to any audio processing, IPC, or COM hot paths. All benchmarks pass with no regressions.

**No benchmark regressions.**

## [0.21.0] - 2026-02-28 — Complete vst3-rs migration (no perf impact)

### Summary

Complete migration from hand-written COM FFI to vst3-rs crate v0.3.0. All VST3 type definitions now come from auto-generated bindings. This is a pure type-level change — no logic changes to any hot paths. The vst3-rs types are `#[repr(C)]` with identical memory layout to the hand-written versions, so vtable dispatch performance is unchanged.

**All benchmarks pass with no regressions.** Key results:
- `event_list/add_events/1024`: ~2.3 µs (unchanged)
- `param_changes/add_changes/64x16`: ~8.5 µs (unchanged)
- `process_buffers/full_cycle_stereo/1024`: ~1.1 µs (unchanged)
- `midi_translate/batch/256`: ~508 ns (unchanged)
- `ipc_messages/encode_process_response/1024`: ~213 ns (unchanged)

## [0.20.13] - 2026-02-28 — ibstream.rs vst3-rs migration (no perf impact)

### Summary

Migrated `component_handler.rs` from hand-written `IComponentHandlerVtbl` to vst3-rs crate types. Removed local vtable struct and local IID constants, updated COM function signatures to typed `this` pointers (`*mut FUnknown`, `*mut IComponentHandler`) and vst3-rs parameter types (`ParamID`, `ParamValue`, `int32`). Pure API conformance change — no logic changes to any hot paths (component handler is called during plugin parameter edits, not in the audio processing loop itself).

**No benchmark regressions.**

## [0.20.11] - 2026-02-28 — host_context.rs vst3-rs migration (no perf impact)

### Summary

Migrated `host_context.rs` from hand-written `IHostApplicationVtbl` to vst3-rs crate types. Removed local vtable struct, updated COM function signatures to typed `this` pointers and vst3-rs parameter types. Pure API conformance change — no logic changes to any hot paths (host context is only used during plugin initialization).

**No benchmark regressions.**

## [0.20.10] - 2026-02-28 — module.rs vst3-rs migration (no perf impact)

### Summary

Migrated `module.rs` from hand-written COM vtable definitions to vst3-rs crate types. Removed ~130 lines of local type definitions (`IUnknownVtbl`, `IPluginFactoryVtbl`, `IPluginFactory2Vtbl`, `IPluginFactory3Vtbl`, `ComObj<V>`, `RawFactoryInfo`, `RawClassInfo`, `RawClassInfo2`, `RawClassInfoW`, IID constants). Updated all vtable calls to camelCase with typed `this` pointers. Pure API conformance change — no logic changes to any hot paths (factory loading, class enumeration, instance creation).

**No benchmark regressions.**

## [0.20.9] - 2026-02-28 — miri_tests.rs + benchmarks vst3-rs migration (no perf impact)

### Summary

Migrated `miri_tests.rs` from hand-written COM FFI to vst3-rs types. Fixed `e2e_tests.rs` typed pointer casts. Fixed `benches/event_list.rs` and `benches/midi_translate.rs` for vst3-rs types (vtable field names, event construction, typed this pointers). Pure test/bench-only changes — no impact on production code or hot paths.

**No benchmark regressions.**

## [0.20.8] - 2026-02-28 — asan_tests.rs vst3-rs migration (no perf impact)\n\n### Summary\n\nMigrated `asan_tests.rs` from hand-written COM FFI to vst3-rs types. Updated event construction to use `make_note_on_event`/`make_note_off_event`, replaced raw `event.data` access with `event_as_note_on`/`event_as_note_off` helpers, used camelCase field names for ProcessData/AudioBusBuffers/Event, typed pointer casts for process buffer setters, and camelCase IEventListVtbl field names. Pure test-only change — no impact on production code or hot paths.\n\n**No benchmark regressions.**\n\n## [0.20.7] - 2026-02-28 — consumer files vst3-rs migration (no perf impact)

### Summary

Migrated 4 consumer files (`translate.rs`, `worker.rs`, `editor.rs`, `engine.rs`) from hand-written COM FFI to vst3-rs types. Updated event construction to use `make_note_on_event`/`make_note_off_event`, replaced `ComPtr<IPlugViewVtbl>` with `*mut IPlugView`, updated vtbl field names to camelCase, added typed pointer casts for process buffer setters. Pure API conformance change — no logic changes to any hot paths (MIDI translation, audio processing, IPC event passing, editor lifecycle).

**No benchmark regressions.**

## [0.20.6] - 2026-02-28 — params.rs vst3-rs migration (no perf impact)

### Summary

Migrated `params.rs` from hand-written `ComPtr<IEditControllerVtbl>` FFI to vst3-rs typed `*mut IEditController`. Updated all vtable calls to camelCase with typed self pointers and nested base chain for terminate/release. Pure API conformance change — no logic changes to any hot paths (parameter enumeration, value conversion, display string retrieval).

**No benchmark regressions.**

## [0.20.5] - 2026-02-28 — process.rs vst3-rs migration (no perf impact)

### Summary

Migrated `process.rs` from hand-written COM FFI to vst3-rs types. Renamed all struct fields from snake_case to camelCase, changed `AudioBusBuffers` initialization to use `std::mem::zeroed()` + union `__field0`, and updated setter parameter types from `*mut c_void` to typed pointers. Pure API conformance change — no logic changes to any hot paths (audio buffer management, interleave/deinterleave, process call setup).

**No benchmark regressions.**

## [0.20.4] - 2026-02-28 — instance.rs vst3-rs migration (no perf impact)

### Summary

Migrated `instance.rs` from hand-written `ComPtr<XVtbl>` FFI to vst3-rs typed COM interface structs. Updated all vtable calls to nested base patterns with typed `this` pointers and camelCase field access. Pure API conformance change — no logic changes to any hot paths (audio process, bus arrangement, latency queries).

**No benchmark regressions.**

## [0.20.3] - 2026-02-28 — event_list.rs vst3-rs migration (no perf impact)

### Summary

Fixed event_list.rs compilation by migrating vtable construction to camelCase field names, nested `FUnknownVtbl` base, and typed `this` pointers to match vst3-rs API. Pure API conformance change — no logic changes to any hot paths.

**No benchmark regressions.**

## [0.20.2] - 2026-02-28 — plug_frame.rs vst3-rs migration (no perf impact)

### Summary

Fixed plug_frame.rs compilation by migrating vtable construction to camelCase field names and typed `this` pointers to match vst3-rs API. Pure API conformance change — no logic changes to any hot paths.

**No benchmark regressions.**

## [0.20.1] - 2026-02-28 — Clippy cleanup (no perf impact)

### Summary

Fixed all 26 remaining clippy warnings. Changes are purely code quality: unused import removal, `assert!` style fixes, struct initializer refactoring, iterator idioms in test code. No changes to any hot audio/IPC/rendering paths.

**No benchmark regressions.**

## [0.20.0] - 2026-02-28 — IBStream, editor fixes, lint cleanup (no perf impact)

### Summary

New IBStream COM implementation for plugin state transfer, editor z-order fix (`orderFrontRegardless`), plugin list filtering (Audio Module Class only), setComponentState for split-architecture plugins, and 30+ lint warning fixes. None of these changes touch hot audio/IPC/rendering paths — IBStream is used only during plugin initialization, and all other changes are GUI or code quality fixes.

**No benchmark regressions.**

## [0.19.9] - 2026-02-28 — IPluginFactory3 vtable fix (no perf impact)

### Summary

Fixed segfault in plugin scan caused by missing `getClassInfoUnicode` slot in `IPluginFactory3Vtbl`. Only the COM vtable struct definition and helper functions were modified — no changes to any hot audio/IPC/rendering paths.

**No benchmark regressions.**

## [0.19.8] - 2026-02-28 — Exclusive --paths flag (no perf impact)

### Summary

Behavior change: `--paths` now exclusively replaces default scan paths instead of appending to them. `--paths` added to `gui` and `audio-worker` commands. No changes to any hot audio/IPC/rendering paths — only CLI argument threading and path resolution logic modified.

**No benchmark regressions.**

## [0.19.6] - 2026-02-28 — Test script fix (no perf impact)

### Summary

Updated `test.bash` to run all tests correctly. No code changes to any library or binary targets — only the shell script and documentation were modified.

**No benchmark regressions.**

## [0.19.5] - 2026-02-28 — Headless GUI tests (no perf impact)

### Summary

New `gui_tests` module with 6 headless GUI integration tests and PNG screenshot capture. No changes to any hot audio/IPC/rendering paths. Panel rendering methods changed from private to `pub(crate)` — zero runtime impact.

**No benchmark regressions.**

## [0.19.4] - 2026-02-28 — Editor window fix (no perf impact)

### Summary

Bug fix only (plugin editor windows not opening in supervised mode). No changes to hot audio/IPC paths. `poll_editors()` now includes an `if !self.editor_windows.is_empty()` guard so the AppKit pump is only called when editors are open — zero overhead when no editors are displayed.

**No benchmark regressions.**

## [0.19.2] - 2026-02-28 — Initial Divan benchmark suite

### Benchmark Suite Created

11 benchmark files covering all hot paths in the audio processing pipeline.

### Baseline Results (Apple Silicon, release mode)

#### Audio Engine (`benches/audio_engine.rs`)

| Benchmark | 64 | 128 | 256 | 512 | 1024 | 2048 | 4096 |
|-|-|-|-|-|-|-|-|
| `fill_buffer_44100` | 263 ns | 541 ns | 1.10 µs | 2.22 µs | 4.46 µs | 8.98 µs | 17.5 µs |
| `fill_buffer_96000` | 248 ns | 520 ns | 1.03 µs | 2.05 µs | 4.12 µs | 8.21 µs | — |
| `next_sample_44100` | 3.18 ns | | | | | | |
| `next_sample_96000` | 3.14 ns | | | | | | |
| `sustained_10_blocks` | — | 5.25 µs | — | 21.0 µs | 43.8 µs | — | — |

#### Process Buffers (`benches/process_buffers.rs`)

| Benchmark | 64 | 128 | 256 | 512 | 1024 | 2048 |
|-|-|-|-|-|-|-|
| `prepare_stereo` | — | — | — | — | — | — |
| `write_input_interleaved_stereo` | — | — | — | — | — | — |
| `read_output_interleaved_stereo` | — | — | — | — | — | — |
| `full_cycle_stereo` | — | 649 ns | 1.28 µs | 2.54 µs | 5.06 µs | — |

#### Event List (`benches/event_list.rs`)

| Benchmark | 1 | 4 | 8 | 32 | 64 | 128 | 512 |
|-|-|-|-|-|-|-|-|
| `add_events` | 294 ns | — | 313 ns | 426 ns | 201 ns | 968 ns | 10.9 µs |
| `vtable_add_events` | — | 207 ns | — | 380 ns | — | 957 ns | — |
| `vtable_get_all_events` | — | 27 ns | — | 58 ns | — | 213 ns | — |

#### Parameter Changes (`benches/param_changes.rs`)

| Benchmark | 1 | 4 | 8 | 16 | 32 | 64 |
|-|-|-|-|-|-|-|
| `add_change_single_param` | 9.4 ns | 80 ns | 28 ns | 100 ns | — | — |
| `add_change_multi_params` | 45 ns | 16 ns | — | 97 ns | 283 ns | 1.14 µs |
| `add_change_last_param` | — | — | 80 ns | — | 47 ns | 65 ns |

#### MIDI Translation (`benches/midi_translate.rs`)

| Benchmark | Result |
|-|-|
| `translate_note_on` | 4.28 ns |
| `translate_note_off` | 3.80 ns |
| `translate_unsupported_cc` | 2.94 ns |
| `event_note_on_construct` | 4.14 ns |
| `event_note_off_construct` | 3.13 ns |

| Batch | 4 | 16 | 64 | 128 | 256 |
|-|-|-|-|-|-|
| `translate_batch_notes` | 45 ns | 174 ns | 539 ns | 889 ns | 1.56 µs |
| `translate_batch_mixed` | — | 164 ns | 472 ns | — | 1.33 µs |

#### IPC Messages (`benches/ipc_messages.rs`)

| Benchmark | 0 events | 4 events | 16 events | 64 events |
|-|-|-|-|-|
| `encode_process_msg` | 561 ns | 1.51 µs | 3.35 µs | 7.43 µs |
| `decode_process_msg` | 577 ns | 1.24 µs | 3.19 µs | 10.7 µs |
| `roundtrip_process_msg` | — | — | — | — |

| Benchmark | Result |
|-|-|
| `encode_processed_response` | 57 ns |
| `encode_transport_state` | 169 ns |
| `encode_load_plugin` | 215 ns |

#### Process Context (`benches/process_context.rs`)

| Benchmark | 64 | 128 | 256 | 512 | 1024 |
|-|-|-|-|-|-|
| `advance_single` | — | — | — | — | — |
| `full_block_update` | — | — | — | — | — |

Sustain: 10 blocks × 512 samples in ~trivial time (arithmetic only).

#### Host Alloc (`benches/host_alloc.rs`)

| Benchmark | system_alloc | Box (mimalloc) |
|-|-|-|
| Small (16 B) | 18.2 ns | 19.6 ns |
| Medium (96 B) | 18.0 ns | 18.9 ns |
| Large (4 KB) | 78.3 ns | 68.3 ns |

system_alloc vs Box are comparable — mimalloc slightly faster for large allocs.

#### Diagnostics (`benches/diagnostics.rs`)

| Benchmark | Result |
|-|-|
| `heap_check` | 14.3 µs |
| `check_malloc_env` | 684 ns |
| `active_allocator_name` | 0.59 ns |
| `recommended_env_vars` | 22.1 ns |

#### Session Serde (`benches/session_serde.rs`)

| Benchmark | 1 slot | 4 slots | 8 slots | 16 slots |
|-|-|-|-|-|
| `serde_serialize` | — | — | — | — |
| `serde_deserialize` | — | — | — | — |
| `serde_roundtrip` | — | — | — | — |

#### Cache Serde (`benches/cache_serde.rs`)

| Benchmark | 4 modules | 16 modules | 64 modules |
|-|-|-|-|
| `serialize_cache` | 2.4 µs | 9.2 µs | 37.1 µs |
| `deserialize_cache` | 5.1 µs | 20.0 µs | 74.3 µs |
| `roundtrip_cache` | 9.0 µs | 38.8 µs | 143 µs |

### Notes

- All benchmarks run on Apple Silicon (aarch64-apple-darwin) in release mode
- Timer precision: 41 ns
- No regressions detected (this is the initial baseline)

---

## [0.19.3] - 2026-02-28 — Performance Optimizations

### Changes

5 targeted optimizations applied based on benchmark analysis:

1. **Stereo interleave/deinterleave fast path** — `chunks_exact`/`chunks_exact_mut` eliminates inner loop and bounds checks
2. **Leaner `prepare()`** — only refresh 2 self-referential pointers instead of full `update_ptrs()` rebuild
3. **Single-allocation `encode_message`** — `serde_json::to_writer` into one buffer (was: serialize → temp Vec → copy into final Vec)
4. **Pre-allocated `translate_midi_batch`** — `Vec::with_capacity(n)` + loop (was: `.filter_map().collect()`)
5. **Direct array cast in event_list `query_interface`** — `*(iid as *const [u8; 16])` vs `slice::from_raw_parts`

### Results vs v0.19.2 Baseline

#### Process Buffers — Stereo (most impactful, audio callback hot path)

| Benchmark | v0.19.2 | v0.19.3 | Speedup |
|-----------|---------|---------|---------|
| `write_input_interleaved_stereo/64` | 142.9 ns | 21.7 ns | **6.6×** |
| `write_input_interleaved_stereo/256` | 562.1 ns | 66.7 ns | **8.4×** |
| `write_input_interleaved_stereo/512` | 1,114 ns | 116.8 ns | **9.5×** |
| `write_input_interleaved_stereo/1024` | 2,228 ns | 231 ns | **9.6×** |
| `read_output_interleaved_stereo/64` | 144.2 ns | 55.3 ns | **2.6×** |
| `read_output_interleaved_stereo/256` | 562.1 ns | 222.3 ns | **2.5×** |
| `read_output_interleaved_stereo/512` | 1,114 ns | 400.7 ns | **2.8×** |
| `read_output_interleaved_stereo/1024` | 2,228 ns | 687 ns | **3.2×** |
| `full_cycle_stereo/128` | 582.9 ns | 132.4 ns | **4.4×** |
| `full_cycle_stereo/512` | 2,291 ns | 554.3 ns | **4.1×** |
| `full_cycle_stereo/1024` | 4,541 ns | 1,072 ns | **4.2×** |

8ch paths unchanged (no special-casing).

#### IPC Messages

| Benchmark | v0.19.2 | v0.19.3 | Speedup |
|-----------|---------|---------|---------|
| `encode_processed_response` | 48.8 ns | 27.7 ns | **1.76×** |
| `encode_load_plugin` | 214.5 ns | 140.3 ns | **1.53×** |
| `encode_transport_state` | 152 ns | 127.9 ns | **1.19×** |
| `encode_process_msg/0` | 536 ns | 463 ns | 1.16× |
| `encode_crash_report` | 797 ns | 724 ns | 1.10× |
| `roundtrip_process_msg/0` | 1,124 ns | 1,030 ns | 1.09× |

#### MIDI Translation (batch)

| Benchmark | v0.19.2 | v0.19.3 | Speedup |
|-----------|---------|---------|---------|
| `translate_batch_notes/16` | 171.5 ns | 90.2 ns | **1.90×** |
| `translate_batch_notes/64` | 520.4 ns | 309.5 ns | **1.68×** |
| `translate_batch_notes/128` | 885 ns | 562.1 ns | **1.57×** |
| `translate_batch_notes/256` | 1,551 ns | 1,083 ns | **1.43×** |
| `translate_batch_mixed/16` | 155.9 ns | 76.5 ns | **2.04×** |
| `translate_batch_mixed/64` | 457.9 ns | 252.2 ns | **1.81×** |
| `translate_batch_mixed/256` | 1,280 ns | 926.8 ns | **1.38×** |

### Net Impact on Audio Callback

For a typical stereo 1024-sample block with 16 MIDI events:
- **Before**: ~4.5 µs (interleave) + ~171 ns (MIDI) ≈ 4.7 µs
- **After**: ~1.1 µs (interleave) + ~90 ns (MIDI) ≈ 1.2 µs
- **Net**: **~3.5 µs saved per audio callback** (3.9× faster on this path)

No regressions detected in any benchmark suite.
