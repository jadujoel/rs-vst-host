# Dynamic Analysis with Miri and AddressSanitizer

This guide covers how to use [Miri](https://github.com/rust-lang/miri) and [AddressSanitizer (ASan)](https://clang.llvm.org/docs/AddressSanitizer.html) for dynamic analysis of unsafe code in `rs-vst-host`. Together they provide complementary coverage:

- **Miri** interprets Rust MIR and catches aliasing violations, uninitialized reads, and data races in pure-Rust unsafe code
- **ASan** instruments compiled native code and catches use-after-free, double-free, buffer overflows, and allocator mismatches in FFI code paths (libc::malloc, mmap, etc.)

## What Each Tool Detects

### Miri

Miri can find bugs that neither the compiler nor standard tests catch:

- **Use-after-free** — dereferencing pointers to freed memory
- **Double-free** — freeing the same allocation twice
- **Out-of-bounds access** — reading/writing past allocation boundaries
- **Invalid alignment** — accessing data through improperly aligned pointers
- **Uninitialized memory reads** — reading `MaybeUninit` or `mem::zeroed` data without initialization
- **Aliasing violations** — multiple `&mut` references or `&mut` + `&` to the same data
- **Data races** — unsynchronized access across threads
- **Invalid `Send`/`Sync` usage** — moving non-Send types between threads
- **Dangling pointer dereference** — using pointers after their provenance expires

## Prerequisites

```bash
# Install the nightly toolchain (Miri requires nightly)
rustup toolchain install nightly

# Add the Miri component
rustup +nightly component add miri

# rust-src is also needed (Miri will prompt for it on first run)
rustup +nightly component add rust-src
```

## Project Setup

The project has a `src/lib.rs` that re-exports all modules as a library crate, enabling `cargo miri test --lib` without compiling the binary entry point (which uses FFI global allocators that Miri cannot interpret).

Miri-specific integration tests are in `src/miri_tests.rs` — 21 tests targeting the most safety-critical unsafe code paths.

## Quick Start

### Run all Miri-compatible tests (recommended)

```bash
# Tree Borrows model — handles self-referential structs correctly
MIRIFLAGS="-Zmiri-tree-borrows" cargo +nightly miri test --lib -- \
  "vst3::event_list" "vst3::param_changes" "vst3::process" \
  "vst3::types" "midi::translate" "miri_tests"
```

This runs **~109 tests** covering:
- COM vtable dispatch (event list, parameter changes)
- Self-referential buffer management (ProcessBuffers)
- Struct-to-bytes reinterpretation (Event union)
- Cross-module integration (MIDI → EventList → ProcessData)
- Thread safety (`Send` across threads)

### Run only the dedicated Miri tests

```bash
MIRIFLAGS="-Zmiri-tree-borrows" cargo +nightly miri test --lib -- miri_tests
```

### Run with Stacked Borrows (stricter, older model)

```bash
cargo +nightly miri test --lib -- "vst3::event_list" "vst3::param_changes" \
  "vst3::types" "midi::translate" "miri_tests::tests::miri_event" \
  "miri_tests::tests::miri_com" "miri_tests::tests::miri_null" \
  "miri_tests::tests::miri_param"
```

**~70 tests** pass under Stacked Borrows. The `ProcessBuffers` tests trigger Stacked Borrows violations due to the self-referential pointer pattern (see [Known Limitations](#known-limitations)).

## Aliasing Models: Stacked Borrows vs Tree Borrows

Miri supports two aliasing models that determine what pointer usage patterns are considered valid:

### Stacked Borrows (default)

The original and stricter model. Every memory location has a "borrow stack" tracking which references are allowed to access it. Rules:

- `&mut` creates a Unique entry — invalidates all other entries below it
- `&` creates a SharedReadOnly entry
- Raw pointers (`*mut`, `*const`) create SharedReadWrite entries
- Accessing through an invalidated entry is Undefined Behavior

**Limitation for this project:** `ProcessBuffers` uses a self-referential pattern where `process_data.inputs` stores a raw pointer to `self.input_bus`. Any `&mut self` method call retags the entire struct, invalidating the stored pointer. This is technically UB under Stacked Borrows but is a well-known pattern that works correctly in practice.

### Tree Borrows (`-Zmiri-tree-borrows`)

A newer, more permissive model designed to handle common patterns that are technically UB under Stacked Borrows but safe in practice. Uses a tree structure instead of a stack, allowing more flexible aliasing.

**Recommended for this project** because it correctly handles the `ProcessBuffers` self-referential pattern while still catching real bugs like use-after-free, double-free, and data races.

## What's Tested Under Miri

### Miri-Compatible Modules (no FFI)

| Module | Tests | What's Validated |
|--------|------:|-----------------|
| `vst3/event_list.rs` | 14 | COM vtable dispatch, `Box::into_raw`/`from_raw`, `mem::zeroed` safety |
| `vst3/param_changes.rs` | 16 | Nested COM objects, queue reuse, `mem::zeroed` array init |
| `vst3/process.rs` | 20 | Self-referential pointer chain, deinterleaving, `unsafe impl Send` |
| `vst3/process_context.rs` | 10 | Pointer cast to `*mut c_void` |
| `vst3/types.rs` | 10 | Pure data structs (serde, Clone) — no unsafe |
| `midi/translate.rs` | 18 | `Event::note_on`/`note_off` byte reinterpretation |
| `miri_tests.rs` | 21 | Integration: MIDI→EventList→ProcessData, full mock process call |
| **Total** | **109** | |

### Miri-Targeted Tests (`miri_tests.rs`)

These 21 tests exercise the highest-risk unsafe patterns:

| Test | What It Validates |
|------|-------------------|
| `miri_event_note_on_roundtrip` | NoteOnEvent struct→bytes→struct reinterpretation |
| `miri_event_note_off_roundtrip` | NoteOffEvent struct→bytes→struct reinterpretation |
| `miri_event_data_fully_initialized` | No uninitialized memory in Event.data bytes |
| `miri_event_extreme_values` | Boundary values (i16::MAX, i32::MIN) through byte conversion |
| `miri_event_list_full_lifecycle` | Create→add→vtable read→destroy lifecycle |
| `miri_event_list_query_interface_preserves_object` | QI doesn't corrupt COM object state |
| `miri_event_list_capacity_stress` | 512 events — vtable dispatch at capacity |
| `miri_param_changes_full_lifecycle` | Parameter queue create→add→clear→destroy |
| `miri_param_changes_queue_reuse` | Existing queue reuse doesn't corrupt adjacent queues |
| `miri_process_buffers_pointer_chain` | Full: ProcessData→AudioBusBuffers→channel_buffers→samples |
| `miri_process_buffers_prepare_stability` | Pointer chain valid after repeated `prepare()` calls |
| `miri_process_buffers_interleave_roundtrip` | Interleave→deinterleave via raw pointer copy |
| `miri_process_buffers_zero_channels` | Zero-channel config: null pointer safety |
| `miri_process_buffers_asymmetric_channels` | Mono in / stereo out pointer arrays |
| `miri_process_context_in_process_data` | ProcessContext wired into ProcessData |
| `miri_midi_to_process_data_integration` | MIDI→translate→EventList→ProcessData→vtable readback |
| `miri_full_mock_process_call` | All COM objects + buffers wired into ProcessData simultaneously |
| `miri_process_buffers_send_across_thread` | `unsafe impl Send` — move + re-prepare on another thread |
| `miri_com_lifecycle_stress` | 50 create/destroy cycles for memory leak detection |
| `miri_event_clone` | Bitwise copy of Event with byte-array union |
| `miri_null_destroy_safety` | Null pointer destroy is a no-op |

### Modules NOT Compatible with Miri

These modules use FFI or system calls that Miri cannot interpret:

| Module | Why |
|--------|-----|
| `vst3/host_alloc.rs` | `libc::malloc` / `libc::free` |
| `vst3/host_context.rs` | Uses `host_alloc` → `libc::malloc` |
| `vst3/component_handler.rs` | Uses `host_alloc` → `libc::malloc` |
| `vst3/plug_frame.rs` | Uses `host_alloc` → `libc::malloc` |
| `ipc/shm.rs` | `shm_open`, `mmap`, `munmap`, `ftruncate` |
| `vst3/sandbox.rs` | `sigsetjmp`, `siglongjmp`, `sigaction` |
| `vst3/instance.rs` | COM FFI to VST3 plugins |
| `vst3/module.rs` | `libloading` (dlopen/dlsym) |
| `gui/editor.rs` | ObjC runtime FFI |
| `audio/engine.rs` | `cpal` audio device FFI |

## Known Limitations

### 1. Self-Referential Pointer Pattern in `ProcessBuffers`

`ProcessBuffers` stores raw pointers from `&mut self` fields into `ProcessData`:

```rust
// In update_ptrs():
self.process_data.inputs = &mut self.input_bus;
self.input_bus.channel_buffers_32 = self.input_ptrs.as_mut_ptr();
```

Under Stacked Borrows, any subsequent `&mut self` method call (e.g., `set_input_events()`) invalidates these stored pointers because the function entry retag covers the entire allocation.

**Impact:** This is technically "UB" under the Stacked Borrows model, but:
- The pointers are re-established by `prepare()` before each real-time process call
- The underlying memory never moves (Vec capacities are pre-allocated)
- The pattern is correct under Tree Borrows and real hardware
- The Rust unsafe code guidelines group is aware of this class of issues

**Mitigation:** Always run Miri with `-Zmiri-tree-borrows` to test `ProcessBuffers` code.

### 2. `ipc/messages.rs` Tests Are Very Slow

Serde's heavy generic expansion makes message serialization tests extremely slow under Miri (minutes per test vs milliseconds normally). These tests contain no unsafe code, so the Miri coverage gain is minimal — they can be skipped.

### 3. No FFI Coverage

Miri cannot interpret foreign function calls. All interaction with VST3 plugins, audio devices, MIDI devices, and native windows is outside Miri's scope. These paths are covered by the project's signal-handler sandboxing and process isolation instead.

## CI Integration

Add to your CI pipeline:

```yaml
# GitHub Actions example
- name: Miri dynamic analysis
  run: |
    rustup toolchain install nightly
    rustup +nightly component add miri rust-src
    MIRIFLAGS="-Zmiri-tree-borrows" cargo +nightly miri test --lib -- \
      "vst3::event_list" "vst3::param_changes" "vst3::process" \
      "vst3::types" "midi::translate" "miri_tests"
```

Expected runtime: ~30 seconds on modern hardware.

## Interpreting Miri Output

### Clean run

```
test miri_tests::tests::miri_full_mock_process_call ... ok
test result: ok. 109 passed; 0 failed; 0 ignored
```

### When Miri finds a bug

```
error: Undefined Behavior: trying to retag from <TAG> for SharedReadOnly permission
  at alloc[0x60], but that tag does not exist in the borrow stack
  --> src/vst3/process.rs:124:40
```

The error message includes:
1. **What** — the specific UB (Stacked Borrows violation, use-after-free, etc.)
2. **Where** — the code location that triggered the UB
3. **When it was created** — where the original borrow/allocation happened
4. **When it was invalidated** — what operation made it invalid

For a more detailed backtrace:

```bash
MIRIFLAGS="-Zmiri-tree-borrows -Zmiri-backtrace=full" cargo +nightly miri test --lib -- <test_name>
```

## Useful Miri Flags

| Flag | Effect |
|------|--------|
| `-Zmiri-tree-borrows` | Use Tree Borrows instead of Stacked Borrows |
| `-Zmiri-backtrace=full` | Full stack trace on errors |
| `-Zmiri-disable-isolation` | Allow access to host environment (clock, etc.) |
| `-Zmiri-ignore-leaks` | Don't report memory leaks at exit |
| `-Zmiri-symbolic-alignment-check` | Check alignment symbolically |
| `-Zmiri-seed=N` | Deterministic thread scheduling (for reproducing races) |

## Complementary Tools

Miri and ASan are two layers in the project's safety strategy:

| Layer | Tool | Coverage |
|-------|------|----------|
| Static | `cargo clippy`, `unsafe_op_in_unsafe_fn` lint | All code |
| Dynamic (safe Rust) | `cargo test` (579 tests) | All modules |
| Dynamic (unsafe Rust) | **Miri** (109 tests) | COM vtable, buffers, events |
| Dynamic (native code) | **ASan** (564 tests) | FFI, system malloc, shm, COM objects |
| Dynamic (FFI) | Signal handler sandbox (`vst3/sandbox.rs`) | Plugin COM calls |
| Dynamic (crash isolation) | Process-per-plugin (`ipc/`) | Full plugin lifecycle |
| Runtime | mimalloc heap isolation | Rust vs. plugin allocations |
| Runtime | `MallocGuardEdges` + `MallocScribble` (macOS) | System heap corruption |
| Profiling | dhat (`--features debug-alloc`) | Heap allocation tracking |

---

## AddressSanitizer (ASan)

ASan instruments compiled native code at the LLVM level and catches real hardware-level memory errors at runtime with low overhead. Unlike Miri, ASan can run tests that use FFI (libc::malloc, mmap, etc.).

### What ASan Detects

- **Use-after-free** — accessing memory after `free()` / `system_free()`
- **Double-free** — freeing the same allocation twice
- **Heap buffer overflow** — reading/writing past allocation boundaries
- **Stack buffer overflow** — out-of-bounds access on stack arrays
- **Memory leaks** — allocated memory never freed (ASan leak detector)
- **Allocator mismatch** — `malloc`/`Box` cross-free detection

### Prerequisites

```bash
# ASan requires the nightly toolchain
rustup toolchain install nightly
```

### Quick Start

#### Run all ASan-targeted tests

```bash
RUSTFLAGS="-Z sanitizer=address" \
  cargo +nightly test --target aarch64-apple-darwin --lib -- asan_tests
```

This runs **46 tests** specifically designed for ASan validation.

#### Run the full test suite under ASan

```bash
RUSTFLAGS="-Z sanitizer=address" \
  cargo +nightly test --target aarch64-apple-darwin --lib -- \
    --skip test_heap_check_returns_true_in_clean_process \
    --skip test_sandbox_catches_raised_sigbus \
    --skip test_sandbox_catches_sigsegv \
    --skip test_sandbox_recovery_allows_subsequent_calls \
    --skip test_sandbox_catches_sigabrt \
    --skip test_sandbox_multiple_crashes_same_signal \
    --skip test_sandbox_alternating_crash_and_normal \
    --skip test_sandbox_crash_produces_backtrace \
    --skip test_clean_recovery_has_no_heap_corruption \
    --skip test_sandbox_crash_recovery_in_instance_context \
    --skip test_sandbox_catches_abort_during_cleanup \
    --skip test_last_drop_crashed_set_on_sandbox_crash \
    --skip test_crash_flags_set_together_on_com_crash \
    --skip test_module_drop_skips_unload_after_instance_crash \
    --skip test_check_heap_after_recovery_clean
```

This runs **564 tests** (579 total minus 15 ASan-incompatible).

### macOS Target Requirement

On macOS, ASan requires specifying `--target aarch64-apple-darwin` explicitly. Without it, ASan's interceptors fail to install and the process aborts with SIGABRT.

### ASan-Targeted Tests (`asan_tests.rs`)

46 tests covering the FFI-heavy code paths that Miri cannot interpret:

| Category | Tests | What's Validated |
|----------|------:|-----------------|
| host_alloc lifecycle | 7 | system_alloc/system_free pairing, null safety, varying sizes, concurrent threads, rapid cycle stress, drop semantics |
| COM object lifecycle | 5 | HostApplication, HostComponentHandler, HostPlugFrame create→use→destroy |
| ProcessBuffers | 5 | Full pointer chain, varying block sizes, cross-thread transfer, zero channels, interleave roundtrip |
| Shared memory | 5 | Create/write/read, boundary writes, host↔worker roundtrip, zero channels, rapid create/destroy |
| Event byte access | 3 | Note on/off byte-level roundtrip, event clone safety |
| MIDI→ProcessData | 3 | Batch translate, all 16 channels, full pipeline |
| Sandbox (non-crash) | 6 | Normal call, heap alloc, system_alloc, panic recovery, nested, sequential stress |
| IPC messages | 1 | Encode/decode roundtrip for all message variants |
| Full mock process | 2 | All COM objects wired into ProcessData, multi-block session |
| Concurrent COM | 2 | Multi-threaded handler edits (COM vtable), concurrent object create/destroy |
| Zone check | 1 | system_alloc pointer validation under ASan's malloc wrapper |
| **Total** | **46** | |

### ASan-Incompatible Tests (15 skipped)

These tests conflict with ASan's signal and malloc zone interception:

| Test | Conflict |
|------|----------|
| `test_heap_check_returns_true_in_clean_process` | `malloc_zone_check` — ASan replaces malloc zones |
| `test_check_heap_after_recovery_clean` | `check_heap_after_recovery` → `malloc_zone_check` |
| `test_sandbox_catches_raised_sigbus` | `libc::raise(SIGBUS)` — ASan intercepts signals |
| `test_sandbox_catches_sigsegv` | `libc::raise(SIGSEGV)` |
| `test_sandbox_catches_sigabrt` | `libc::raise(SIGABRT)` |
| `test_sandbox_recovery_allows_subsequent_calls` | `libc::raise` |
| `test_sandbox_multiple_crashes_same_signal` | `libc::raise` |
| `test_sandbox_alternating_crash_and_normal` | `libc::raise` |
| `test_sandbox_crash_produces_backtrace` | `libc::raise` |
| `test_clean_recovery_has_no_heap_corruption` | `libc::raise` + `malloc_zone_check` |
| `test_sandbox_crash_recovery_in_instance_context` | `libc::raise` |
| `test_sandbox_catches_abort_during_cleanup` | `libc::raise` |
| `test_last_drop_crashed_set_on_sandbox_crash` | `libc::raise` |
| `test_crash_flags_set_together_on_com_crash` | `libc::raise` + `heap_check` |
| `test_module_drop_skips_unload_after_instance_crash` | `libc::raise` |
