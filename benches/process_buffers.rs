//! Benchmarks for VST3 ProcessBuffers — the per-block buffer management layer.
//!
//! Measures prepare (zero + pointer setup), interleave, and deinterleave
//! operations at typical channel counts and buffer sizes.

use divan::Bencher;
use rs_vst_host::vst3::process::ProcessBuffers;

fn main() {
    divan::main();
}

// ─── Construction ──────────────────────────────────────────────────────────

#[divan::bench(args = [64, 256, 512, 1024])]
fn new_stereo(bencher: Bencher, block_size: usize) {
    bencher.bench(|| ProcessBuffers::new(2, 2, divan::black_box(block_size)));
}

#[divan::bench(args = [256, 1024])]
fn new_8ch(bencher: Bencher, block_size: usize) {
    bencher.bench(|| ProcessBuffers::new(8, 8, divan::black_box(block_size)));
}

// ─── prepare() — zero output buffers, reset pointers ───────────────────────

#[divan::bench(args = [64, 128, 256, 512, 1024, 2048])]
fn prepare_stereo(bencher: Bencher, block_size: usize) {
    bencher
        .with_inputs(|| ProcessBuffers::new(2, 2, block_size))
        .bench_local_refs(|bufs| bufs.prepare(block_size));
}

#[divan::bench(args = [256, 512, 1024])]
fn prepare_8ch(bencher: Bencher, block_size: usize) {
    bencher
        .with_inputs(|| ProcessBuffers::new(8, 8, block_size))
        .bench_local_refs(|bufs| bufs.prepare(block_size));
}

// ─── write_input_interleaved — deinterleave from cpal ──────────────────────

#[divan::bench(args = [64, 128, 256, 512, 1024])]
fn write_input_interleaved_stereo(bencher: Bencher, block_size: usize) {
    bencher
        .with_inputs(|| {
            let mut bufs = ProcessBuffers::new(2, 2, block_size);
            bufs.prepare(block_size);
            let interleaved: Vec<f32> = (0..block_size * 2).map(|i| i as f32 * 0.001).collect();
            (bufs, interleaved)
        })
        .bench_local_refs(|(bufs, data)| bufs.write_input_interleaved(data, 2));
}

#[divan::bench(args = [256, 512, 1024])]
fn write_input_interleaved_8ch(bencher: Bencher, block_size: usize) {
    bencher
        .with_inputs(|| {
            let mut bufs = ProcessBuffers::new(8, 8, block_size);
            bufs.prepare(block_size);
            let interleaved: Vec<f32> = (0..block_size * 8).map(|i| i as f32 * 0.001).collect();
            (bufs, interleaved)
        })
        .bench_local_refs(|(bufs, data)| bufs.write_input_interleaved(data, 8));
}

// ─── read_output_interleaved — interleave for cpal ─────────────────────────

#[divan::bench(args = [64, 128, 256, 512, 1024])]
fn read_output_interleaved_stereo(bencher: Bencher, block_size: usize) {
    bencher
        .with_inputs(|| {
            let mut bufs = ProcessBuffers::new(2, 2, block_size);
            bufs.prepare(block_size);
            let output = vec![0.0f32; block_size * 2];
            (bufs, output)
        })
        .bench_local_refs(|(bufs, output)| bufs.read_output_interleaved(output, 2));
}

#[divan::bench(args = [256, 512, 1024])]
fn read_output_interleaved_8ch(bencher: Bencher, block_size: usize) {
    bencher
        .with_inputs(|| {
            let mut bufs = ProcessBuffers::new(8, 8, block_size);
            bufs.prepare(block_size);
            let output = vec![0.0f32; block_size * 8];
            (bufs, output)
        })
        .bench_local_refs(|(bufs, output)| bufs.read_output_interleaved(output, 8));
}

// ─── input_buffer_mut — per-channel access ─────────────────────────────────

#[divan::bench(args = [256, 512, 1024])]
fn input_buffer_mut_access(bencher: Bencher, block_size: usize) {
    bencher
        .with_inputs(|| {
            let mut bufs = ProcessBuffers::new(2, 2, block_size);
            bufs.prepare(block_size);
            bufs
        })
        .bench_local_refs(|bufs| {
            // Simulate filling both channels manually
            for ch in 0..2 {
                if let Some(buf) = bufs.input_buffer_mut(ch) {
                    for (i, sample) in buf.iter_mut().enumerate() {
                        *sample = i as f32 * 0.001;
                    }
                }
            }
        });
}

// ─── Full cycle: prepare + write + read ────────────────────────────────────

#[divan::bench(args = [128, 256, 512, 1024])]
fn full_cycle_stereo(bencher: Bencher, block_size: usize) {
    bencher
        .with_inputs(|| {
            let bufs = ProcessBuffers::new(2, 2, block_size);
            let input: Vec<f32> = (0..block_size * 2).map(|i| i as f32 * 0.001).collect();
            let output = vec![0.0f32; block_size * 2];
            (bufs, input, output)
        })
        .bench_local_refs(|(bufs, input, output)| {
            bufs.prepare(block_size);
            bufs.write_input_interleaved(input, 2);
            bufs.read_output_interleaved(output, 2);
        });
}
