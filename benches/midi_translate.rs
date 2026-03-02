//! Benchmarks for MIDI → VST3 event translation.
//!
//! Measures per-event and batch translation from raw MIDI bytes to VST3 Events,
//! including the Event construction overhead.

use divan::Bencher;
use rs_vst_host::midi::device::RawMidiMessage;
use rs_vst_host::midi::translate::{midi_to_vst3_event, translate_midi_batch};

fn main() {
    divan::main();
}

// ─── Helper ────────────────────────────────────────────────────────────────

fn make_note_on(pitch: u8, velocity: u8) -> RawMidiMessage {
    RawMidiMessage {
        timestamp_us: 0,
        data: [0x90, pitch, velocity],
        len: 3,
    }
}

fn make_note_off(pitch: u8) -> RawMidiMessage {
    RawMidiMessage {
        timestamp_us: 0,
        data: [0x80, pitch, 64],
        len: 3,
    }
}

fn make_cc(cc: u8, value: u8) -> RawMidiMessage {
    RawMidiMessage {
        timestamp_us: 0,
        data: [0xB0, cc, value],
        len: 3,
    }
}

// ─── Single conversion ────────────────────────────────────────────────────

#[divan::bench]
fn translate_note_on(bencher: Bencher) {
    let msg = make_note_on(60, 100);
    bencher.bench(|| midi_to_vst3_event(divan::black_box(&msg), 0));
}

#[divan::bench]
fn translate_note_off(bencher: Bencher) {
    let msg = make_note_off(60);
    bencher.bench(|| midi_to_vst3_event(divan::black_box(&msg), 0));
}

#[divan::bench]
fn translate_note_on_vel0_as_off(bencher: Bencher) {
    let msg = make_note_on(60, 0); // vel=0 → note off
    bencher.bench(|| midi_to_vst3_event(divan::black_box(&msg), 0));
}

#[divan::bench]
fn translate_unsupported_cc(bencher: Bencher) {
    let msg = make_cc(1, 64);
    bencher.bench(|| midi_to_vst3_event(divan::black_box(&msg), 0));
}

// ─── Event construction ────────────────────────────────────────────────────

#[divan::bench]
fn event_note_on_construct(bencher: Bencher) {
    bencher.bench(|| {
        rs_vst_host::vst3::com::make_note_on_event(
            divan::black_box(0),
            divan::black_box(0),
            divan::black_box(60),
            divan::black_box(0.8),
            divan::black_box(-1),
        )
    });
}

#[divan::bench]
fn event_note_off_construct(bencher: Bencher) {
    bencher.bench(|| {
        rs_vst_host::vst3::com::make_note_off_event(
            divan::black_box(0),
            divan::black_box(0),
            divan::black_box(60),
            divan::black_box(0.0),
            divan::black_box(-1),
        )
    });
}

// ─── Batch translation ─────────────────────────────────────────────────────

#[divan::bench(args = [4, 16, 64, 128, 256])]
fn translate_batch_notes(bencher: Bencher, count: usize) {
    bencher
        .with_inputs(|| {
            (0..count)
                .map(|i| {
                    if i % 2 == 0 {
                        make_note_on(60 + (i % 12) as u8, 100)
                    } else {
                        make_note_off(60 + ((i - 1) % 12) as u8)
                    }
                })
                .collect::<Vec<_>>()
        })
        .bench_refs(|msgs| translate_midi_batch(msgs));
}

#[divan::bench(args = [16, 64, 256])]
fn translate_batch_mixed(bencher: Bencher, count: usize) {
    // Mix of notes and unsupported CC — tests filter path
    bencher
        .with_inputs(|| {
            (0..count)
                .map(|i| match i % 3 {
                    0 => make_note_on(60, 100),
                    1 => make_note_off(60),
                    _ => make_cc(1, 64),
                })
                .collect::<Vec<_>>()
        })
        .bench_refs(|msgs| translate_midi_batch(msgs));
}

// ─── MidiReceiver push + drain cycle ───────────────────────────────────────

#[divan::bench(args = [4, 32, 128])]
fn receiver_push_drain(bencher: Bencher, count: usize) {
    use rs_vst_host::midi::device::MidiReceiver;

    bencher
        .with_inputs(|| {
            let receiver = MidiReceiver::new();
            // Pre-build MIDI byte slices
            let messages: Vec<[u8; 3]> = (0..count)
                .map(|i| [0x90, 60 + (i % 12) as u8, 100])
                .collect();
            (receiver, messages)
        })
        .bench_local_refs(|(receiver, messages)| {
            for msg in messages.iter() {
                receiver.push(0, msg);
            }
            divan::black_box(receiver.drain());
        });
}
