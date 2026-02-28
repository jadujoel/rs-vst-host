//! Benchmarks for host_alloc — system malloc/free vs Box allocation.
//!
//! Measures the overhead of bypassing mimalloc for COM objects that must
//! live on the system malloc heap.

use divan::Bencher;
use rs_vst_host::vst3::host_alloc::{system_alloc, system_free};

fn main() {
    divan::main();
}

// ─── Small struct (typical COM object header) ──────────────────────────────

#[repr(C)]
struct SmallObj {
    vtbl: *const (),
    data: u64,
}

#[divan::bench]
fn system_alloc_small(bencher: Bencher) {
    bencher.bench(|| unsafe {
        let ptr = system_alloc(SmallObj {
            vtbl: std::ptr::null(),
            data: 42,
        });
        divan::black_box(ptr);
        system_free(ptr);
    });
}

#[divan::bench]
fn box_alloc_small(bencher: Bencher) {
    bencher.bench(|| {
        let b = Box::new(SmallObj {
            vtbl: std::ptr::null(),
            data: 42,
        });
        divan::black_box(&*b);
        drop(b);
    });
}

// ─── Medium struct (similar to HostComponentHandler) ───────────────────────

#[repr(C)]
struct MediumObj {
    vtbl: *const (),
    ptr_a: *mut (),
    ptr_b: *mut (),
    flags: u32,
    count: u32,
    data: [u8; 64],
}

#[divan::bench]
fn system_alloc_medium(bencher: Bencher) {
    bencher.bench(|| unsafe {
        let ptr = system_alloc(MediumObj {
            vtbl: std::ptr::null(),
            ptr_a: std::ptr::null_mut(),
            ptr_b: std::ptr::null_mut(),
            flags: 0,
            count: 0,
            data: [0u8; 64],
        });
        divan::black_box(ptr);
        system_free(ptr);
    });
}

#[divan::bench]
fn box_alloc_medium(bencher: Bencher) {
    bencher.bench(|| {
        let b = Box::new(MediumObj {
            vtbl: std::ptr::null(),
            ptr_a: std::ptr::null_mut(),
            ptr_b: std::ptr::null_mut(),
            flags: 0,
            count: 0,
            data: [0u8; 64],
        });
        divan::black_box(&*b);
        drop(b);
    });
}

// ─── Large struct (similar to HostParameterChanges with inline queues) ─────

#[repr(C)]
struct LargeObj {
    vtbl: *const (),
    data: [u8; 4096],
}

#[divan::bench]
fn system_alloc_large(bencher: Bencher) {
    bencher.bench(|| unsafe {
        let ptr = system_alloc(LargeObj {
            vtbl: std::ptr::null(),
            data: [0u8; 4096],
        });
        divan::black_box(ptr);
        system_free(ptr);
    });
}

#[divan::bench]
fn box_alloc_large(bencher: Bencher) {
    bencher.bench(|| {
        let b = Box::new(LargeObj {
            vtbl: std::ptr::null(),
            data: [0u8; 4096],
        });
        divan::black_box(&*b);
        drop(b);
    });
}

// ─── Alloc-only (no free) — measures pure allocation cost ──────────────────

#[divan::bench]
fn system_alloc_only_small(bencher: Bencher) {
    bencher.bench(|| unsafe {
        let ptr = system_alloc(SmallObj {
            vtbl: std::ptr::null(),
            data: 42,
        });
        // Intentional: free outside benchmark to isolate alloc cost
        // We must still free to avoid leaking
        divan::black_box(ptr);
        system_free(ptr);
    });
}

// ─── Repeated alloc/free (cache effects) ───────────────────────────────────

#[divan::bench(args = [1, 10, 100])]
fn system_alloc_free_batch(bencher: Bencher, count: usize) {
    bencher.bench(|| unsafe {
        let mut ptrs = Vec::with_capacity(count);
        for _ in 0..count {
            ptrs.push(system_alloc(SmallObj {
                vtbl: std::ptr::null(),
                data: 42,
            }));
        }
        for ptr in ptrs {
            system_free(ptr);
        }
    });
}

#[divan::bench(args = [1, 10, 100])]
fn box_alloc_free_batch(bencher: Bencher, count: usize) {
    bencher.bench(|| {
        let mut ptrs = Vec::with_capacity(count);
        for _ in 0..count {
            ptrs.push(Box::new(SmallObj {
                vtbl: std::ptr::null(),
                data: 42,
            }));
        }
        drop(ptrs);
    });
}
