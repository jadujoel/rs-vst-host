//! Audio device and stream management.
//!
//! This module handles:
//! - Audio device enumeration and selection via `cpal`
//! - Output stream setup with configurable sample rate, block size, channels
//! - Real-time audio callback processing engine
//! - Test tone generation for effect plugin testing
//! - Audio routing graph for multi-plugin processing chains

pub mod device;
pub mod engine;
pub mod graph;
