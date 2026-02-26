# User Interaction Plan: Plugin Parameter Editing

**Status: Implemented** (v0.12.0 — 362 tests passing)

## Goal

When a user clicks a plugin in the GUI rack, the parameter panel should update to show that plugin's parameters and allow direct adjustment with clear feedback, safe defaults, and predictable state changes.

## Scope

- Applies to the GUI workflow only.
- Focuses on selecting a rack slot and editing parameters for the selected plugin.
- Covers both active and inactive plugins, and safe mode behavior.

## User Flow (Happy Path)

1. User clicks a plugin slot in the rack.
2. The selected slot is highlighted and becomes the current context.
3. The parameter panel switches to the selected plugin and requests its parameters (if not already cached).
4. Parameters are displayed with search, value formatting, and read-only/bypass cues.
5. User adjusts a parameter slider.
6. The change is sent to the audio engine, and the UI updates to reflect the applied value.

## UX Details

- Selection is a single click anywhere in the slot card.
- Selection is independent of activation; users can browse parameters for inactive plugins.
- When a plugin is active, parameter changes are applied in real time.
- When a plugin is inactive, parameter changes update the cached state and are applied on activation.
- The parameter panel includes a search field and a visual indicator for the selected plugin name and vendor.
- Read-only parameters are shown as text only (no slider).
- Bypass parameters are visually highlighted.
- Double-click resets to default value.

## States and Transitions

- No slot selected:
  - Parameter panel shows a placeholder message and hint to select a plugin.
- Slot selected, plugin inactive:
  - Parameters are shown, sliders enabled, changes staged.
- Slot selected, plugin active:
  - Parameters shown, sliders enabled, changes applied live.
- Safe mode on:
  - Editor button hidden; parameter editing remains available.
- Plugin removed:
  - If removed slot was selected, selection clears and panel resets.

## Data Flow

- UI selection -> GUI app state
- GUI app state -> backend parameter refresh
- Backend -> parameter snapshot cache
- UI slider change -> backend set_parameter
- Backend -> parameter change queue -> audio thread

## Backend Responsibilities

- Provide a stable parameter snapshot for the selected plugin.
- Apply parameter changes via the existing parameter changes queue.
- Cache parameter values for inactive plugins so edits persist until activation.
- Expose explicit error states (no active instance, parameter not found).

## GUI Responsibilities

- Track selected slot and request parameter snapshots when selection changes.
- Render parameter list with search filtering and value formatting.
- Show status messages for parameter apply failures.
- Avoid blocking the UI when fetching parameters; use cached snapshots when possible.

## Error Handling

- If parameter query fails, show a short error in the status bar and keep the last known snapshot.
- If apply fails (no active instance), show a non-blocking warning and keep the staged value.
- If plugin reports a parameter change concurrently, update the UI value to the latest reported value.

## Testing Plan

- Unit tests for selection state transitions (no slot -> slot -> removed).
- Unit tests for parameter snapshot refresh on selection change.
- Unit tests for staging parameter changes while inactive.
- Unit tests for live parameter application while active.
- Unit tests for error state messaging on failed parameter apply.

## Open Questions

- Should staged parameter edits be persisted in sessions for inactive plugins?
- Should parameter edits for inactive plugins trigger activation prompts?
- Should the parameter panel show per-parameter automation indicators?
