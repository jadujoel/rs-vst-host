//! GUI module — Liquid Glass style interface using `egui` + `eframe`.
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

pub mod app;
pub mod backend;
pub mod editor;
pub mod session;
pub mod theme;

pub use app::launch;
