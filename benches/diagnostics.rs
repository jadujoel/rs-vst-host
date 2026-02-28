//! Benchmarks for diagnostics — heap_check and malloc env detection.
//!
//! These measure the overhead of diagnostic functions that may be called
//! periodically during host operation.

use divan::Bencher;
use rs_vst_host::diagnostics;

fn main() {
    divan::main();
}

// ─── heap_check ────────────────────────────────────────────────────────────

#[divan::bench]
fn heap_check(bencher: Bencher) {
    bencher.bench(|| divan::black_box(diagnostics::heap_check()));
}

// ─── check_malloc_env ──────────────────────────────────────────────────────

#[divan::bench]
fn check_malloc_env(bencher: Bencher) {
    bencher.bench(|| divan::black_box(diagnostics::check_malloc_env()));
}

// ─── active_allocator_name ─────────────────────────────────────────────────

#[divan::bench]
fn active_allocator_name(bencher: Bencher) {
    bencher.bench(|| divan::black_box(diagnostics::active_allocator_name()));
}

// ─── recommended_env_vars ──────────────────────────────────────────────────

#[divan::bench]
fn recommended_env_vars(bencher: Bencher) {
    bencher.bench(|| divan::black_box(diagnostics::recommended_env_vars()));
}

// ─── heap_check throughput (multiple calls) ────────────────────────────────

#[divan::bench(args = [1, 10, 100])]
fn heap_check_repeated(bencher: Bencher, count: usize) {
    bencher.bench(|| {
        for _ in 0..count {
            divan::black_box(diagnostics::heap_check());
        }
    });
}
