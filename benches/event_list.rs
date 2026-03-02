//! Benchmarks for HostEventList — the COM IEventList implementation.
//!
//! Measures event add, clear, and get operations both via direct API
//! and through the COM vtable (as plugins would call them).

use divan::Bencher;
use rs_vst_host::vst3::com::{Event, IEventList, IEventListVtbl, make_note_on_event};
use rs_vst_host::vst3::event_list::HostEventList;

fn main() {
    divan::main();
}

/// Access the COM vtable from a HostEventList pointer.
///
/// HostEventList is #[repr(C)] with the vtable pointer as its first field,
/// matching C++ COM layout. This is the same cast a plugin would perform.
unsafe fn vtbl(list: *mut HostEventList) -> &'static IEventListVtbl {
    let vtbl_ptr = unsafe { *(list as *const *const IEventListVtbl) };
    unsafe { &*vtbl_ptr }
}

// ─── Construction / destruction ────────────────────────────────────────────

#[divan::bench]
fn new_and_destroy(bencher: Bencher) {
    bencher.bench(|| {
        let list = HostEventList::new();
        unsafe { HostEventList::destroy(list) };
    });
}

// ─── Add events (direct API) ──────────────────────────────────────────────

#[divan::bench(args = [1, 8, 32, 64, 128, 512])]
fn add_events(bencher: Bencher, count: usize) {
    bencher
        .with_inputs(|| {
            let list = HostEventList::new();
            let events: Vec<Event> = (0..count)
                .map(|i| make_note_on_event(i as i32, 0, 60 + (i % 12) as i16, 0.8, -1))
                .collect();
            (list, events)
        })
        .bench_local_refs(|(list, events)| unsafe {
            HostEventList::clear(*list);
            for event in events.iter() {
                HostEventList::add(*list, *event);
            }
        });
}

// ─── Clear ─────────────────────────────────────────────────────────────────

#[divan::bench(args = [0, 32, 128, 512])]
fn clear_with_events(bencher: Bencher, count: usize) {
    bencher
        .with_inputs(|| {
            let list = HostEventList::new();
            unsafe {
                for i in 0..count {
                    HostEventList::add(list, make_note_on_event(i as i32, 0, 60, 0.8, -1));
                }
            }
            list
        })
        .bench_local_refs(|list| unsafe {
            HostEventList::clear(*list);
        });
}

// ─── Add + clear cycle (simulates one process block) ───────────────────────

#[divan::bench(args = [4, 16, 64])]
fn add_clear_cycle(bencher: Bencher, events_per_block: usize) {
    bencher
        .with_inputs(|| {
            let list = HostEventList::new();
            let events: Vec<Event> = (0..events_per_block)
                .map(|i| make_note_on_event(i as i32, 0, 60 + (i % 12) as i16, 0.8, -1))
                .collect();
            (list, events)
        })
        .bench_local_refs(|(list, events)| unsafe {
            for event in events.iter() {
                HostEventList::add(*list, *event);
            }
            HostEventList::clear(*list);
        });
}

// ─── COM vtable: get_event_count ───────────────────────────────────────────

#[divan::bench(args = [0, 32, 128])]
fn vtable_get_event_count(bencher: Bencher, count: usize) {
    bencher
        .with_inputs(|| {
            let list = HostEventList::new();
            unsafe {
                for i in 0..count {
                    HostEventList::add(list, make_note_on_event(i as i32, 0, 60, 0.8, -1));
                }
            }
            list
        })
        .bench_local_refs(|list| unsafe {
            let vt = vtbl(*list);
            divan::black_box((vt.getEventCount)(*list as *mut IEventList));
        });
}

// ─── COM vtable: get_event ─────────────────────────────────────────────────

#[divan::bench(args = [4, 32, 128])]
fn vtable_get_all_events(bencher: Bencher, count: usize) {
    bencher
        .with_inputs(|| {
            let list = HostEventList::new();
            unsafe {
                for i in 0..count {
                    HostEventList::add(list, make_note_on_event(i as i32, 0, 60, 0.8, -1));
                }
            }
            list
        })
        .bench_local_refs(|list| unsafe {
            let vt = vtbl(*list);
            let n = (vt.getEventCount)(*list as *mut IEventList);
            let mut evt = std::mem::zeroed::<Event>();
            for i in 0..n {
                (vt.getEvent)(*list as *mut IEventList, i, &mut evt);
            }
            divan::black_box(&evt);
        });
}

// ─── COM vtable: add_event ─────────────────────────────────────────────────

#[divan::bench(args = [4, 32, 128])]
fn vtable_add_events(bencher: Bencher, count: usize) {
    bencher
        .with_inputs(|| {
            let list = HostEventList::new();
            let events: Vec<Event> = (0..count)
                .map(|i| make_note_on_event(i as i32, 0, 60, 0.8, -1))
                .collect();
            (list, events)
        })
        .bench_local_refs(|(list, events)| unsafe {
            HostEventList::clear(*list);
            let vt = vtbl(*list);
            for event in events.iter() {
                (vt.addEvent)(
                    *list as *mut IEventList,
                    event as *const Event as *mut Event,
                );
            }
        });
}
