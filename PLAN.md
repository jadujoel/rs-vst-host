# VST3 Host Plan (Rust)

This plan outlines a practical path to build a minimal, reliable VST3 host in this repository, then iterate toward a production-ready host.

## Goals

- Load and run VST3 instrument/effect plugins.
- Process real-time audio with low latency and no allocations on the audio thread.
- Support basic MIDI input and automation.
- Provide a small but usable host shell for scanning, loading, and running plugins.

## Non-goals (MVP)

- Full DAW timeline, recording, or advanced editing.
- Complex project/session management.
- Network collaboration features.

## Phase 0 — Discovery and Technical Decisions

1. **Select core crates and approach**
   - Evaluate VST3 interop options in Rust (existing bindings vs custom FFI to Steinberg SDK).
   - Choose audio backend strategy:
     - `cpal` for cross-platform audio I/O, or
     - native backend abstractions if lower-level control is needed.
   - Choose MIDI input crate (e.g. `midir`) for live MIDI.

2. **Licensing and SDK setup**
   - Verify Steinberg VST3 SDK licensing compatibility with project goals.
   - Add SDK acquisition/setup instructions in project docs (do not commit restricted SDK content if prohibited).

3. **Define MVP success criteria**
   - Host can discover at least one plugin path.
   - Host can instantiate plugin and process buffers.
   - Host can pass MIDI note-on/off to instrument and hear output.

## Phase 1 — Project Foundations

1. **Restructure codebase**
   - Keep `src/main.rs` as binary entry.
   - Add modules:
     - `src/audio/` (device, stream, callbacks)
     - `src/midi/` (input, event queue)
     - `src/vst3/` (loading, factory, component, processor)
     - `src/host/` (transport/time info, parameter plumbing)
     - `src/app/` (CLI/app wiring)

2. **Error handling + logging**
   - Introduce `thiserror` for typed errors.
   - Add `tracing` + `tracing-subscriber` for structured logs.

3. **Threading model design**
   - Main/control thread for plugin lifecycle and UI commands.
   - Audio callback thread for real-time processing only.
   - Lock-free queues/ring buffers between control and audio paths.

## Phase 2 — VST3 Plugin Discovery and Loading

1. **Plugin scanner**
   - Search standard VST3 install locations per OS plus user-provided paths.
   - Validate candidate bundles/files and read metadata.
   - Cache scan results (e.g. JSON file with plugin info).

2. **Dynamic loading + factory access**
   - Load plugin module and obtain VST3 factory.
   - Enumerate classes and pick processor/instrument class IDs.
   - Instantiate component + audio processor interfaces.

3. **Host context interfaces**
   - Implement required host-side interfaces (minimal stubs first).
   - Ensure plugin initialization/termination lifecycle is correct.

## Phase 3 — Audio Engine Integration

1. **Audio device setup**
   - Open output stream with configurable sample rate, block size, and channel count.
   - Store current stream format in shared host state.

2. **Process setup negotiation**
   - Configure plugin bus arrangements.
   - Set sample rate, max block size, and process mode.
   - Activate/deactivate plugin processing correctly.

3. **Real-time processing loop**
   - In callback: gather queued MIDI/automation events for current block.
   - Call VST3 process with prepared buffers and event lists.
   - Write plugin output to device buffers.

4. **Real-time safety audit**
   - Remove allocations, blocking locks, and I/O from callback path.
   - Preallocate process structures reused per block.

## Phase 4 — MIDI, Parameters, and Basic Automation

1. **MIDI routing**
   - Capture MIDI from selected input device.
   - Translate to VST3 event structures with sample offsets.

2. **Parameter introspection**
   - Enumerate plugin parameters and metadata.
   - Build host-side parameter registry (ID, range, default, normalized value).

3. **Parameter changes/automation**
   - Apply parameter updates from control thread to audio thread safely.
   - Implement sample-accurate parameter queue where feasible.

## Phase 5 — Host UX (MVP CLI)

1. **CLI commands**
   - `scan`: scan plugin paths and cache metadata.
   - `list`: list discovered plugins.
   - `run --plugin <id|path>`: load plugin and start audio engine.

2. **Runtime controls**
   - Device selection (audio + MIDI).
   - Basic parameter set/get from CLI.
   - Graceful shutdown and resource cleanup.

## Phase 6 — Validation and Quality Gates

1. **Automated tests (non-RT components)**
   - Unit tests for scanner, metadata parsing, and parameter mapping.
   - Queue and scheduling logic tests.

2. **Manual test matrix**
   - Test at multiple sample rates/block sizes.
   - Test with at least: one synth plugin, one effect plugin.
   - Verify startup/shutdown, reload, and error paths.

3. **Performance checks**
   - Measure callback xruns/dropouts under load.
   - Track CPU usage and latency baseline.

## Phase 7 — Iteration Beyond MVP

- Add plugin editor window support where available.
- Add preset/program management.
- Add multiple plugin instances and simple routing graph.
- Add session save/load.

## Suggested Milestones

- **M1:** Scanner + plugin metadata listing works.
- **M2:** Single plugin instantiates and initializes.
- **M3:** Real-time audio callback calls plugin process reliably.
- **M4:** MIDI note input triggers instrument output.
- **M5:** Parameter control + stable CLI UX.

## Risks and Mitigations

- **SDK and binding complexity:** start with smallest supported interface subset, expand gradually.
- **Real-time glitches:** enforce strict RT-safe coding rules and preallocation.
- **Plugin variability:** test across multiple vendors early; improve compatibility incrementally.
- **Cross-platform differences:** isolate OS-specific scanning/loading logic behind traits.

## Immediate Next Actions

1. Pick and document exact crate choices for VST3 interop/audio/MIDI.
2. Create module skeletons (`audio`, `midi`, `vst3`, `host`, `app`).
3. Implement `scan` + `list` CLI as first shippable vertical slice.
