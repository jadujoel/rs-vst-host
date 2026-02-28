# Phase 7 GUI Design Document

## 1. Introduction
This document outlines the design and architecture for the Phase 7 GUI of the `rs-vst-host` project. The primary focus is on implementing a modern, visually appealing interface using the "Glass" design language. This phase transitions the host from a purely CLI-based application to a fully-featured graphical application capable of hosting plugin editor windows, managing presets, and visualizing audio routing.

## 2. Design Philosophy:
The style is characterized by:
- **Translucency (Frosted Glass Effect):** Background blur (`backdrop-filter: blur`) to create a sense of depth and hierarchy.
- **Multi-layered Approach:** UI elements appear to float in space, overlapping each other.
- **Vivid Backgrounds:** Colorful, abstract, or dynamic backgrounds that highlight the blurred transparency of the foreground elements.
- **Subtle Borders:** Thin, semi-transparent light borders on translucent objects to define their edges.
- **Soft Shadows:** Diffused drop shadows to enhance the floating effect.

## 3. GUI Architecture
Given the Rust ecosystem, we need a GUI framework that supports advanced rendering techniques like background blur and custom shaders to achieve the Glass effect.

### 3.1 Framework Selection
- **Primary Candidate: `egui` with `wgpu` backend.** `egui` is immediate mode, highly performant, and integrates well with Rust audio applications. With a custom `wgpu` backend, we can implement the necessary shaders for the frosted glass effect.
- **Alternative: `iced`.** A reactive GUI library that also supports `wgpu` and might offer better layout primitives for complex routing graphs.
- **Alternative: `slint`.** A declarative GUI toolkit that compiles to native code, offering good performance and styling capabilities.

*Decision:* We will proceed with **`egui` + `wgpu`** due to its widespread adoption in the Rust audio community and the flexibility to write custom shaders for the Glass effect.

### 3.2 Core Components
1. **Main Window (The Workspace):**
   - A dynamic, subtly animated background (e.g., slow-moving gradients or abstract fluid shapes).
   - The main container for all floating panels.
2. **Plugin Rack / Routing Graph:**
   - A node-based editor or a vertical rack where each plugin instance is represented as a "glass card".
   - Connections between plugins (audio/MIDI routing) drawn as glowing, semi-transparent splines.
3. **Plugin Editor Host:**
   - A dedicated window or embedded panel that hosts the native VST3 plugin UI (HWND/NSView/X11 Window).
   - Wrapped in a Glass frame with host-provided controls (bypass, preset selection).
4. **Preset & Session Manager:**
   - A slide-out drawer or modal dialog with a frosted glass background for browsing presets and saving/loading sessions.
5. **Transport & Global Controls:**
   - A floating dock at the top or bottom containing play/pause, tempo, CPU usage, and master volume.

## 4. Implementing the Glass Effect in `egui`
To achieve the look in `egui`, we will need to extend its default rendering:

### 4.1 Custom Shaders
- We will implement a custom `wgpu` render pass that applies a Gaussian blur to the framebuffer region behind specific `egui` windows.
- This requires capturing the background texture before rendering the UI elements that need the frosted glass effect.

### 4.2 Styling `egui`
- **Fill Colors:** Use highly transparent colors (e.g., `Rgba::from_white_alpha(0.1)`).
- **Strokes:** Add a 1px border with a slightly higher opacity (e.g., `Rgba::from_white_alpha(0.3)`) to simulate the glass edge.
- **Shadows:** Configure `egui::epaint::Shadow` with a large blur radius and low opacity to create depth.
- **Rounding:** Use generous corner rounding (e.g., `Rounding::same(12.0)`) for a softer, modern look.

## 5. Phase 7 Feature Integration

### 5.1 Plugin Editor Window Support
- **Challenge:** Embedding native OS windows (VST3 editors) within a custom rendered GUI (`wgpu`).
- **Solution:** We will likely need to use a multi-window approach where the native plugin editor is a separate OS window, but we can style its non-client area (title bar) to match the host's theme, or use a transparent overlay window if supported by the OS.

### 5.2 Preset/Program Management
- A themed list view showing available presets.
- Search and filter functionality with smooth animations.

### 5.3 Multiple Instances & Routing Graph
- **Visual Representation:** Each plugin is a glass node.
- **Interaction:** Drag and drop to connect nodes. The routing lines should have a subtle glow effect.

### 5.4 Session Save/Load
- Serialize the routing graph, plugin states, and UI layout to a JSON or binary format.
- Provide visual feedback (e.g., a glass toast notification) upon successful save/load.

## 6. Next Steps
1. Set up a basic `egui` + `wgpu` project skeleton within the `rs-vst-host` workspace.
2. Develop the custom background blur shader for the Glass effect.
3. Create a prototype of the "glass card" UI component.
4. Integrate the existing VST3 loading logic to display loaded plugins as glass cards.
5. Tackle the native window embedding for plugin editors.
