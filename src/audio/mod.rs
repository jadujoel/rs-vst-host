//! Audio device and stream management.
//!
//! This module handles:
//! - Audio device enumeration and selection via `cpal`
//! - Output stream setup with configurable sample rate, block size, channels
//! - Real-time audio callback processing engine
//! - Test tone generation for effect plugin testing

pub mod device;
pub mod engine;
