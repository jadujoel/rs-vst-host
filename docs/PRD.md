# Product Requirements Document (PRD)

## Product
rs-vst-host GUI application (Host UI for VST3 plugins)

## Purpose
Deliver a modern, dependable graphical host that makes scanning, loading, and controlling VST3 plugins fast and intuitive, while preserving real-time audio safety and the host's low-level VST3 compatibility goals.

## Background
The project already ships a CLI with full VST3 loading and audio/MIDI processing. Phase 7 introduced an initial GUI skeleton (plugin browser, rack, transport). This PRD defines the requirements to evolve the GUI into a production-ready host that integrates live audio, device routing, parameter control, plugin editors, and session management.

## Goals
- Provide a smooth visual workflow for scanning, browsing, loading, and managing VST3 plugins.
- Integrate the GUI with the real-time audio engine without sacrificing stability or performance.
- Enable parameter viewing and editing, including automation-ready controls.
- Support plugin editor windows where available.
- Persist and restore sessions (rack, routing, parameter states, transport).
- Maintain cross-platform compatibility (macOS, Linux, Windows).

## Non-Goals
- Building a full DAW (multi-track editing, timeline, audio recording).
- Implementing VST2 or AU hosting.
- Providing custom DSP effects or synthesis beyond test tone.
- Building a web-based UI or remote control surface in this phase.

## Target Users and Personas
- Music producers who want a lightweight VST3 host for quick plugin testing.
- Sound designers who need to audition effects or instruments with MIDI input.
- Plugin developers who want a simple, reliable host for validation.
- Power users who prefer a GUI over CLI for daily use.

## User Experience Principles
- Fast to scan, fast to load, fast to recover from errors.
- Controls should be obvious, minimal, and consistent.
- Styling should enhance clarity, not reduce readability.
- Avoid blocking UI actions that could impact audio processing.

## Key User Stories
1. As a user, I can scan for plugins and see new results immediately in the browser.
2. As a user, I can search by name/vendor/category and add a plugin to the rack in one click.
3. As a user, I can start audio processing and hear the plugin output with minimal setup.
4. As a user, I can select audio output and MIDI input devices from the GUI.
5. As a user, I can view and adjust parameters of the selected plugin.
6. As a user, I can open the plugin's native editor window if available.
7. As a user, I can save a session and restore it later with the same rack and settings.
8. As a user, I can see meaningful error messages when a plugin fails to load.

## Functional Requirements

### 1) Plugin Discovery
- Provide a "Scan Plugins" action that refreshes the cached plugin list.
- Display scan progress and summary status (modules scanned, classes found, errors).
- Allow scanning of default paths and optional user-specified paths.
- Cache results on disk; GUI should load from cache on startup.

### 2) Plugin Browser
- Display cached plugin classes with name, vendor, category, and bundle path.
- Provide text search and filter by category/subcategory/vendor.
- Allow single-click add-to-rack; prevent duplicates by default with an override option.
- Provide visible empty states (no cache, no match results).

### 3) Plugin Rack
- Each rack slot shows plugin name, vendor, status, bypass state, and remove action.
- Allow reordering slots via drag and drop (phase 2 of rack work).
- Allow per-slot enable/disable without removing.
- Show a selected slot state that drives the parameter panel.

### 4) Audio Engine Integration
- Start/stop audio processing from the GUI.
- Support test tone input (on by default) and silence input toggle.
- Display audio engine status (sample rate, buffer size, device name).
- Recover cleanly if a plugin returns an error in process.

### 5) Device Selection
- Provide audio output device selection, including default device.
- Provide MIDI input device selection with connection status.
- Allow changes while stopped; for running audio, prompt to restart stream.

### 6) Parameter Panel
- Display parameters for the selected plugin with name, value, and units.
- Support normalized and plain value editing where the plugin provides conversion.
- Provide a quick search within parameters.
- Show incoming parameter changes from the plugin UI or automation.

### 7) Plugin Editor Windows
- Provide a button to open the plugin's native editor if available.
- Support multiple plugin editors open at once.
- Ensure editor windows are closed on plugin removal or app exit.

### 8) Transport and Timing
- Provide play/pause, tempo, and time signature controls.
- Sync ProcessContext timing with transport state changes.
- Allow BPM and time signature changes while running.

### 9) Session Management
- Save session to a file (rack order, plugin class IDs, parameter states, transport).
- Load session and restore rack, parameters, and transport state.
- Handle missing plugins gracefully with clear errors.

### 10) Error Handling and Recovery
- Errors should be surfaced in the status area with actionable language.
- Plugin load failures should not crash the app; show failure reason.
- Provide a "safe mode" option to launch with no plugins loaded.

## Non-Functional Requirements
- Audio thread must remain real-time safe (no allocations or locks).
- GUI interactions must not block audio processing.
- Startup time under 2 seconds with cache available.
- Stable operation during long-running sessions (2+ hours).
- Consistent behavior across macOS, Linux, Windows.
- Maintain overall test coverage above 80%.

## UX and Visual Requirements
- Styling should enhance clarity, not reduce readability.
- Use subtle animation for panel transitions (no heavy motion).
- Preserve readability under bright and dark system themes.
- Support window resizing down to 1024x640 with responsive layout.

## Accessibility Requirements
- Minimum contrast ratio of 4.5:1 for text.
- Keyboard navigation for primary actions (scan, add, remove, play/pause).
- Visible focus states for interactive controls.

## Data and Storage
- Plugin cache stored in platform data directory (existing).
- Session files stored as JSON in user-selected locations.
- Consider versioning the session format for forward compatibility.

## Telemetry and Logging
- No telemetry by default.
- Optional debug logging via existing `RUST_LOG` config.

## Dependencies and Constraints
- GUI framework: `egui`/`eframe`.
- Audio/MIDI: `cpal`, `midir`.
- VST3 hosting: manual COM FFI implementation.
- VST3 editor embedding requires OS-specific window handling (per platform constraints).

## Risks
- Plugin editors may not embed cleanly across all platforms.
- Some plugins may misbehave under non-DAW hosts.
- Real-time audio stability could be affected by GUI-to-engine synchronization.
- Long plugin scans may freeze UI without careful threading.

## Milestones and Phases

### Phase 7 Step 2 (Short Term)
- Wire GUI to live audio engine (load and process).
- Device selection and transport integration.
- Parameter panel with read-only values.

### Phase 7 Step 3 (Mid Term)
- Parameter editing and automation-safe updates.
- Plugin editor windows.
- Session save/load.

### Phase 8 (Beyond MVP)
- Routing graph visualization.
- Multiple plugin chains and simple sends.
- Preset management and file browser.

## Success Metrics
- 95%+ successful plugin load rate from cache list.
- 0 audio dropouts during 30-minute test sessions on reference hardware.
- UI response time under 100 ms for common actions.
- Session save/load completes in under 1 second for 10 plugins.

## Open Questions
- Should the GUI allow multiple concurrent audio engines or a single global engine only, single engine.
- What is the desired default input source (test tone vs silence), test tone.
- How should plugin editor windows be managed on Linux, use Wayland.
- Do we want a "safe mode" launch toggle in the GUI or CLI only, cli only.

## Out of Scope for This PRD
- Networked control surfaces.
- Audio recording and export.
- Detailed performance profiling UI.
