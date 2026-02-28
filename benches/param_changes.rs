//! Benchmarks for HostParameterChanges — the COM IParameterChanges implementation.
//!
//! Measures parameter change add, clear, and vtable access patterns
//! at various queue depths and parameter counts.

use divan::Bencher;
use rs_vst_host::vst3::param_changes::HostParameterChanges;

fn main() {
    divan::main();
}

// ─── Construction / destruction ────────────────────────────────────────────

#[divan::bench]
fn new_and_destroy(bencher: Bencher) {
    bencher.bench(|| unsafe {
        let pc = HostParameterChanges::new();
        HostParameterChanges::destroy(pc);
    });
}

// ─── add_change — single parameter, multiple points ────────────────────────

#[divan::bench(args = [1, 4, 8, 16])]
fn add_change_single_param(bencher: Bencher, points: usize) {
    bencher
        .with_inputs(HostParameterChanges::new)
        .bench_local_refs(|pc| unsafe {
            HostParameterChanges::clear(*pc);
            for i in 0..points {
                HostParameterChanges::add_change(*pc, 100, i as i32, i as f64 / points as f64);
            }
        });
}

// ─── add_change — multiple parameters, 1 point each ───────────────────────

#[divan::bench(args = [1, 4, 16, 32, 64])]
fn add_change_multi_params(bencher: Bencher, num_params: usize) {
    bencher
        .with_inputs(HostParameterChanges::new)
        .bench_local_refs(|pc| unsafe {
            HostParameterChanges::clear(*pc);
            for p in 0..num_params {
                HostParameterChanges::add_change(*pc, p as u32, 0, 0.5);
            }
        });
}

// ─── add_change — worst case: search through all existing queues ───────────

#[divan::bench(args = [8, 32, 64])]
fn add_change_last_param(bencher: Bencher, num_existing: usize) {
    bencher
        .with_inputs(|| {
            let pc = HostParameterChanges::new();
            // Pre-fill with N-1 different params so the Nth is a linear scan miss
            unsafe {
                for p in 0..num_existing {
                    HostParameterChanges::add_change(pc, p as u32, 0, 0.5);
                }
            }
            pc
        })
        .bench_local_refs(|pc| unsafe {
            // Add to the last param (forces full linear scan)
            HostParameterChanges::add_change(*pc, (num_existing - 1) as u32, 1, 0.75);
        });
}

// ─── clear ─────────────────────────────────────────────────────────────────

#[divan::bench(args = [0, 8, 32, 64])]
fn clear_with_params(bencher: Bencher, num_params: usize) {
    bencher
        .with_inputs(|| {
            let pc = HostParameterChanges::new();
            unsafe {
                for p in 0..num_params {
                    HostParameterChanges::add_change(pc, p as u32, 0, 0.5);
                }
            }
            pc
        })
        .bench_local_refs(|pc| unsafe {
            HostParameterChanges::clear(*pc);
        });
}

// ─── Full block cycle: add changes + clear ─────────────────────────────────

#[divan::bench(args = [4, 16, 32])]
fn block_cycle(bencher: Bencher, num_params: usize) {
    bencher
        .with_inputs(HostParameterChanges::new)
        .bench_local_refs(|pc| unsafe {
            for p in 0..num_params {
                HostParameterChanges::add_change(*pc, p as u32, 0, 0.5);
            }
            HostParameterChanges::clear(*pc);
        });
}

// ─── change_count ──────────────────────────────────────────────────────────

#[divan::bench(args = [0, 16, 64])]
fn change_count(bencher: Bencher, num_params: usize) {
    bencher
        .with_inputs(|| {
            let pc = HostParameterChanges::new();
            unsafe {
                for p in 0..num_params {
                    HostParameterChanges::add_change(pc, p as u32, 0, 0.5);
                }
            }
            pc
        })
        .bench_local_refs(|pc| unsafe {
            divan::black_box(HostParameterChanges::change_count(*pc));
        });
}
