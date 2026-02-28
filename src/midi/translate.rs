//! MIDI to VST3 event translation.
//!
//! Converts raw MIDI messages (from `midir`) into VST3 `Event` structs
//! suitable for the `IEventList` input.

use crate::midi::device::RawMidiMessage;
use crate::vst3::com::Event;
use tracing::debug;

// MIDI status byte masks
const STATUS_MASK: u8 = 0xF0;
const CHANNEL_MASK: u8 = 0x0F;

// MIDI message types
const NOTE_OFF: u8 = 0x80;
const NOTE_ON: u8 = 0x90;

/// Convert a raw MIDI message into a VST3 Event, if applicable.
///
/// Returns `None` for unsupported message types (CC, pitch bend, etc. — future work).
/// The `sample_offset` determines where in the current process block the event is placed.
pub fn midi_to_vst3_event(msg: &RawMidiMessage, sample_offset: i32) -> Option<Event> {
    if msg.len < 1 {
        return None;
    }

    let status = msg.data[0] & STATUS_MASK;
    let channel = (msg.data[0] & CHANNEL_MASK) as i16;

    match status {
        NOTE_ON if msg.len >= 3 => {
            let pitch = msg.data[1] as i16;
            let velocity_raw = msg.data[2];

            // Note On with velocity 0 is treated as Note Off (MIDI convention)
            if velocity_raw == 0 {
                debug!(channel, pitch, "MIDI Note Off (vel=0)");
                Some(Event::note_off(sample_offset, channel, pitch, 0.0, -1))
            } else {
                let velocity = velocity_raw as f32 / 127.0;
                debug!(channel, pitch, velocity = %format!("{:.2}", velocity), "MIDI Note On");
                Some(Event::note_on(sample_offset, channel, pitch, velocity, -1))
            }
        }
        NOTE_OFF if msg.len >= 3 => {
            let pitch = msg.data[1] as i16;
            let velocity = msg.data[2] as f32 / 127.0;
            debug!(channel, pitch, "MIDI Note Off");
            Some(Event::note_off(sample_offset, channel, pitch, velocity, -1))
        }
        _ => {
            // TODO: Phase 5+ — CC, pitch bend, aftertouch, etc.
            None
        }
    }
}

