# Phase 8 — Beyond MVP: Production-Ready VST3 Host

## Overview

Phase 8 transforms rs-vst-host from a functional MVP into a production-ready VST3 host. Building on the solid foundation of Phases 0–7 (CLI, audio engine, MIDI, parameters, GUI, crash sandboxing, process isolation, three-process architecture), this phase focuses on missing DAW-grade features: preset management, multi-plugin routing, cross-platform support, performance hardening, and polish.

**Entry state:** v0.19.0 — 678 tests passing, three-process architecture (supervisor → audio worker + GUI worker), process-per-plugin sandboxing, Liquid Glass GUI with plugin browser/rack/parameter panel/session save-load, E2E test suite with real FabFilter plugins.

---

## Goals

1. **Preset/program management** — Browse, load, save, and switch presets per plugin
2. **Multi-plugin routing** — Multiple plugins in series/parallel with a visual routing graph
3. **Plugin state persistence** — Save/restore full plugin component state (not just parameters)
4. **Cross-platform editor windows** — Linux (X11/Wayland) and Windows editor embedding
5. **Performance hardening** — Real-time safety audit, lock-free improvements, xrun tracking
6. **Undo/redo** — Reversible parameter and rack changes
7. **Drag-and-drop rack reordering** — Visual slot reordering in the GUI
8. **Plugin compatibility** — Broader VST3 plugin support and edge case handling
9. **Distribution & packaging** — App bundles, installers, CI/CD pipeline

---

## Sub-Phases

### Phase 8.1 — Plugin State Persistence (Component State Save/Load)

**Goal:** Save and restore the full internal state of each plugin instance, going beyond parameter snapshots.

**Background:** Currently, session save/load preserves parameter values but not the plugin's opaque component state (e.g., custom UI state, internal lookup tables, undo history). VST3 provides `IComponent::getState()`/`setState()` and `IEditController::getState()`/`setState()` for this.

**Tasks:**

1. **IBStream COM implementation** (`vst3/ibstream.rs`)
   - Implement `IBStream` interface (read/write/seek/tell) backed by a `Vec<u8>` buffer
   - COM vtable with proper reference counting
   - Used for both reading and writing plugin state

2. **State capture on Vst3Instance** (`vst3/instance.rs`)
   - `get_component_state() -> Vec<u8>` — calls `IComponent::getState()` with IBStream
   - `get_controller_state() -> Vec<u8>` — calls `IEditController::getState()` with IBStream
   - `set_component_state(data: &[u8])` — calls `IComponent::setState()` with IBStream
   - `set_controller_state(data: &[u8])` — calls `IEditController::setState()` with IBStream

3. **Session format v2** (`gui/session.rs`)
   - Extend `SessionSlot` with optional `component_state: Option<Base64Bytes>` and `controller_state: Option<Base64Bytes>`
   - Bump session format version with backward compatibility (v1 sessions load without state blobs)
   - State capture on session save, state restore on session load (after plugin instantiation)

4. **IPC transport for state blobs** (`gui/ipc.rs`, `gui/audio_worker.rs`)
   - New `GuiAction::SaveState(slot_index)` / `SupervisorUpdate::StateBlob(slot_index, data)`
   - Audio worker extracts state and sends to supervisor for session persistence

5. **Tests**
   - IBStream COM vtable tests (read, write, seek, tell, QI, ref counting)
   - State capture/restore roundtrip with mock data
   - Session v2 serde with and without state blobs
   - Backward compatibility: load v1 session in v2 code

**Success criteria:** Plugin state survives save → quit → restart → load cycle.

---

### Phase 8.2 — Preset/Program Management

**Goal:** Browse, load, save, and switch plugin presets.

**Background:** VST3 defines `IUnitInfo` for program lists and `IProgramListData` for program data access. Many plugins expose factory presets through this interface. User presets can be stored as state blobs on disk.

**Tasks:**

1. **IUnitInfo query** (`vst3/instance.rs`)
   - Query `IUnitInfo` from the edit controller
   - Enumerate units and program lists
   - `get_program_count(list_id)` and `get_program_name(list_id, index)`

2. **Preset file format** (`vst3/presets.rs`)
   - Define a JSON-based preset file: `{ name, plugin_cid, component_state, controller_state, param_overrides }`
   - Save/load to `~/.rs-vst-host/presets/<plugin-name>/`
   - Import/export to VST3 `.vstpreset` format (Steinberg standard)

