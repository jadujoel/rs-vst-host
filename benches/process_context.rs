//! Benchmarks for ProcessContext — transport and timing state.
//!
//! Measures advance() (the per-block transport update), construction,
//! and combined advance+set operations.

use divan::Bencher;
use rs_vst_host::vst3::process_context::ProcessContext;

fn main() {
    divan::main();
}

// ─── Construction ──────────────────────────────────────────────────────────

#[divan::bench]
fn new_context(bencher: Bencher) {
    bencher.bench(|| ProcessContext::new(divan::black_box(44_100.0)));
}

// ─── advance() — called once per process block ─────────────────────────────

#[divan::bench(args = [64, 128, 256, 512, 1024])]
fn advance_single(bencher: Bencher, block_size: usize) {
    bencher
        .with_inputs(|| ProcessContext::new(44_100.0))
        .bench_local_refs(|ctx| ctx.advance(block_size as i32));
}

#[divan::bench(args = [128, 512])]
fn advance_single_96k(bencher: Bencher, block_size: usize) {
    bencher
        .with_inputs(|| ProcessContext::new(96_000.0))
        .bench_local_refs(|ctx| ctx.advance(block_size as i32));
}

// ─── Sustained advance — simulate running for N blocks ─────────────────────

#[divan::bench(args = [10, 100, 1000])]
fn advance_sustained(bencher: Bencher, num_blocks: usize) {
    bencher
        .with_inputs(|| ProcessContext::new(44_100.0))
        .bench_local_refs(|ctx| {
            for _ in 0..num_blocks {
                ctx.advance(512);
            }
        });
}

// ─── set_tempo ─────────────────────────────────────────────────────────────

#[divan::bench]
fn set_tempo(bencher: Bencher) {
    bencher
        .with_inputs(|| ProcessContext::new(44_100.0))
        .bench_local_refs(|ctx| ctx.set_tempo(divan::black_box(140.0)));
}

// ─── set_playing ───────────────────────────────────────────────────────────

#[divan::bench]
fn set_playing(bencher: Bencher) {
    bencher
        .with_inputs(|| ProcessContext::new(44_100.0))
        .bench_local_refs(|ctx| ctx.set_playing(divan::black_box(true)));
}

// ─── set_time_signature ────────────────────────────────────────────────────

#[divan::bench]
fn set_time_signature(bencher: Bencher) {
    bencher
        .with_inputs(|| ProcessContext::new(44_100.0))
        .bench_local_refs(|ctx| ctx.set_time_signature(divan::black_box(6), divan::black_box(8)));
}

// ─── Combined: set_playing + advance cycle ─────────────────────────────────

#[divan::bench(args = [128, 512])]
fn play_and_advance(bencher: Bencher, block_size: usize) {
    bencher
        .with_inputs(|| {
            let mut ctx = ProcessContext::new(44_100.0);
            ctx.set_playing(true);
            ctx.set_tempo(120.0);
            ctx
        })
        .bench_local_refs(|ctx| {
            ctx.advance(block_size as i32);
        });
}

// ─── as_ptr ────────────────────────────────────────────────────────────────

#[divan::bench]
fn as_ptr(bencher: Bencher) {
    bencher
        .with_inputs(|| ProcessContext::new(44_100.0))
        .bench_local_refs(|ctx| divan::black_box(ctx.as_ptr()));
}

// ─── Full block simulation: advance + set all fields ───────────────────────

#[divan::bench(args = [128, 512, 1024])]
fn full_block_update(bencher: Bencher, block_size: usize) {
    bencher
        .with_inputs(|| {
            let mut ctx = ProcessContext::new(44_100.0);
            ctx.set_playing(true);
            ctx.set_tempo(120.0);
            ctx.set_time_signature(4, 4);
            ctx
        })
        .bench_local_refs(|ctx| {
            ctx.advance(block_size as i32);
            divan::black_box(ctx.as_ptr());
        });
}