/// Convert a batch of raw MIDI messages into VST3 events.
///
/// All events are placed at `sample_offset` 0 (block start) since
/// midir timestamps don't directly map to sample offsets within a block.
/// Future work: interpolate timestamps to provide sample-accurate offsets.
pub fn translate_midi_batch(messages: &[RawMidiMessage]) -> Vec<Event> {
    let mut events = Vec::with_capacity(messages.len());
    for msg in messages {
        if let Some(event) = midi_to_vst3_event(msg, 0) {
            events.push(event);
        }
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_midi(data: &[u8]) -> RawMidiMessage {
        let mut msg = RawMidiMessage {
            timestamp_us: 0,
            data: [0; 3],
            len: data.len() as u8,
        };
        msg.data[..data.len()].copy_from_slice(data);
        msg
    }

    #[test]
    fn test_note_on() {
        let msg = make_midi(&[0x90, 60, 100]); // Note On, C4, vel 100
        let event = midi_to_vst3_event(&msg, 0).expect("should produce event");
        assert_eq!(event.event_type, crate::vst3::com::K_NOTE_ON_EVENT);
        assert_eq!(event.sample_offset, 0);

        let note: &crate::vst3::com::NoteOnEvent =
            unsafe { &*(event.data.as_ptr() as *const crate::vst3::com::NoteOnEvent) };
        assert_eq!(note.channel, 0);
        assert_eq!(note.pitch, 60);
        assert!((note.velocity - 100.0 / 127.0).abs() < 0.01);
    }

    #[test]
    fn test_note_off() {
        let msg = make_midi(&[0x80, 60, 64]); // Note Off, C4
        let event = midi_to_vst3_event(&msg, 128).expect("should produce event");
        assert_eq!(event.event_type, crate::vst3::com::K_NOTE_OFF_EVENT);
        assert_eq!(event.sample_offset, 128);

        let note: &crate::vst3::com::NoteOffEvent =
            unsafe { &*(event.data.as_ptr() as *const crate::vst3::com::NoteOffEvent) };
        assert_eq!(note.pitch, 60);
    }

    #[test]
    fn test_note_on_velocity_zero_is_note_off() {
        let msg = make_midi(&[0x90, 64, 0]); // Note On with vel 0 = Note Off
        let event = midi_to_vst3_event(&msg, 0).expect("should produce event");
        assert_eq!(event.event_type, crate::vst3::com::K_NOTE_OFF_EVENT);
    }

    #[test]
    fn test_channel_extraction() {
        // Note On on channel 5
        let msg = make_midi(&[0x95, 60, 100]);
        let event = midi_to_vst3_event(&msg, 0).expect("should produce event");

        let note: &crate::vst3::com::NoteOnEvent =
            unsafe { &*(event.data.as_ptr() as *const crate::vst3::com::NoteOnEvent) };
        assert_eq!(note.channel, 5);
    }

    #[test]
    fn test_unsupported_message_returns_none() {
        // Control Change (0xB0) - not yet supported
        let msg = make_midi(&[0xB0, 1, 64]);
        assert!(midi_to_vst3_event(&msg, 0).is_none());

        // Program Change (0xC0)
        let msg = make_midi(&[0xC0, 5]);
        assert!(midi_to_vst3_event(&msg, 0).is_none());
    }

    #[test]
    fn test_empty_message_returns_none() {
        let msg = RawMidiMessage {
            timestamp_us: 0,
            data: [0; 3],
            len: 0,
        };
        assert!(midi_to_vst3_event(&msg, 0).is_none());
    }

    #[test]
    fn test_translate_batch() {
        let messages = vec![
            make_midi(&[0x90, 60, 100]),  // Note On
            make_midi(&[0xB0, 1, 64]),    // CC (ignored)
            make_midi(&[0x80, 60, 0]),    // Note Off
        ];

        let events = translate_midi_batch(&messages);
        assert_eq!(events.len(), 2); // Only Note On + Note Off
    }

    #[test]
    fn test_velocity_range() {
        // Min velocity
        let msg = make_midi(&[0x90, 60, 1]);
        let event = midi_to_vst3_event(&msg, 0).unwrap();
        let note: &crate::vst3::com::NoteOnEvent =
            unsafe { &*(event.data.as_ptr() as *const crate::vst3::com::NoteOnEvent) };
        assert!(note.velocity > 0.0);
        assert!(note.velocity < 0.02);

        // Max velocity
        let msg = make_midi(&[0x90, 60, 127]);
        let event = midi_to_vst3_event(&msg, 0).unwrap();
        let note: &crate::vst3::com::NoteOnEvent =
            unsafe { &*(event.data.as_ptr() as *const crate::vst3::com::NoteOnEvent) };
        assert!((note.velocity - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_all_channels() {
        // Test all 16 MIDI channels (0x90-0x9F)
        for ch in 0..16u8 {
            let msg = make_midi(&[0x90 | ch, 60, 100]);
            let event = midi_to_vst3_event(&msg, 0).unwrap();
            let note: &crate::vst3::com::NoteOnEvent =
                unsafe { &*(event.data.as_ptr() as *const crate::vst3::com::NoteOnEvent) };
            assert_eq!(note.channel, ch as i16, "Channel {} mismatch", ch);
        }
    }

    #[test]
    fn test_all_pitches() {
        // Test extreme pitches
        for pitch in [0u8, 1, 60, 126, 127] {
            let msg = make_midi(&[0x90, pitch, 100]);
            let event = midi_to_vst3_event(&msg, 0).unwrap();
            let note: &crate::vst3::com::NoteOnEvent =
                unsafe { &*(event.data.as_ptr() as *const crate::vst3::com::NoteOnEvent) };
            assert_eq!(note.pitch, pitch as i16);
        }
    }

    #[test]
    fn test_note_off_velocity() {
        // Note Off with non-zero velocity
        let msg = make_midi(&[0x80, 64, 100]);
        let event = midi_to_vst3_event(&msg, 0).unwrap();
        let note: &crate::vst3::com::NoteOffEvent =
            unsafe { &*(event.data.as_ptr() as *const crate::vst3::com::NoteOffEvent) };
        assert!((note.velocity - 100.0 / 127.0).abs() < 0.01);
    }

    #[test]
    fn test_sample_offset_propagated() {
        let msg = make_midi(&[0x90, 60, 100]);
        let event = midi_to_vst3_event(&msg, 256).unwrap();
        assert_eq!(event.sample_offset, 256);
    }

    #[test]
    fn test_note_id_is_negative_one() {
        // All translated events should use note_id = -1 (unspecified)
        let msg = make_midi(&[0x90, 60, 100]);
        let event = midi_to_vst3_event(&msg, 0).unwrap();
        let note: &crate::vst3::com::NoteOnEvent =
            unsafe { &*(event.data.as_ptr() as *const crate::vst3::com::NoteOnEvent) };
        assert_eq!(note.note_id, -1);
    }

    #[test]
    fn test_truncated_note_on_message() {
        // Note on with only 2 bytes (missing velocity) — should return None
        let msg = RawMidiMessage {
            timestamp_us: 0,
            data: [0x90, 60, 0],
            len: 2,
        };
        assert!(midi_to_vst3_event(&msg, 0).is_none());
    }

    #[test]
    fn test_translate_batch_empty() {
        let events = translate_midi_batch(&[]);
        assert!(events.is_empty());
    }

    #[test]
    fn test_translate_batch_all_filtered() {
        // All unsupported messages
        let mut at = make_midi(&[0xD0, 100, 0]);
        at.len = 2;
        let messages = vec![
            make_midi(&[0xB0, 1, 64]),
            make_midi(&[0xE0, 0, 64]),
            at,
        ];
        let events = translate_midi_batch(&messages);
        assert!(events.is_empty());
    }

    #[test]
    fn test_translate_batch_preserves_order() {
        let messages = vec![
            make_midi(&[0x90, 60, 100]), // Note On C4
            make_midi(&[0x90, 64, 80]),  // Note On E4
            make_midi(&[0x80, 60, 0]),   // Note Off C4
        ];
        let events = translate_midi_batch(&messages);
        assert_eq!(events.len(), 3);

        // First should be note on C4
        let note0: &crate::vst3::com::NoteOnEvent =
            unsafe { &*(events[0].data.as_ptr() as *const crate::vst3::com::NoteOnEvent) };
        assert_eq!(note0.pitch, 60);

        // Second should be note on E4
        let note1: &crate::vst3::com::NoteOnEvent =
            unsafe { &*(events[1].data.as_ptr() as *const crate::vst3::com::NoteOnEvent) };
        assert_eq!(note1.pitch, 64);

        // Third should be note off C4
        assert_eq!(events[2].event_type, crate::vst3::com::K_NOTE_OFF_EVENT);
    }

    #[test]
    fn test_single_byte_message() {
        // System real-time (single byte) — should be unsupported
        let msg = RawMidiMessage {
            timestamp_us: 0,
            data: [0xFE, 0, 0], // Active Sensing
            len: 1,
        };
        assert!(midi_to_vst3_event(&msg, 0).is_none());
    }
}