3. **Preset browser panel** (`gui/app.rs`)
   - New tab or side panel showing factory presets (from IUnitInfo) and user presets (from disk)
   - Search/filter by name
   - One-click load: captures current state, applies preset state, updates parameter panel

4. **Preset save workflow**
   - "Save Preset" button captures current plugin state + parameters
   - Name dialog with overwrite protection
   - User preset directory management

5. **GUI integration**
   - Preset name displayed in rack slot
   - Previous/next preset navigation buttons
   - "Init" button to reset to default state

6. **Tests**
   - Preset file serde roundtrip
   - IUnitInfo mock tests (program count, program names)
   - Preset save/load filesystem tests
   - GUI preset panel state tests

**Success criteria:** User can browse factory presets, save custom presets, and switch between them.

---

### Phase 8.3 — Multi-Plugin Routing Graph

**Goal:** Support multiple plugins in configurable series and parallel chains with a visual routing editor.

**Background:** Currently the rack is a single slot. Phase 8.3 adds a proper signal routing graph where plugins can be chained in series or arranged in parallel with mix/split nodes.

**Tasks:**

1. **Routing graph data model** (`audio/graph.rs`)
   - `AudioGraph` struct with nodes (plugin instances, input, output, split, mix) and edges (audio connections)
   - Topological sort for processing order
   - Cycle detection
   - Add/remove node, connect/disconnect edge operations
   - Support stereo and mono bus routing

2. **Graph-aware audio engine** (`audio/engine.rs`)
   - Replace single-plugin processing with graph traversal
   - Process nodes in topological order
   - Intermediate buffer pool for inter-node audio transfer
   - Support parallel branches (process independent subgraphs concurrently where possible)

3. **Graph-aware process isolation** (`ipc/`)
   - Each node in the graph can be in-process or in its own child process
   - Shared memory regions per edge for zero-copy inter-process audio
   - Supervisor manages multiple audio worker processes

4. **Visual routing editor** (`gui/routing.rs`)
   - Node-based editor panel using `egui`
   - Drag-to-connect between node ports
   - Glass-styled nodes with plugin name, bypass toggle, and status indicator
   - Auto-layout algorithm (left-to-right flow)

5. **Serial chain shortcut**
   - For the common case: rack slots implicitly form a serial chain (output of slot N feeds input of slot N+1)
   - "Advanced routing" toggle reveals the full graph editor

6. **Tests**
   - Graph construction, topological sort, cycle detection
   - Multi-node processing correctness (known DSP → expected output)
   - Add/remove node with graph reconnection
   - Graph serialization for session save/load

**Success criteria:** Two or more plugins process audio in sequence, with optional parallel routing visible in the GUI.

---

### Phase 8.4 — Undo/Redo System

**Goal:** Provide reversible parameter changes and rack operations.

**Tasks:**

1. **Command pattern** (`gui/undo.rs`)
   - `UndoCommand` trait with `execute()`, `undo()`, `description()`
   - Concrete commands: `SetParameter`, `AddPlugin`, `RemovePlugin`, `ReorderPlugin`, `LoadPreset`, `SetTempo`, `SetTimeSignature`
   - `UndoStack` with configurable max depth (default 100)

2. **Parameter coalescing**
   - Consecutive `SetParameter` commands on the same parameter within 500ms are merged into a single undo entry
   - Begin/end edit markers from `IComponentHandler` define coalescing boundaries

3. **GUI integration**
   - Undo/Redo buttons in toolbar
   - Keyboard shortcuts: Cmd+Z / Cmd+Shift+Z (macOS), Ctrl+Z / Ctrl+Shift+Z (Linux/Windows)
   - Undo history dropdown showing recent operations

4. **Tests**
   - Undo/redo for each command type
   - Coalescing behavior
   - Stack overflow (max depth eviction)
   - Redo invalidation on new action after undo

**Success criteria:** User can undo/redo parameter changes and rack modifications.

---

### Phase 8.5 — Drag-and-Drop Rack Reordering

**Goal:** Allow visual reordering of plugins in the rack by dragging.

**Tasks:**

1. **Drag interaction** (`gui/app.rs`)
   - Detect drag start on rack slot (mouse down + move threshold)
   - Visual feedback: dragged slot follows cursor with transparency, insertion marker between slots
   - Drop: reorder the slot list, update routing graph connections

