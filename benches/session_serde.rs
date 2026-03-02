//! Benchmarks for Session serialization — save/load and serde roundtrip.

use divan::Bencher;
use rs_vst_host::gui::app::{PluginSlot, TransportState};
use rs_vst_host::gui::session::Session;
use std::path::PathBuf;

fn main() {
    divan::main();
}

// ─── Helpers ───────────────────────────────────────────────────────────────

fn sample_transport() -> TransportState {
    TransportState {
        playing: true,
        tempo: 140.0,
        time_sig_num: 3,
        time_sig_den: 4,
    }
}

fn sample_rack(count: usize) -> Vec<PluginSlot> {
    (0..count)
        .map(|i| PluginSlot {
            name: format!("Plugin {}", i),
            vendor: format!("Vendor {}", i),
            category: "Audio Module Class".to_string(),
            path: PathBuf::from(format!("/Library/Audio/Plug-Ins/VST3/Plugin{}.vst3", i)),
            cid: [i as u8; 16],
            bypassed: i % 3 == 0,
            param_cache: Vec::new(),
            staged_changes: Vec::new(),
            component_state: None,
            controller_state: None,
        })
        .collect()
}

fn sample_session(rack_size: usize) -> Session {
    Session::capture(
        &sample_transport(),
        &sample_rack(rack_size),
        Some("Speakers".to_string()),
        Some("MIDI Keyboard".to_string()),
    )
}

// ─── Session::capture ──────────────────────────────────────────────────────

#[divan::bench(args = [1, 4, 8, 16])]
fn capture(bencher: Bencher, rack_size: usize) {
    bencher
        .with_inputs(|| (sample_transport(), sample_rack(rack_size)))
        .bench_refs(|(transport, rack)| {
            Session::capture(
                transport,
                rack,
                Some("Speakers".to_string()),
                Some("MIDI Keyboard".to_string()),
            )
        });
}

// ─── Session::restore ──────────────────────────────────────────────────────

#[divan::bench(args = [1, 4, 8, 16])]
fn restore(bencher: Bencher, rack_size: usize) {
    bencher
        .with_inputs(|| sample_session(rack_size))
        .bench_refs(|session| session.restore());
}

// ─── Serde roundtrip (serialize + deserialize) ─────────────────────────────

#[divan::bench(args = [1, 4, 8, 16])]
fn serde_serialize(bencher: Bencher, rack_size: usize) {
    bencher
        .with_inputs(|| sample_session(rack_size))
        .bench_refs(|session| serde_json::to_string(session).unwrap());
}

#[divan::bench(args = [1, 4, 8, 16])]
fn serde_deserialize(bencher: Bencher, rack_size: usize) {
    bencher
        .with_inputs(|| {
            let session = sample_session(rack_size);
            serde_json::to_string(&session).unwrap()
        })
        .bench_refs(|json| serde_json::from_str::<Session>(json).unwrap());
}

#[divan::bench(args = [1, 4, 8, 16])]
fn serde_roundtrip(bencher: Bencher, rack_size: usize) {
    bencher
        .with_inputs(|| sample_session(rack_size))
        .bench_refs(|session| {
            let json = serde_json::to_string(session).unwrap();
            divan::black_box(serde_json::from_str::<Session>(&json).unwrap());
        });
}
