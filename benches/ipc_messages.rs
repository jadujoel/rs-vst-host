//! Benchmarks for IPC message serialization and deserialization.
//!
//! Measures encode/decode throughput for all key message types,
//! focusing on the hot-path Process / Processed messages.

use divan::Bencher;
use rs_vst_host::ipc::messages::*;

fn main() {
    divan::main();
}

// ─── Helpers ───────────────────────────────────────────────────────────────

fn make_process_msg(num_events: usize, num_params: usize) -> HostMessage {
    let events: Vec<MidiEvent> = (0..num_events)
        .map(|i| MidiEvent {
            sample_offset: i as i32,
            channel: 0,
            event_type: MidiEventType::NoteOn {
                pitch: 60 + (i % 12) as i16,
                velocity: 0.8,
            },
        })
        .collect();

    let param_changes: Vec<ParamChange> = (0..num_params)
        .map(|i| ParamChange {
            id: i as u32,
            sample_offset: 0,
            value: 0.5,
        })
        .collect();

    HostMessage::Process {
        num_samples: 512,
        events,
        param_changes,
        transport: TransportState::default(),
    }
}

fn make_param_info_list(count: usize) -> WorkerResponse {
    let params: Vec<ParamInfo> = (0..count)
        .map(|i| ParamInfo {
            id: i as u32,
            title: format!("Parameter {}", i),
            short_title: format!("P{}", i),
            units: "dB".to_string(),
            step_count: 0,
            default_normalized: 0.5,
            current_normalized: 0.5,
            can_automate: true,
            is_read_only: false,
            is_bypass: false,
        })
        .collect();

    WorkerResponse::Parameters { params }
}

// ─── encode_message — HostMessage::Process ─────────────────────────────────

#[divan::bench(args = [0, 4, 16, 64])]
fn encode_process_msg(bencher: Bencher, num_events: usize) {
    bencher
        .with_inputs(|| make_process_msg(num_events, 4))
        .bench_refs(|msg| encode_message(msg));
}

#[divan::bench(args = [0, 4, 16, 64])]
fn encode_process_msg_params(bencher: Bencher, num_params: usize) {
    bencher
        .with_inputs(|| make_process_msg(4, num_params))
        .bench_refs(|msg| encode_message(msg));
}

// ─── decode_message — HostMessage::Process ─────────────────────────────────

#[divan::bench(args = [0, 4, 16, 64])]
fn decode_process_msg(bencher: Bencher, num_events: usize) {
    bencher
        .with_inputs(|| {
            let msg = make_process_msg(num_events, 4);
            encode_message(&msg).unwrap()
        })
        .bench_refs(|bytes| {
            // Skip the 4-byte length prefix for decode
            let payload = &bytes[4..];
            divan::black_box(serde_json::from_slice::<HostMessage>(payload).unwrap());
        });
}

// ─── encode_message — WorkerResponse::Processed ───────────────────────────

#[divan::bench]
fn encode_processed_response(bencher: Bencher) {
    let msg = WorkerResponse::Processed;
    bencher.bench(|| encode_message(divan::black_box(&msg)));
}

// ─── encode_message — TransportState (small struct) ────────────────────────

#[divan::bench]
fn encode_transport_state(bencher: Bencher) {
    let state = TransportState::default();
    bencher.bench(|| encode_message(divan::black_box(&state)));
}

// ─── encode_message — HostMessage::LoadPlugin ──────────────────────────────

#[divan::bench]
fn encode_load_plugin(bencher: Bencher) {
    let msg = HostMessage::LoadPlugin {
        path: "/Library/Audio/Plug-Ins/VST3/FabFilter Pro-Q 4.vst3".to_string(),
        cid: [0x01; 16],
        name: "FabFilter Pro-Q 4".to_string(),
    };
    bencher.bench(|| encode_message(divan::black_box(&msg)));
}

// ─── encode_message — WorkerResponse::Parameters ──────────────────────────

#[divan::bench(args = [8, 32, 128])]
fn encode_param_list(bencher: Bencher, count: usize) {
    bencher
        .with_inputs(|| make_param_info_list(count))
        .bench_refs(|msg| encode_message(msg));
}

// ─── decode_message — WorkerResponse::Parameters ──────────────────────────

#[divan::bench(args = [8, 32, 128])]
fn decode_param_list(bencher: Bencher, count: usize) {
    bencher
        .with_inputs(|| {
            let msg = make_param_info_list(count);
            encode_message(&msg).unwrap()
        })
        .bench_refs(|bytes| {
            let payload = &bytes[4..];
            divan::black_box(serde_json::from_slice::<WorkerResponse>(payload).unwrap());
        });
}

// ─── encode_message — WorkerResponse::Crashed (with backtrace) ────────────

#[divan::bench]
fn encode_crash_report(bencher: Bencher) {
    let msg = WorkerResponse::Crashed {
        signal: "SIGSEGV".to_string(),
        context: "During process() call".to_string(),
        backtrace: (0..20)
            .map(|i| format!("frame #{}: 0x{:016x} in some_function", i, i * 0x1000))
            .collect(),
    };
    bencher.bench(|| encode_message(divan::black_box(&msg)));
}

// ─── Full roundtrip: encode → decode ───────────────────────────────────────

#[divan::bench(args = [0, 16, 64])]
fn roundtrip_process_msg(bencher: Bencher, num_events: usize) {
    bencher
        .with_inputs(|| make_process_msg(num_events, 4))
        .bench_refs(|msg| {
            let bytes = encode_message(msg).unwrap();
            let payload = &bytes[4..];
            divan::black_box(serde_json::from_slice::<HostMessage>(payload).unwrap());
        });
}