2. **Integration with undo system**
   - Reorder emits a `ReorderPlugin` undo command

3. **Integration with routing graph**
   - Serial chain is automatically updated when slots are reordered
   - If advanced routing is active, prompt user to confirm reconnection

4. **Tests**
   - Slot reorder logic (move forward, backward, no-op)
   - Undo/redo for reorder
   - Routing graph reconnection after reorder

**Success criteria:** User can drag rack slots to change processing order.

---

### Phase 8.6 — Cross-Platform Plugin Editor Windows

**Goal:** Support native plugin editor embedding on Linux and Windows (macOS already works).

**Tasks:**

1. **Linux editor embedding** (`gui/editor_linux.rs`)
   - X11 window creation via `x11rb` or `xcb` crate
   - XEmbed protocol for embedding plugin editor views
   - Wayland support via `xwayland` fallback or native `wl_surface` if the plugin supports it
   - Resize handling via `IPlugFrame`

2. **Windows editor embedding** (`gui/editor_windows.rs`)
   - `HWND` creation via `windows` crate
   - Win32 window as parent for IPlugView
   - Message pump integration with `eframe`
   - DPI awareness and scaling

3. **Platform abstraction** (`gui/editor.rs`)
   - `EditorWindow` trait with platform-specific implementations
   - `#[cfg(target_os = "...")]` conditional compilation
   - Unified open/close/resize API

4. **Tests**
   - Platform detection and editor availability
   - EditorWindow trait mock implementation tests
   - Resize propagation tests

**Success criteria:** Plugin editors open on macOS, Linux, and Windows.

---

### Phase 8.7 — Performance Hardening

**Goal:** Ensure rock-solid real-time audio performance under load.

**Tasks:**

1. **Real-time safety audit** (`audio/engine.rs`)
   - Static analysis pass: identify any allocation, lock, or I/O on the audio thread
   - Replace any remaining `Mutex::lock()` on audio thread with `try_lock()` + fallback
   - Verify all process buffers are pre-allocated

2. **Lock-free parameter queue**
   - Replace `Mutex`-protected parameter change queue with a lock-free SPSC ring buffer
   - Use `crossbeam` or custom wait-free implementation
   - Benchmarks to verify zero-allocation on audio thread

3. **Xrun detection and reporting**
   - Track audio callback timing (expected vs actual interval)
   - Count and display xruns (buffer underruns) in the GUI status bar
   - Log xruns with timestamps and context

4. **CPU usage monitoring**
   - Measure plugin `process()` wall-clock time per block
   - Calculate and display CPU load percentage in the GUI
   - Per-plugin CPU breakdown in the parameter panel

5. **Thread priority**
   - Set audio thread to real-time priority on macOS (`pthread_set_qos_class_self`)
   - Linux: SCHED_FIFO with appropriate priority
   - Windows: `THREAD_PRIORITY_TIME_CRITICAL`

6. **Benchmarks**
   - `criterion` benchmarks for audio processing hot path
   - Measure latency at various buffer sizes (32, 64, 128, 256, 512, 1024)
   - Track regression via CI

7. **Tests**
   - Lock-free queue correctness under concurrent access
   - Xrun counter accuracy
   - CPU measurement plausibility

**Success criteria:** Zero xruns during a 30-minute session at 64-sample buffer size on reference hardware.

---

### Phase 8.8 — Plugin Compatibility Improvements

**Goal:** Broaden the range of VST3 plugins that work correctly.

**Tasks:**

1. **IPluginFactory3 improvements**
   - Better handling of `classInfo2` and Unicode metadata
   - Subcategory parsing and normalization

2. **Bus arrangement negotiation fallbacks**
   - Try multiple arrangements if the preferred one is rejected
   - Support mono plugins, surround plugins, and plugins with side-chain inputs

3. **Latency compensation**
   - Query `IAudioProcessor::getLatencySamples()` per plugin
   - Implement delay compensation in the routing graph to align plugin outputs

4. **IProgress and IMessage support**
   - Implement `IProgress` interface for long-running plugin operations
   - Implement `IMessage`/`IAttributeList` for plugin-to-host communication

5. **Plugin validation suite**
   - Automated test runner that exercises every loaded plugin through a standard sequence
   - Report compatibility issues per plugin
   - Test with common free VST3 plugins (Surge XT, Vital, Dexed, etc.)

6. **Tests**
   - Bus arrangement fallback logic
   - Latency value query and compensation math
   - IProgress/IMessage COM vtable tests

