//! Benchmarks for the audio engine's test tone generator.
//!
//! Measures per-sample and per-block sine wave generation at various
//! buffer sizes and sample rates — the hottest path for pure-host audio.

use divan::Bencher;
use rs_vst_host::audio::engine::TestToneGenerator;

fn main() {
    divan::main();
}

// ─── Per-sample generation ─────────────────────────────────────────────────

#[divan::bench]
fn next_sample_44100(bencher: Bencher) {
    bencher
        .with_inputs(|| TestToneGenerator::new(44_100.0))
        .bench_local_refs(|tone| tone.next_sample());
}

#[divan::bench]
fn next_sample_96000(bencher: Bencher) {
    bencher
        .with_inputs(|| TestToneGenerator::new(96_000.0))
        .bench_local_refs(|tone| tone.next_sample());
}

// ─── fill_buffer at various block sizes ────────────────────────────────────

#[divan::bench(args = [64, 128, 256, 512, 1024, 2048, 4096])]
fn fill_buffer_44100(bencher: Bencher, block_size: usize) {
    bencher
        .with_inputs(|| {
            let tone = TestToneGenerator::new(44_100.0);
            let buf = vec![0.0f32; block_size];
            (tone, buf)
        })
        .bench_local_refs(|(tone, buf)| tone.fill_buffer(buf));
}

#[divan::bench(args = [64, 128, 256, 512, 1024, 2048])]
fn fill_buffer_96000(bencher: Bencher, block_size: usize) {
    bencher
        .with_inputs(|| {
            let tone = TestToneGenerator::new(96_000.0);
            let buf = vec![0.0f32; block_size];
            (tone, buf)
        })
        .bench_local_refs(|(tone, buf)| tone.fill_buffer(buf));
}

// ─── Construction ──────────────────────────────────────────────────────────

#[divan::bench]
fn new_default(bencher: Bencher) {
    bencher.bench(|| TestToneGenerator::new(divan::black_box(44_100.0)));
}

#[divan::bench]
fn with_params(bencher: Bencher) {
    bencher.bench(|| {
        TestToneGenerator::with_params(
            divan::black_box(48_000.0),
            divan::black_box(880.0),
            divan::black_box(0.5),
        )
    });
}

// ─── Sustained generation (simulate real-time callback) ────────────────────

#[divan::bench(args = [128, 512, 1024])]
fn sustained_10_blocks(bencher: Bencher, block_size: usize) {
    bencher
        .with_inputs(|| {
            let tone = TestToneGenerator::new(44_100.0);
            let buf = vec![0.0f32; block_size];
            (tone, buf)
        })
        .bench_local_refs(|(tone, buf)| {
            for _ in 0..10 {
                tone.fill_buffer(buf);
            }
        });
}
