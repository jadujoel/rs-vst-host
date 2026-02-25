//! VST3 host context and transport.
//!
//! This module handles:
//! - Host context interfaces required by plugins (IHostApplication) — see `vst3::host_context`
//! - Parameter management via IEditController — see `vst3::params`
//! - Event list for MIDI/note events — see `vst3::event_list`
//! - Transport state (tempo, time signature, position) — planned for Phase 5

// TODO: Phase 5+ implementation
// - IComponentHandler for parameter change notifications
// - Transport/timing info