**Success criteria:** ≥95% of tested VST3 plugins load and process audio correctly.

---

### Phase 8.9 — Distribution and Packaging

**Goal:** Make rs-vst-host easy to install and distribute.

**Tasks:**

1. **macOS app bundle**
   - `.app` bundle with proper `Info.plist`
   - Code signing with `codesign`
   - DMG installer with drag-to-Applications
   - Hardened runtime for notarization

2. **Linux packaging**
   - AppImage for universal distribution
   - `.deb` package for Debian/Ubuntu
   - `.rpm` package for Fedora
   - Flatpak manifest

3. **Windows installer**
   - MSI or NSIS installer
   - Start menu and desktop shortcuts
   - Proper uninstall support

4. **CI/CD pipeline**
   - GitHub Actions workflow for build + test on macOS, Linux, Windows
   - Automated release builds with artifact upload
   - Test matrix: multiple Rust versions, multiple OS versions
   - Miri and ASan runs in CI

5. **Documentation**
   - Installation guide per platform
   - Update USER_GUIDE.md with platform-specific instructions
   - Contributing guide

6. **Tests**
   - CI smoke test: build → scan → list on each platform
   - Package installation/uninstallation test scripts

**Success criteria:** One-command install on all three platforms; CI runs green on every PR.

---

## Phase 8 Milestones

| Milestone | Sub-Phase | Description | Key Metric |
|-----------|-----------|-------------|------------|
| M6 | 8.1 | Plugin state save/load works | State survives restart |
| M7 | 8.2 | Preset browser functional | Factory + user presets |
| M8 | 8.3 | Multi-plugin serial chain | 3+ plugins in series |
| M9 | 8.4 + 8.5 | Undo/redo + drag reorder | 10-step undo works |
| M10 | 8.6 | Cross-platform editors | Editors on 3 OS |
| M11 | 8.7 | Performance hardened | 0 xruns @ 64 samples |
| M12 | 8.8 | Broad compatibility | ≥95% plugin success |
| M13 | 8.9 | Distributable | CI + installers |

## Priority Order

1. **8.1 Plugin State Persistence** — Foundation for presets and session reliability
2. **8.2 Preset Management** — High user value, builds on 8.1
3. **8.3 Multi-Plugin Routing** — Core feature gap vs other hosts
4. **8.7 Performance Hardening** — Must be solid before broad release
5. **8.5 Drag-and-Drop Reordering** — UX polish, relatively quick win
6. **8.4 Undo/Redo** — Important for usability, moderate complexity
7. **8.8 Plugin Compatibility** — Broader testing, ongoing effort
8. **8.6 Cross-Platform Editors** — Platform expansion
9. **8.9 Distribution** — Final step before public release

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| IBStream compatibility | Plugins may expect specific seek/tell behavior | Test with multiple vendors; refer to Steinberg SDK reference implementation |
| Routing graph complexity | Large graphs may cause audio glitches | Start with simple serial chains; add parallel routing incrementally |
| Cross-platform editor embedding | X11/Wayland/Win32 are very different | Abstract behind trait; implement one platform at a time |
| Lock-free queue bugs | Subtle data races on audio thread | Extensive Miri + ASan testing; use proven implementations (crossbeam) |
| Plugin compatibility variance | Each plugin vendor has quirks | Build a plugin compatibility database; community testing |
| CI infrastructure cost | macOS CI runners are expensive | Use GitHub Actions macOS M1 runners; cache aggressively |

## Dependencies

| New Crate | Purpose | Sub-Phase |
|-----------|---------|-----------|
| `crossbeam` | Lock-free SPSC ring buffer | 8.7 |
| `criterion` | Benchmarks | 8.7 |
| `x11rb` or `xcb` | Linux X11 window management | 8.6 |
| `windows` | Windows Win32 API | 8.6 |
| `base64` | State blob encoding in session files | 8.1 |

---

## Implementation Notes

- Each sub-phase is designed to be independently shippable as a minor version bump
- Sub-phases 8.1 and 8.2 share the IBStream infrastructure and should be implemented together
- The routing graph (8.3) is the most architecturally significant change and should be designed carefully before implementation
- Performance hardening (8.7) should be done before broad distribution (8.9) to avoid shipping with known latency issues
- All sub-phases must maintain the existing test coverage bar (>80%) and add tests for new functionality
