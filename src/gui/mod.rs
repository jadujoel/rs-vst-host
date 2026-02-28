//! GUI module — Interface using `egui` + `eframe`.
//!
//! This module provides a graphical user interface for the VST3 host with:
//! - Plugin browser panel for scanning and loading plugins
//! - Plugin rack displaying loaded plugin instances as "glass cards"
//! - Transport controls (play/pause, tempo, time signature)
//! - Parameter display and editing for the active plugin
//! - Plugin editor windows for native plugin UIs
//! - Session save/load for persisting host state
//! - Audio/MIDI device selection
//! - Backend bridge for connecting GUI to audio engine
//!
//! # Process Architecture
//!
//! The GUI can run in two modes:
//!
//! 1. **In-process** (legacy): GUI and audio share a single process (`launch`).
//!    A plugin crash can corrupt the entire process.
//!
//! 2. **Separate-process** (default): A supervisor process manages audio/plugins
//!    and spawns the GUI in a child process (`launch_supervised`). If the GUI
//!    crashes, the supervisor relaunches it. Audio continues uninterrupted.

pub mod app;
pub mod audio_worker;
pub mod backend;
pub mod editor;
pub mod gui_worker;
pub mod ipc;
pub mod routing;
pub mod session;
pub mod supervisor;
pub mod theme;
pub mod undo;

/// Launch the GUI in-process (legacy mode). Blocks until window is closed.
pub use app::launch;

/// Launch the GUI in a supervised child process (crash-resilient mode).
/// The supervisor manages audio/plugins and relaunches the GUI on crash.
pub fn launch_supervised(
    safe_mode: bool,
    malloc_debug: bool,
    paths: Vec<std::path::PathBuf>,
) -> anyhow::Result<()> {
    supervisor::run_supervisor(safe_mode, malloc_debug, paths)
}
