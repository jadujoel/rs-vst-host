//! MIDI input handling and event routing.
//!
//! This module handles:
//! - MIDI device enumeration and selection (`device`)
//! - Real-time MIDI input capture via `midir`
//! - MIDI to VST3 event translation (`translate`)

pub mod device;
pub mod translate;
