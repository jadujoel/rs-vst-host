# Performance Changelog

All performance benchmark results are tracked here. Benchmarks use [Divan](https://github.com/nvzqz/divan).

Run benchmarks: `cargo bench`

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
