//! GUI module — Liquid Glass style interface using `egui` + `eframe`.
//!
//! This module provides a graphical user interface for the VST3 host with:
//! - Plugin browser panel for scanning and loading plugins  
//! - Plugin rack displaying loaded plugin instances as "glass cards"
//! - Transport controls (play/pause, tempo, time signature)
//! - Parameter display and editing for the active plugin

pub mod app;
pub mod theme;

pub use app::launch;
