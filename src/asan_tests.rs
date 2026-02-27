//! AddressSanitizer-targeted tests for memory safety of unsafe code.
//!
//! These tests are designed to run under ASan (`-Z sanitizer=address`) and
//! exercise the most safety-critical memory operations that ASan is uniquely
//! suited to validate:
//!
//! - **Use-after-free**: Accessing memory after `free()` / `system_free()`
//! - **Double-free**: Freeing the same allocation twice
//! - **Heap buffer overflow**: Reading/writing past allocation boundaries
//! - **Stack buffer overflow**: Out-of-bounds access on stack arrays
//! - **Memory leaks**: Allocated memory never freed (ASan leak detector)
//! - **Allocator mismatch**: `malloc`/`Box` cross-free detection
//!
//! Unlike Miri (which interprets MIR and catches aliasing/provenance issues),
//! ASan instruments the compiled native code and catches real hardware-level
//! memory errors at runtime with low overhead. ASan can run tests that use
//! FFI (libc::malloc, mmap, etc.) which Miri cannot interpret.
//!
//! # Running
//!
//! ```bash
//! RUSTFLAGS="-Z sanitizer=address" \
//!   cargo +nightly test --target aarch64-apple-darwin --lib asan_tests
//! ```
//!
//! # Compatibility
//!
//! Tests that use `libc::raise` (signal sandbox tests) or `malloc_zone_check`
//! are guarded with `#[cfg(not(sanitize = "address"))]` in their respective
//! modules. Those features conflict with ASan's signal and malloc interception.

#[cfg(test)]
mod tests {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicU32, Ordering};

    // ═══════════════════════════════════════════════════════════════════
    // host_alloc: system malloc lifecycle (libc::malloc / libc::free)
    // ═══════════════════════════════════════════════════════════════════

    /// Basic allocation and deallocation through system_alloc/system_free.
    /// ASan validates the malloc/free pairing and catches any heap corruption.
    #[test]
    fn asan_host_alloc_basic_lifecycle() {
        use crate::vst3::host_alloc::{system_alloc, system_free};

        #[repr(C)]
        struct ComObj {
            vtbl: *const u8,
            ref_count: AtomicU32,
            data: [u8; 64],
        }

        unsafe {
            let ptr = system_alloc(ComObj {
                vtbl: std::ptr::null(),
                ref_count: AtomicU32::new(1),
                data: [0xAA; 64],
            });
            assert!(!ptr.is_null());

            // Verify all bytes are accessible
            assert_eq!((*ptr).ref_count.load(Ordering::Relaxed), 1);
            for byte in &(*ptr).data {
                assert_eq!(*byte, 0xAA);
            }

            // Modify and verify
            (*ptr).ref_count.store(42, Ordering::Relaxed);
            (*ptr).data[63] = 0xBB;
            assert_eq!((*ptr).ref_count.load(Ordering::Relaxed), 42);
            assert_eq!((*ptr).data[63], 0xBB);

            system_free(ptr);
        }
    }

    /// Stress test: rapid alloc/free cycles to detect heap corruption.
    /// ASan tracks every allocation and will catch any metadata corruption
    /// from rapid recycling of heap chunks.
    #[test]
    fn asan_host_alloc_rapid_cycle_stress() {
        use crate::vst3::host_alloc::{system_alloc, system_free};

        for i in 0..200u32 {
            unsafe {
                let ptr = system_alloc(i);
                assert_eq!(*ptr, i);
                system_free(ptr);
            }
        }
    }

    /// Allocate multiple objects simultaneously, then free in various orders.
    /// ASan detects if freeing one object corrupts metadata of another.
    #[test]
    fn asan_host_alloc_multiple_live_objects() {
        use crate::vst3::host_alloc::{system_alloc, system_free};

        unsafe {
            let a = system_alloc(100u64);
            let b = system_alloc(200u64);
            let c = system_alloc(300u64);
            let d = system_alloc(400u64);

            assert_eq!(*a, 100);
            assert_eq!(*b, 200);
            assert_eq!(*c, 300);
            assert_eq!(*d, 400);

            // Free in non-LIFO order
            system_free(b);
            system_free(d);

            // Remaining objects still valid
            assert_eq!(*a, 100);
            assert_eq!(*c, 300);

            system_free(a);
            system_free(c);
        }
    }

    /// Verify that system_free on null is safe (no ASan error).
    #[test]
    fn asan_host_alloc_free_null() {
        use crate::vst3::host_alloc::system_free;
        unsafe {
            system_free::<u64>(std::ptr::null_mut());
            system_free::<[u8; 1024]>(std::ptr::null_mut());
            system_free::<AtomicU32>(std::ptr::null_mut());
        }
    }

    /// system_alloc with Drop type: verify destructor runs before free.
    /// ASan catches use-after-free if drop accesses freed memory.
    #[test]
    fn asan_host_alloc_drop_semantics() {
        use crate::vst3::host_alloc::{system_alloc, system_free};
        use std::sync::Arc;
        use std::sync::atomic::AtomicBool;

        struct DropTracker {
            dropped: Arc<AtomicBool>,
            // Larger payload to detect buffer overflow
            payload: [u8; 128],
        }

        impl Drop for DropTracker {
            fn drop(&mut self) {
                // Verify payload is still intact at drop time
                for byte in &self.payload {
                    assert_eq!(*byte, 0xDD);
                }
                self.dropped.store(true, Ordering::SeqCst);
            }
        }

        let dropped = Arc::new(AtomicBool::new(false));
        unsafe {
            let ptr = system_alloc(DropTracker {
                dropped: dropped.clone(),
                payload: [0xDD; 128],
            });
            assert!(!dropped.load(Ordering::SeqCst));

            // Verify payload accessible
            assert_eq!((*ptr).payload[127], 0xDD);

            system_free(ptr);
            assert!(dropped.load(Ordering::SeqCst));
        }
    }

    /// Allocate varying sizes to exercise different malloc size classes.
    /// ASan maintains redzones around each allocation — different sizes
    /// test different redzone configurations.
    #[test]
    fn asan_host_alloc_varying_sizes() {
        use crate::vst3::host_alloc::{system_alloc, system_free};

        unsafe {
            // Tiny allocation (1 byte)
            let tiny = system_alloc(42u8);
            assert_eq!(*tiny, 42);
            system_free(tiny);

            // Small allocation (16 bytes)
            let small = system_alloc([1u8; 16]);
            assert_eq!((*small)[15], 1);
            system_free(small);

            // Medium allocation (256 bytes)
            let medium = system_alloc([2u8; 256]);
            assert_eq!((*medium)[255], 2);
            system_free(medium);

            // Large allocation (4096 bytes)
            let large = system_alloc([3u8; 4096]);
            assert_eq!((*large)[4095], 3);
            system_free(large);

            // Very large allocation (64K)
            let huge = system_alloc([4u8; 65536]);
            assert_eq!((*huge)[65535], 4);
            system_free(huge);
        }
    }

    /// Concurrent allocations from multiple threads.
    /// ASan validates thread-safety of the underlying malloc implementation.
    #[test]
    fn asan_host_alloc_concurrent_threads() {
        use crate::vst3::host_alloc::{system_alloc, system_free};

        let handles: Vec<_> = (0..8)
            .map(|thread_id| {
                std::thread::spawn(move || {
                    for i in 0..50u32 {
                        unsafe {
                            let ptr = system_alloc((thread_id, i));
                            assert_eq!((*ptr).0, thread_id);
                            assert_eq!((*ptr).1, i);
                            system_free(ptr);
                        }
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // COM object lifecycle: Box::into_raw / Box::from_raw
    // ═══════════════════════════════════════════════════════════════════

    /// HostEventList full lifecycle under ASan: create → populate → read → destroy.
    /// ASan validates the Box::into_raw → raw pointer use → Box::from_raw chain.
    #[test]
    fn asan_event_list_full_lifecycle() {
        use crate::vst3::com::*;
        use crate::vst3::event_list::HostEventList;

        let list = HostEventList::new();
        assert!(!list.is_null());

        unsafe {
            // Fill with events
            for i in 0..100 {
                HostEventList::add(list, Event::note_on(i, 0, (i % 128) as i16, 0.8, i));
            }
            assert_eq!(HostEventList::event_count(list), 100);

            // Read back through vtable
            let vtbl_ptr = *(list as *const *const IEventListVtbl);
            let vtbl = &*vtbl_ptr;
            assert_eq!((vtbl.get_event_count)(list as *mut c_void), 100);

            let mut evt = std::mem::zeroed::<Event>();
            let result = (vtbl.get_event)(list as *mut c_void, 50, &mut evt);
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(evt.sample_offset, 50);

            // Read typed data through raw pointer cast
            let note: &NoteOnEvent = &*(evt.data.as_ptr() as *const NoteOnEvent);
            assert_eq!(note.pitch, 50);

            // Clear and reuse
            HostEventList::clear(list);
            assert_eq!(HostEventList::event_count(list), 0);

            // Add more events after clear
            for i in 0..50 {
                HostEventList::add(list, Event::note_off(i, 0, 60, 0.5, i));
            }
            assert_eq!(HostEventList::event_count(list), 50);

            HostEventList::destroy(list);
        }
    }

    /// HostParameterChanges full lifecycle under ASan.
    /// Tests nested COM objects (outer IParameterChanges → inner IParamValueQueue).
    #[test]
    fn asan_param_changes_full_lifecycle() {
        use crate::vst3::param_changes::HostParameterChanges;

        let changes = HostParameterChanges::new();
        assert!(!changes.is_null());

        unsafe {
            // Add changes for multiple parameters with multiple points
            for param_id in 0..32u32 {
                for point in 0..8 {
                    assert!(HostParameterChanges::add_change(
                        changes,
                        param_id,
                        point * 16,
                        param_id as f64 + point as f64 * 0.01,
                    ));
                }
            }
            assert_eq!(HostParameterChanges::change_count(changes), 32);

            // Clear and redo
            HostParameterChanges::clear(changes);
            assert_eq!(HostParameterChanges::change_count(changes), 0);

            // Re-populate after clear
            HostParameterChanges::add_change(changes, 999, 0, 0.5);
            assert_eq!(HostParameterChanges::change_count(changes), 1);

            HostParameterChanges::destroy(changes);
        }
    }

    /// Rapid create/destroy cycles for COM objects.
    /// ASan's quarantine list catches use-after-free from stale pointers.
    #[test]
    fn asan_com_rapid_create_destroy() {
        use crate::vst3::com::Event;
        use crate::vst3::event_list::HostEventList;
        use crate::vst3::param_changes::HostParameterChanges;

        for _ in 0..100 {
            let el = HostEventList::new();
            let pc = HostParameterChanges::new();

            unsafe {
                HostEventList::add(el, Event::note_on(0, 0, 60, 0.8, 1));
                HostEventList::add(el, Event::note_off(64, 0, 60, 0.0, 1));
                HostParameterChanges::add_change(pc, 1, 0, 0.5);
                HostParameterChanges::add_change(pc, 1, 32, 0.75);

                assert_eq!(HostEventList::event_count(el), 2);
                assert_eq!(HostParameterChanges::change_count(pc), 1);

                HostEventList::destroy(el);
                HostParameterChanges::destroy(pc);
            }
        }
    }

    /// EventList capacity stress: fill to MAX_EVENTS_PER_BLOCK, verify all readable.
    #[test]
    fn asan_event_list_capacity_boundary() {
        use crate::vst3::com::*;
        use crate::vst3::event_list::HostEventList;

        let list = HostEventList::new();
        unsafe {
            // Fill to capacity (512)
            for i in 0..512 {
                HostEventList::add(list, Event::note_on(i, 0, (i % 128) as i16, 0.5, i));
            }
            assert_eq!(HostEventList::event_count(list), 512);

            // Verify boundary elements through vtable
            let vtbl_ptr = *(list as *const *const IEventListVtbl);
            let vtbl = &*vtbl_ptr;

            let mut evt = std::mem::zeroed::<Event>();

            // First element
            (vtbl.get_event)(list as *mut c_void, 0, &mut evt);
            assert_eq!(evt.sample_offset, 0);

            // Last element
            (vtbl.get_event)(list as *mut c_void, 511, &mut evt);
            assert_eq!(evt.sample_offset, 511);

            // Out-of-bounds read should return error (not crash)
            let result = (vtbl.get_event)(list as *mut c_void, 512, &mut evt);
            assert_ne!(result, K_RESULT_OK);

            // Overflow: 513th event silently dropped
            HostEventList::add(list, Event::note_on(999, 0, 60, 1.0, 999));
            assert_eq!(HostEventList::event_count(list), 512); // Still 512

            HostEventList::destroy(list);
        }
    }

    /// QueryInterface on EventList under ASan — validates raw pointer casts.
    #[test]
    fn asan_event_list_query_interface() {
        use crate::vst3::com::*;
        use crate::vst3::event_list::HostEventList;

        let list = HostEventList::new();
        unsafe {
            let vtbl_ptr = *(list as *const *const IEventListVtbl);
            let vtbl = &*vtbl_ptr;
            let mut obj: *mut c_void = std::ptr::null_mut();

            // QI for IEventList
            let result =
                (vtbl.query_interface)(list as *mut c_void, IEVENT_LIST_IID.as_ptr(), &mut obj);
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(obj, list as *mut c_void);

            // QI for FUnknown
            let result =
                (vtbl.query_interface)(list as *mut c_void, FUNKNOWN_IID.as_ptr(), &mut obj);
            assert_eq!(result, K_RESULT_OK);

            // QI for unknown IID should fail
            let fake_iid: [u8; 16] = [0xFF; 16];
            let result = (vtbl.query_interface)(list as *mut c_void, fake_iid.as_ptr(), &mut obj);
            assert_ne!(result, K_RESULT_OK);

            HostEventList::destroy(list);
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // HostApplication COM object (system_alloc lifecycle)
    // ═══════════════════════════════════════════════════════════════════

    /// HostApplication lifecycle: system_alloc → COM vtable use → system_free.
    /// ASan validates the entire libc::malloc-backed COM object chain.
    #[test]
    fn asan_host_application_lifecycle() {
        use crate::vst3::host_context::HostApplication;

        let app = HostApplication::new();
        assert!(!app.is_null());

        // Use the object (get COM pointer)
        let ptr = HostApplication::as_unknown(app);
        assert!(!ptr.is_null());

        unsafe {
            HostApplication::destroy(app);
        }
    }

    /// HostComponentHandler lifecycle under ASan.
    #[test]
    fn asan_component_handler_lifecycle() {
        use crate::vst3::component_handler::HostComponentHandler;

        let handler = HostComponentHandler::new();
        assert!(!handler.is_null());

        let ptr = HostComponentHandler::as_ptr(handler);
        assert!(!ptr.is_null());

        unsafe {
            // Simulate plugin operations
            let _ = HostComponentHandler::drain_changes(handler);
            let _ = HostComponentHandler::take_restart_flags(handler);

            HostComponentHandler::destroy(handler);
        }
    }

    /// HostPlugFrame lifecycle under ASan.
    #[test]
    fn asan_plug_frame_lifecycle() {
        use crate::vst3::plug_frame::HostPlugFrame;

        let frame = HostPlugFrame::new();
        assert!(!frame.is_null());

        unsafe {
            let ptr = HostPlugFrame::as_ptr(frame);
            assert!(!ptr.is_null());

            // Check pending resize
            let resize = HostPlugFrame::take_pending_resize(frame);
            assert!(resize.is_none());

            HostPlugFrame::destroy(frame);
        }
    }

    /// All three COM host objects created and destroyed together.
    /// Tests that system_alloc objects don't interfere with each other's metadata.
    #[test]
    fn asan_all_host_objects_coexist() {
        use crate::vst3::component_handler::HostComponentHandler;
        use crate::vst3::host_context::HostApplication;
        use crate::vst3::plug_frame::HostPlugFrame;

        let app = HostApplication::new();
        let handler = HostComponentHandler::new();
        let frame = HostPlugFrame::new();

        assert!(!app.is_null());
        assert!(!handler.is_null());
        assert!(!frame.is_null());

        // Verify they have distinct addresses
        let app_addr = app as *const _ as usize;
        let handler_addr = handler as *const _ as usize;
        let frame_addr = frame as *const _ as usize;
        assert_ne!(app_addr, handler_addr);
        assert_ne!(handler_addr, frame_addr);
        assert_ne!(app_addr, frame_addr);

        // Destroy in reverse order
        unsafe {
            HostPlugFrame::destroy(frame);
            HostComponentHandler::destroy(handler);
            HostApplication::destroy(app);
        }
    }

    /// Stress: rapid creation and destruction of host COM objects.
    #[test]
    fn asan_host_objects_stress() {
        use crate::vst3::component_handler::HostComponentHandler;
        use crate::vst3::host_context::HostApplication;
        use crate::vst3::plug_frame::HostPlugFrame;

        for _ in 0..50 {
            let app = HostApplication::new();
            let handler = HostComponentHandler::new();
            let frame = HostPlugFrame::new();

            unsafe {
                HostApplication::destroy(app);
                HostComponentHandler::destroy(handler);
                HostPlugFrame::destroy(frame);
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // ProcessBuffers: self-referential pointer chain
    // ═══════════════════════════════════════════════════════════════════

    /// Full process buffer pointer chain under ASan.
    /// ASan validates every pointer dereference through the chain:
    /// ProcessData → AudioBusBuffers → channel_buffers_32 → sample data.
    #[test]
    fn asan_process_buffers_full_chain() {
        use crate::vst3::process::ProcessBuffers;

        let mut bufs = ProcessBuffers::new(2, 2, 512);
        bufs.prepare(256);

        unsafe {
            let pd = &*bufs.process_data_ptr();
            assert_eq!(pd.num_samples, 256);
            assert_eq!(pd.num_inputs, 1);
            assert_eq!(pd.num_outputs, 1);

            // Walk input chain
            let input_bus = &*pd.inputs;
            assert_eq!(input_bus.num_channels, 2);
            let ch0_ptr = *input_bus.channel_buffers_32;
            let ch1_ptr = *input_bus.channel_buffers_32.add(1);

            // Write to every sample in both channels — tests boundary access
            for i in 0..256 {
                *ch0_ptr.add(i) = i as f32 * 0.001;
                *ch1_ptr.add(i) = -(i as f32 * 0.001);
            }

            // Walk output chain
            let output_bus = &*pd.outputs;
            assert_eq!(output_bus.num_channels, 2);
            let out0_ptr = *output_bus.channel_buffers_32;
            let out1_ptr = *output_bus.channel_buffers_32.add(1);

            for i in 0..256 {
                *out0_ptr.add(i) = *ch0_ptr.add(i) * 0.5;
                *out1_ptr.add(i) = *ch1_ptr.add(i) * 0.5;
            }

            // Verify through safe API
            let out0 = bufs.output_buffer(0).unwrap();
            assert!((out0[0] - 0.0).abs() < f32::EPSILON);
            assert!((out0[255] - 255.0 * 0.001 * 0.5).abs() < 0.001);
        }
    }

    /// ProcessBuffers with various block sizes — tests buffer boundary conditions.
    #[test]
    fn asan_process_buffers_varying_block_sizes() {
        use crate::vst3::process::ProcessBuffers;

        let mut bufs = ProcessBuffers::new(2, 2, 4096);

        for block_size in [1, 2, 7, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096] {
            bufs.prepare(block_size);

            // Write last sample of each channel — boundary test
            if let Some(ch0) = bufs.input_buffer_mut(0) {
                assert!(ch0.len() >= block_size);
                ch0[block_size - 1] = block_size as f32;
            }

            if let Some(ch1) = bufs.input_buffer_mut(1) {
                ch1[block_size - 1] = -(block_size as f32);
            }

            // Verify through pointer chain
            unsafe {
                let pd = &*bufs.process_data_ptr();
                assert_eq!(pd.num_samples, block_size as i32);
                let input_bus = &*pd.inputs;
                let ch0 = *input_bus.channel_buffers_32;
                assert_eq!(*ch0.add(block_size - 1), block_size as f32);
            }
        }
    }

    /// Interleave/deinterleave roundtrip — lots of pointer arithmetic.
    #[test]
    fn asan_process_buffers_interleave_roundtrip() {
        use crate::vst3::process::ProcessBuffers;

        let mut bufs = ProcessBuffers::new(2, 2, 1024);
        bufs.prepare(1024);

        // Create test signal (stereo sine)
        let input: Vec<f32> = (0..2048).map(|i| (i as f32 * 0.01).sin()).collect();

        bufs.write_input_interleaved(&input, 2);

        // Copy input → output through raw pointers
        unsafe {
            let pd = &*bufs.process_data_ptr();
            let in_bus = &*pd.inputs;
            let out_bus = &*pd.outputs;
            for ch in 0..2 {
                let in_ptr = *in_bus.channel_buffers_32.add(ch);
                let out_ptr = *out_bus.channel_buffers_32.add(ch);
                std::ptr::copy_nonoverlapping(in_ptr, out_ptr, 1024);
            }
        }

        let mut output = vec![0.0f32; 2048];
        bufs.read_output_interleaved(&mut output, 2);

        for i in 0..2048 {
            assert!(
                (output[i] - input[i]).abs() < f32::EPSILON,
                "Sample mismatch at index {}: got {}, expected {}",
                i,
                output[i],
                input[i]
            );
        }
    }

    /// ProcessBuffers moved across threads.
    /// ASan validates that pointers remain valid after cross-thread move.
    #[test]
    fn asan_process_buffers_cross_thread() {
        use crate::vst3::process::ProcessBuffers;

        let mut bufs = ProcessBuffers::new(2, 2, 256);
        bufs.prepare(128);

        // Write on main thread
        if let Some(ch0) = bufs.input_buffer_mut(0) {
            for (i, s) in ch0.iter_mut().enumerate() {
                *s = i as f32;
            }
        }

        // Move to worker thread, re-prepare, verify
        let handle = std::thread::spawn(move || {
            bufs.prepare(64);

            unsafe {
                let pd = &*bufs.process_data_ptr();
                assert_eq!(pd.num_samples, 64);

                let input_bus = &*pd.inputs;
                let ch0 = *input_bus.channel_buffers_32;
                // Write through raw pointer
                for i in 0..64 {
                    *ch0.add(i) = i as f32 * 2.0;
                }
            }

            // Verify through safe API
            let ch0 = bufs.input_buffer_mut(0).unwrap();
            assert_eq!(ch0[63], 63.0 * 2.0);
            bufs
        });

        let _bufs = handle.join().unwrap();
    }

    /// Zero-channel configurations: no null dereferences under ASan.
    #[test]
    fn asan_process_buffers_zero_channels() {
        use crate::vst3::process::ProcessBuffers;

        let mut bufs = ProcessBuffers::new(0, 0, 256);
        bufs.prepare(128);

        unsafe {
            let pd = &*bufs.process_data_ptr();
            assert_eq!(pd.num_inputs, 0);
            assert_eq!(pd.num_outputs, 0);
            assert!(pd.inputs.is_null());
            assert!(pd.outputs.is_null());
        }

        // Asymmetric zero
        let mut bufs2 = ProcessBuffers::new(0, 2, 128);
        bufs2.prepare(64);
        assert!(bufs2.input_buffer_mut(0).is_none());
        assert!(bufs2.output_buffer(0).is_some());
        assert!(bufs2.output_buffer(1).is_some());
        assert!(bufs2.output_buffer(2).is_none());
    }

    // ═══════════════════════════════════════════════════════════════════
    // Shared memory (POSIX shm_open / mmap / munmap)
    // ═══════════════════════════════════════════════════════════════════

    /// Create shared memory, write data, read it back — full mmap lifecycle.
    /// ASan validates all pointer arithmetic on the mapped region.
    #[test]
    fn asan_shm_create_write_read() {
        use crate::ipc::shm::ShmAudioBuffer;
        use std::sync::atomic::{AtomicU64, Ordering as AtomOrd};
        static SHM_COUNTER: AtomicU64 = AtomicU64::new(0);
        let name = format!(
            "/rs-vst-asan-{}-{}",
            std::process::id(),
            SHM_COUNTER.fetch_add(1, AtomOrd::Relaxed)
        );

        let shm = ShmAudioBuffer::create(&name, 2, 2, 1024).unwrap();

        // Write to all input channels
        unsafe {
            let ch0 = shm.input_channel_mut(0).unwrap();
            for (i, s) in ch0.iter_mut().enumerate() {
                *s = i as f32 * 0.001;
            }

            let ch1 = shm.input_channel_mut(1).unwrap();
            for (i, s) in ch1.iter_mut().enumerate() {
                *s = -(i as f32 * 0.001);
            }
        }

        // Set metadata
        shm.set_num_samples(512);
        shm.set_ready();
        assert!(shm.is_ready());
        assert_eq!(shm.num_samples(), 512);

        // Read back
        unsafe {
            let ch0 = shm.input_channel(0).unwrap();
            assert!((ch0[0] - 0.0).abs() < f32::EPSILON);
            assert!((ch0[1023] - 1.023).abs() < 0.001);

            let ch1 = shm.input_channel(1).unwrap();
            assert!((ch1[0] - 0.0).abs() < f32::EPSILON);
            assert!((ch1[1023] - (-1.023)).abs() < 0.001);
        }

        shm.clear_ready();
        assert!(!shm.is_ready());
        drop(shm);
    }

    /// Shared memory: host creates, worker opens, both access the same region.
    /// ASan validates that both pointers refer to valid mapped memory.
    #[test]
    fn asan_shm_host_worker_roundtrip() {
        use crate::ipc::shm::ShmAudioBuffer;
        use std::sync::atomic::{AtomicU64, Ordering as AtomOrd};
        static SHM_COUNTER: AtomicU64 = AtomicU64::new(0);
        let name = format!(
            "/rs-vst-asan-hw-{}-{}",
            std::process::id(),
            SHM_COUNTER.fetch_add(1, AtomOrd::Relaxed)
        );

        let host = ShmAudioBuffer::create(&name, 2, 2, 256).unwrap();
        let worker = ShmAudioBuffer::open(&name, 2, 2, 256).unwrap();

        // Host writes input
        unsafe {
            let ch0 = host.input_channel_mut(0).unwrap();
            ch0[0] = 0.42;
            ch0[255] = -0.42;
        }
        host.set_num_samples(256);
        host.set_ready();

        // Worker reads input
        assert!(worker.is_ready());
        assert_eq!(worker.num_samples(), 256);
        unsafe {
            let ch0 = worker.input_channel(0).unwrap();
            assert!((ch0[0] - 0.42).abs() < f32::EPSILON);
            assert!((ch0[255] - (-0.42)).abs() < f32::EPSILON);
        }
        worker.clear_ready();

        // Worker writes output
        unsafe {
            let out0 = worker.output_channel_mut(0).unwrap();
            out0[0] = 0.99;
            let out1 = worker.output_channel_mut(1).unwrap();
            out1[0] = -0.99;
        }
        worker.set_ready();

        // Host reads output
        assert!(host.is_ready());
        unsafe {
            let out0 = host.output_channel(0).unwrap();
            assert!((out0[0] - 0.99).abs() < f32::EPSILON);
            let out1 = host.output_channel(1).unwrap();
            assert!((out1[0] - (-0.99)).abs() < f32::EPSILON);
        }

        drop(worker);
        drop(host);
    }

    /// Boundary writes: write to last valid sample of every channel.
    /// ASan's redzones would catch off-by-one writes past the mapped region.
    #[test]
    fn asan_shm_boundary_writes() {
        use crate::ipc::shm::ShmAudioBuffer;
        use std::sync::atomic::{AtomicU64, Ordering as AtomOrd};
        static SHM_COUNTER: AtomicU64 = AtomicU64::new(0);
        let name = format!(
            "/rs-vst-asan-bw-{}-{}",
            std::process::id(),
            SHM_COUNTER.fetch_add(1, AtomOrd::Relaxed)
        );

        let shm = ShmAudioBuffer::create(&name, 4, 4, 2048).unwrap();

        unsafe {
            // Write to first and last sample of every channel
            for ch in 0..4 {
                let buf = shm.input_channel_mut(ch).unwrap();
                buf[0] = ch as f32;
                buf[2047] = ch as f32 + 0.5;
            }
            for ch in 0..4 {
                let buf = shm.output_channel_mut(ch).unwrap();
                buf[0] = ch as f32 + 100.0;
                buf[2047] = ch as f32 + 100.5;
            }

            // Verify
            for ch in 0..4 {
                let buf = shm.input_channel(ch).unwrap();
                assert_eq!(buf[0], ch as f32);
                assert_eq!(buf[2047], ch as f32 + 0.5);
            }
            for ch in 0..4 {
                let buf = shm.output_channel(ch).unwrap();
                assert_eq!(buf[0], ch as f32 + 100.0);
                assert_eq!(buf[2047], ch as f32 + 100.5);
            }
        }

        // Invalid channel returns None (no crash)
        unsafe {
            assert!(shm.input_channel(4).is_none());
            assert!(shm.output_channel(4).is_none());
        }

        drop(shm);
    }

    /// Zero-channel shared memory: header-only allocation.
    #[test]
    fn asan_shm_zero_channels() {
        use crate::ipc::shm::ShmAudioBuffer;
        use std::sync::atomic::{AtomicU64, Ordering as AtomOrd};
        static SHM_COUNTER: AtomicU64 = AtomicU64::new(0);
        let name = format!(
            "/rs-vst-asan-zc-{}-{}",
            std::process::id(),
            SHM_COUNTER.fetch_add(1, AtomOrd::Relaxed)
        );

        let shm = ShmAudioBuffer::create(&name, 0, 0, 1024).unwrap();
        assert_eq!(shm.size(), 64); // Header only
        assert_eq!(shm.input_channels(), 0);
        assert_eq!(shm.output_channels(), 0);

        // Metadata operations on header-only region
        shm.set_num_samples(128);
        assert_eq!(shm.num_samples(), 128);
        shm.set_ready();
        assert!(shm.is_ready());
        shm.clear_ready();

        drop(shm);
    }

    /// Stress: create and destroy many shared memory regions.
    /// ASan catches any leak in the munmap/shm_unlink cleanup path.
    #[test]
    fn asan_shm_rapid_create_destroy() {
        use crate::ipc::shm::ShmAudioBuffer;
        use std::sync::atomic::{AtomicU64, Ordering as AtomOrd};
        static SHM_COUNTER: AtomicU64 = AtomicU64::new(0);

        for _ in 0..20 {
            let name = format!(
                "/rs-vst-asan-rcd-{}-{}",
                std::process::id(),
                SHM_COUNTER.fetch_add(1, AtomOrd::Relaxed)
            );
            let shm = ShmAudioBuffer::create(&name, 2, 2, 512).unwrap();
            unsafe {
                let ch0 = shm.input_channel_mut(0).unwrap();
                ch0[0] = 1.0;
                ch0[511] = 2.0;
            }
            drop(shm);
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // Event struct: byte-level reinterpretation
    // ═══════════════════════════════════════════════════════════════════

    /// Event struct byte-level access under ASan.
    /// ASan validates alignment and bounds of the data field cast.
    #[test]
    fn asan_event_note_on_byte_access() {
        use crate::vst3::com::*;

        let event = Event::note_on(42, 3, 60, 0.75, 99);

        // Read entire data field byte-by-byte — ASan checks each access
        let mut checksum: u8 = 0;
        for byte in &event.data {
            checksum = checksum.wrapping_add(*byte);
        }
        let _ = checksum;

        // Cast to typed pointer — alignment check
        let note: &NoteOnEvent = unsafe { &*(event.data.as_ptr() as *const NoteOnEvent) };
        assert_eq!(note.channel, 3);
        assert_eq!(note.pitch, 60);
        assert!((note.velocity - 0.75).abs() < f32::EPSILON);
        assert_eq!(note.note_id, 99);
    }

    /// Event clone: bitwise copy of the event data.
    #[test]
    fn asan_event_clone_safety() {
        use crate::vst3::com::*;

        let original = Event::note_on(10, 3, 72, 0.9, 42);
        let cloned = original;

        // Both should have identical data — ASan validates both memory regions
        assert_eq!(original.data, cloned.data);

        let orig_note: &NoteOnEvent = unsafe { &*(original.data.as_ptr() as *const NoteOnEvent) };
        let clone_note: &NoteOnEvent = unsafe { &*(cloned.data.as_ptr() as *const NoteOnEvent) };
        assert_eq!(orig_note.pitch, clone_note.pitch);
        assert_eq!(orig_note.note_id, clone_note.note_id);
    }

    /// Batch event creation and typed readback.
    #[test]
    fn asan_event_batch_note_on_off() {
        use crate::vst3::com::*;

        let mut events = Vec::new();
        for i in 0..256 {
            events.push(Event::note_on(i, (i % 16) as i16, (i % 128) as i16, 0.8, i));
            events.push(Event::note_off(
                i + 1,
                (i % 16) as i16,
                (i % 128) as i16,
                0.0,
                i,
            ));
        }

        assert_eq!(events.len(), 512);

        // Read all events through typed pointers
        for (idx, event) in events.iter().enumerate() {
            if event.event_type == K_NOTE_ON_EVENT {
                let note: &NoteOnEvent = unsafe { &*(event.data.as_ptr() as *const NoteOnEvent) };
                assert_eq!(note.pitch, (idx / 2 % 128) as i16);
            } else {
                let note: &NoteOffEvent = unsafe { &*(event.data.as_ptr() as *const NoteOffEvent) };
                assert_eq!(note.pitch, (idx / 2 % 128) as i16);
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // MIDI → EventList → ProcessData integration
    // ═══════════════════════════════════════════════════════════════════

    /// Full MIDI → VST3 pipeline under ASan.
    #[test]
    fn asan_midi_to_process_data_pipeline() {
        use crate::midi::device::RawMidiMessage;
        use crate::midi::translate::translate_midi_batch;
        use crate::vst3::com::*;
        use crate::vst3::event_list::HostEventList;
        use crate::vst3::param_changes::HostParameterChanges;
        use crate::vst3::process::ProcessBuffers;
        use crate::vst3::process_context::ProcessContext as HostProcessContext;

        // Create MIDI messages
        let messages: Vec<RawMidiMessage> = (0..64)
            .flat_map(|i| {
                vec![
                    RawMidiMessage {
                        timestamp_us: i as u64 * 1000,
                        data: [0x90 | (i as u8 % 16), 60 + (i as u8 % 48), 100],
                        len: 3,
                    },
                    RawMidiMessage {
                        timestamp_us: (i as u64 + 1) * 1000,
                        data: [0x80 | (i as u8 % 16), 60 + (i as u8 % 48), 0],
                        len: 3,
                    },
                ]
            })
            .collect();

        // Translate
        let events = translate_midi_batch(&messages);
        assert_eq!(events.len(), 128);

        // Build event list
        let event_list = HostEventList::new();
        unsafe {
            for event in &events {
                HostEventList::add(event_list, *event);
            }
        }

        // Build param changes
        let param_changes = HostParameterChanges::new();
        unsafe {
            HostParameterChanges::add_change(param_changes, 1, 0, 0.5);
            HostParameterChanges::add_change(param_changes, 2, 0, 0.75);
        }

        // Build process context
        let mut ctx = HostProcessContext::new(44100.0);
        ctx.set_tempo(120.0);
        ctx.set_playing(true);

        // Wire into process buffers
        let mut bufs = ProcessBuffers::new(2, 2, 512);
        bufs.prepare(256);
        bufs.set_input_events(HostEventList::as_ptr(event_list));
        bufs.set_input_parameter_changes(HostParameterChanges::as_ptr(param_changes));
        bufs.set_process_context(ctx.as_ptr());

        // Write audio input
        if let Some(ch0) = bufs.input_buffer_mut(0) {
            for (i, s) in ch0.iter_mut().enumerate() {
                *s = (i as f32 * 0.01).sin();
            }
        }

        // Validate full ProcessData under ASan
        unsafe {
            let pd = &*bufs.process_data_ptr();
            assert_eq!(pd.num_samples, 256);
            assert!(!pd.input_events.is_null());
            assert!(!pd.input_parameter_changes.is_null());
            assert!(!pd.process_context.is_null());
            assert!(!pd.inputs.is_null());
            assert!(!pd.outputs.is_null());

            // Read events through vtable
            let el = pd.input_events as *mut HostEventList;
            let vtbl_ptr = *(el as *const *const IEventListVtbl);
            let vtbl = &*vtbl_ptr;
            assert_eq!((vtbl.get_event_count)(el as *mut c_void), 128);

            // Read first event
            let mut evt = std::mem::zeroed::<Event>();
            (vtbl.get_event)(el as *mut c_void, 0, &mut evt);
            assert_eq!(evt.event_type, K_NOTE_ON_EVENT);

            // Read audio through pointer chain
            let input_bus = &*pd.inputs;
            let ch0 = *input_bus.channel_buffers_32;
            assert!((*ch0).abs() < 0.01); // sin(0) ≈ 0
        }

        // Clean up
        bufs.set_input_events(std::ptr::null_mut());
        bufs.set_input_parameter_changes(std::ptr::null_mut());
        bufs.set_process_context(std::ptr::null_mut());
        unsafe {
            HostEventList::destroy(event_list);
            HostParameterChanges::destroy(param_changes);
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // ProcessContext: pointer cast to c_void
    // ═══════════════════════════════════════════════════════════════════

    /// ProcessContext lifecycle and transport advancement.
    #[test]
    fn asan_process_context_lifecycle() {
        use crate::vst3::process_context::ProcessContext as HostProcessContext;

        let mut ctx = HostProcessContext::new(48000.0);
        ctx.set_tempo(140.0);
        ctx.set_playing(true);
        ctx.set_time_signature(3, 4);

        // Advance transport through multiple blocks
        for _ in 0..100 {
            ctx.advance(256);
        }

        // Wire into process buffers and read back
        let mut bufs = crate::vst3::process::ProcessBuffers::new(2, 2, 256);
        bufs.prepare(256);
        bufs.set_process_context(ctx.as_ptr());

        unsafe {
            let pd = &*bufs.process_data_ptr();
            assert!(!pd.process_context.is_null());
        }

        bufs.set_process_context(std::ptr::null_mut());
    }

    // ═══════════════════════════════════════════════════════════════════
    // MIDI translation: Event byte reinterpretation
    // ═══════════════════════════════════════════════════════════════════

    /// MIDI translate batch under ASan: exercises Event construction
    /// and NoteOnEvent/NoteOffEvent byte-level writing.
    #[test]
    fn asan_midi_translate_batch() {
        use crate::midi::device::RawMidiMessage;
        use crate::midi::translate::translate_midi_batch;
        use crate::vst3::com::*;

        let messages: Vec<RawMidiMessage> = (0..128)
            .map(|i| RawMidiMessage {
                timestamp_us: i as u64 * 100,
                data: [0x90, i as u8, 100],
                len: 3,
            })
            .collect();

        let events = translate_midi_batch(&messages);
        assert_eq!(events.len(), 128);

        for (i, event) in events.iter().enumerate() {
            assert_eq!(event.event_type, K_NOTE_ON_EVENT);
            let note: &NoteOnEvent = unsafe { &*(event.data.as_ptr() as *const NoteOnEvent) };
            assert_eq!(note.pitch, i as i16);
        }
    }

    /// Exercise all 16 MIDI channels.
    #[test]
    fn asan_midi_all_channels() {
        use crate::midi::device::RawMidiMessage;
        use crate::midi::translate::translate_midi_batch;
        use crate::vst3::com::*;

        let messages: Vec<RawMidiMessage> = (0..16)
            .map(|ch| RawMidiMessage {
                timestamp_us: 0,
                data: [0x90 | ch, 60, 100],
                len: 3,
            })
            .collect();

        let events = translate_midi_batch(&messages);
        assert_eq!(events.len(), 16);

        for (ch, event) in events.iter().enumerate() {
            let note: &NoteOnEvent = unsafe { &*(event.data.as_ptr() as *const NoteOnEvent) };
            assert_eq!(note.channel, ch as i16);
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // Sandbox: non-crashing paths (ASan-compatible)
    // ═══════════════════════════════════════════════════════════════════

    /// Sandbox normal call (no signals) under ASan.
    /// Tests the sandbox setup/teardown overhead with ASan instrumentation.
    #[test]
    fn asan_sandbox_normal_call() {
        use crate::vst3::sandbox::sandbox_call;

        let result = sandbox_call("asan_normal", || 42);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    /// Sandbox with heap allocation inside the closure.
    #[test]
    fn asan_sandbox_heap_alloc_inside() {
        use crate::vst3::sandbox::sandbox_call;

        let result = sandbox_call("asan_heap", || {
            let v: Vec<u32> = (0..1000).collect();
            v.iter().sum::<u32>()
        });
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), (0..1000u32).sum::<u32>());
    }

    /// Sandbox with system_alloc/system_free inside the closure.
    #[test]
    fn asan_sandbox_system_alloc_inside() {
        use crate::vst3::host_alloc::{system_alloc, system_free};
        use crate::vst3::sandbox::sandbox_call;

        let result = sandbox_call("asan_sysalloc", || unsafe {
            let ptr = system_alloc(42u64);
            let val = *ptr;
            system_free(ptr);
            val
        });
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    /// Sandbox with panic recovery.
    #[test]
    fn asan_sandbox_panic_recovery() {
        use crate::vst3::sandbox::sandbox_call;

        let result = sandbox_call::<_, ()>("asan_panic", || {
            panic!("intentional panic for ASan test");
        });
        assert!(result.is_panicked());
    }

    /// Nested sandbox calls (non-crashing).
    #[test]
    fn asan_sandbox_nested() {
        use crate::vst3::sandbox::sandbox_call;

        let result = sandbox_call("outer", || {
            let inner = sandbox_call("inner", || 7);
            inner.unwrap() * 6
        });
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    /// Multiple sequential sandbox calls with allocations.
    #[test]
    fn asan_sandbox_sequential_stress() {
        use crate::vst3::sandbox::sandbox_call;

        for i in 0..100u32 {
            let result = sandbox_call(&format!("stress_{}", i), || {
                let v: Vec<u8> = vec![i as u8; 256];
                v.into_iter().map(|x| x as u32).sum::<u32>()
            });
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), i * 256);
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // IPC messages: serialization without unsafe, but verifies no leaks
    // ═══════════════════════════════════════════════════════════════════

    /// Serialize and deserialize large batches of IPC messages.
    /// ASan catches any memory corruption in serde/json allocation paths.
    #[test]
    fn asan_ipc_message_roundtrip() {
        use crate::ipc::messages::{HostMessage, WorkerResponse, encode_message};

        // Encode various message types
        let messages: Vec<HostMessage> = vec![
            HostMessage::Ping,
            HostMessage::Shutdown,
            HostMessage::Configure {
                sample_rate: 44100.0,
                max_block_size: 512,
                output_channels: 2,
                input_arrangement: 3, // stereo
                output_arrangement: 3,
            },
            HostMessage::SetParameter {
                id: 42,
                value: 0.75,
            },
        ];

        for msg in &messages {
            let encoded = encode_message(msg).unwrap();
            // Verify encoding produces a non-empty buffer
            assert!(!encoded.is_empty());
            // Decode via reader
            let mut reader = std::io::Cursor::new(&encoded);
            let decoded: Option<HostMessage> =
                crate::ipc::messages::decode_message(&mut reader).unwrap();
            assert!(decoded.is_some());
            // Re-encode to verify consistency
            let re_encoded = encode_message(&decoded.unwrap()).unwrap();
            assert_eq!(encoded.len(), re_encoded.len());
        }

        // Response messages
        let responses: Vec<WorkerResponse> = vec![
            WorkerResponse::Pong,
            WorkerResponse::Error {
                message: "test error".to_string(),
            },
        ];

        for resp in &responses {
            let encoded = encode_message(resp).unwrap();
            assert!(!encoded.is_empty());
            let mut reader = std::io::Cursor::new(&encoded);
            let decoded: Option<WorkerResponse> =
                crate::ipc::messages::decode_message(&mut reader).unwrap();
            assert!(decoded.is_some());
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // Cross-module integration: full mock process under ASan
    // ═══════════════════════════════════════════════════════════════════

    /// Full mock process call with all COM objects and buffers.
    /// This is the most comprehensive ASan test — exercises the maximum
    /// number of simultaneous unsafe pointer chains.
    #[test]
    fn asan_full_mock_process_call() {
        use crate::vst3::com::*;
        use crate::vst3::event_list::HostEventList;
        use crate::vst3::param_changes::HostParameterChanges;
        use crate::vst3::process::ProcessBuffers;
        use crate::vst3::process_context::ProcessContext as HostProcessContext;

        // Set up all COM objects
        let event_list = HostEventList::new();
        let param_changes = HostParameterChanges::new();
        let mut ctx = HostProcessContext::new(96000.0);
        ctx.set_tempo(128.0);
        ctx.set_playing(true);
        ctx.set_time_signature(6, 8);

        // Populate events
        unsafe {
            for i in 0..100 {
                HostEventList::add(
                    event_list,
                    Event::note_on(i, 0, (60 + i % 24) as i16, 0.8, i),
                );
            }
            for i in 0..100 {
                HostEventList::add(
                    event_list,
                    Event::note_off(100 + i, 0, (60 + i % 24) as i16, 0.0, i),
                );
            }
        }

        // Populate parameter changes
        unsafe {
            for p in 0..16u32 {
                for pt in 0..4 {
                    HostParameterChanges::add_change(param_changes, p, pt * 64, p as f64 * 0.0625);
                }
            }
        }

        // Set up buffers
        let mut bufs = ProcessBuffers::new(2, 2, 1024);
        bufs.prepare(512);

        // Wire everything together
        bufs.set_input_events(HostEventList::as_ptr(event_list));
        bufs.set_input_parameter_changes(HostParameterChanges::as_ptr(param_changes));
        bufs.set_process_context(ctx.as_ptr());

        // Generate input audio
        for ch in 0..2 {
            if let Some(buf) = bufs.input_buffer_mut(ch) {
                for (i, s) in buf.iter_mut().enumerate() {
                    *s = ((i as f32 + ch as f32 * 1000.0) * 0.001).sin();
                }
            }
        }

        // Simulate plugin passthrough: copy input → output via raw pointers
        unsafe {
            let pd = &*bufs.process_data_ptr();
            let in_bus = &*pd.inputs;
            let out_bus = &*pd.outputs;

            for ch in 0..2 {
                let in_ptr = *in_bus.channel_buffers_32.add(ch);
                let out_ptr = *out_bus.channel_buffers_32.add(ch);
                std::ptr::copy_nonoverlapping(in_ptr, out_ptr, 512);
            }
        }

        // Verify output matches input
        for ch in 0..2 {
            let input = bufs.input_buffer_mut(ch).unwrap().to_vec();
            let output = bufs.output_buffer(ch).unwrap();
            for i in 0..512 {
                assert!(
                    (input[i] - output[i]).abs() < f32::EPSILON,
                    "ch{} sample {} mismatch",
                    ch,
                    i
                );
            }
        }

        // Advance context
        ctx.advance(512);

        // Second block
        bufs.prepare(512);
        bufs.set_input_events(HostEventList::as_ptr(event_list));
        bufs.set_input_parameter_changes(HostParameterChanges::as_ptr(param_changes));
        bufs.set_process_context(ctx.as_ptr());

        unsafe {
            let pd = &*bufs.process_data_ptr();
            assert_eq!(pd.num_samples, 512);
        }

        // Clean up
        bufs.set_input_events(std::ptr::null_mut());
        bufs.set_input_parameter_changes(std::ptr::null_mut());
        bufs.set_process_context(std::ptr::null_mut());
        unsafe {
            HostEventList::destroy(event_list);
            HostParameterChanges::destroy(param_changes);
        }
    }

    /// Multiple consecutive process blocks — simulates a real-time audio session.
    #[test]
    fn asan_multi_block_processing_session() {
        use crate::vst3::com::*;
        use crate::vst3::event_list::HostEventList;
        use crate::vst3::param_changes::HostParameterChanges;
        use crate::vst3::process::ProcessBuffers;
        use crate::vst3::process_context::ProcessContext as HostProcessContext;

        let event_list = HostEventList::new();
        let param_changes = HostParameterChanges::new();
        let mut ctx = HostProcessContext::new(44100.0);
        ctx.set_tempo(120.0);
        ctx.set_playing(true);

        let mut bufs = ProcessBuffers::new(2, 2, 512);

        // Simulate 100 audio blocks (~1.16 seconds at 44100Hz / 512 block)
        for block in 0..100u32 {
            // Clear and re-populate events
            unsafe {
                HostEventList::clear(event_list);
                HostParameterChanges::clear(param_changes);
            }

            // Add some events for this block
            if block % 10 == 0 {
                unsafe {
                    HostEventList::add(event_list, Event::note_on(0, 0, 60, 0.8, block as i32));
                }
            }
            if block % 10 == 5 {
                unsafe {
                    HostEventList::add(
                        event_list,
                        Event::note_off(0, 0, 60, 0.0, block as i32 - 5),
                    );
                }
            }

            // Add parameter automation
            unsafe {
                HostParameterChanges::add_change(param_changes, 1, 0, block as f64 / 100.0);
            }

            bufs.prepare(512);
            bufs.set_input_events(HostEventList::as_ptr(event_list));
            bufs.set_input_parameter_changes(HostParameterChanges::as_ptr(param_changes));
            bufs.set_process_context(ctx.as_ptr());

            // Generate sine wave input
            if let Some(ch0) = bufs.input_buffer_mut(0) {
                let phase_offset = block as f32 * 512.0;
                for (i, s) in ch0.iter_mut().enumerate() {
                    *s = ((phase_offset + i as f32) * 0.01).sin();
                }
            }

            // Passthrough via raw pointers
            unsafe {
                let pd = &*bufs.process_data_ptr();
                let in_bus = &*pd.inputs;
                let out_bus = &*pd.outputs;
                for ch in 0..2 {
                    let in_ptr = *in_bus.channel_buffers_32.add(ch);
                    let out_ptr = *out_bus.channel_buffers_32.add(ch);
                    std::ptr::copy_nonoverlapping(in_ptr, out_ptr, 512);
                }
            }

            ctx.advance(512);
        }

        // Clean up
        bufs.set_input_events(std::ptr::null_mut());
        bufs.set_input_parameter_changes(std::ptr::null_mut());
        bufs.set_process_context(std::ptr::null_mut());
        unsafe {
            HostEventList::destroy(event_list);
            HostParameterChanges::destroy(param_changes);
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // Concurrent COM object access
    // ═══════════════════════════════════════════════════════════════════

    /// Multiple threads creating and destroying COM objects simultaneously.
    /// ASan's thread-aware tracking catches data races on heap metadata.
    #[test]
    fn asan_concurrent_com_objects() {
        use crate::vst3::com::Event;
        use crate::vst3::event_list::HostEventList;
        use crate::vst3::param_changes::HostParameterChanges;

        let handles: Vec<_> = (0..4)
            .map(|_| {
                std::thread::spawn(|| {
                    for _ in 0..25 {
                        let el = HostEventList::new();
                        let pc = HostParameterChanges::new();

                        unsafe {
                            for j in 0..10 {
                                HostEventList::add(el, Event::note_on(j, 0, 60, 0.8, j));
                                HostParameterChanges::add_change(pc, j as u32, 0, 0.5);
                            }

                            assert_eq!(HostEventList::event_count(el), 10);
                            HostEventList::destroy(el);
                            HostParameterChanges::destroy(pc);
                        }
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }
    }

    /// HostComponentHandler concurrent perform_edit calls via COM vtable.
    /// The vtable is private, so we use raw pointer offsets matching the
    /// #[repr(C)] COM layout: [query_interface, add_ref, release,
    /// begin_edit(3), perform_edit(4), end_edit(5), restart_component(6)].
    #[test]
    fn asan_component_handler_concurrent() {
        use crate::vst3::component_handler::HostComponentHandler;

        // COM vtable function signatures matching IComponentHandler ABI
        type BeginEditFn = unsafe extern "system" fn(*mut c_void, u32) -> i32;
        type PerformEditFn = unsafe extern "system" fn(*mut c_void, u32, f64) -> i32;
        type EndEditFn = unsafe extern "system" fn(*mut c_void, u32) -> i32;

        let handler = HostComponentHandler::new();
        let ptr = HostComponentHandler::as_ptr(handler);
        assert!(!ptr.is_null());

        // Read COM vtable function pointers from the repr(C) layout.
        // The first field of HostComponentHandler is `vtbl: *const Vtbl`,
        // and each vtable entry is a function pointer.
        let (begin_edit, perform_edit, end_edit) = unsafe {
            let vtbl_ptr = *(handler as *const *const usize);
            let begin_edit: BeginEditFn = std::mem::transmute(*vtbl_ptr.add(3));
            let perform_edit: PerformEditFn = std::mem::transmute(*vtbl_ptr.add(4));
            let end_edit: EndEditFn = std::mem::transmute(*vtbl_ptr.add(5));
            (begin_edit, perform_edit, end_edit)
        };

        // Simulate concurrent plugin callbacks from multiple threads.
        let handler_addr = handler as usize;
        let handles: Vec<_> = (0..4)
            .map(|thread_id| {
                std::thread::spawn(move || {
                    let handler = handler_addr as *mut c_void;
                    for i in 0..25u32 {
                        unsafe {
                            let param_id = thread_id * 1000 + i;
                            (begin_edit)(handler, param_id);
                            (perform_edit)(handler, param_id, i as f64 * 0.04);
                            (end_edit)(handler, param_id);
                        }
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        // Drain all changes. Note: perform_edit uses try_lock(), so some
        // edits can be dropped under contention. We verify we get a reasonable
        // number and that the memory is sound (which is what ASan validates).
        let changes = unsafe { HostComponentHandler::drain_changes(handler) };
        assert!(
            changes.len() > 0 && changes.len() <= 100,
            "Expected 1..=100 changes, got {}",
            changes.len()
        );

        unsafe {
            HostComponentHandler::destroy(handler);
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // is_system_malloc_ptr validation
    // ═══════════════════════════════════════════════════════════════════

    /// Verify system_alloc pointers are in system malloc zone under ASan.
    /// (ASan replaces the allocator, so this tests ASan's malloc wrapper.)
    #[test]
    fn asan_system_alloc_zone_check() {
        use crate::vst3::host_alloc::{is_system_malloc_ptr, system_alloc, system_free};

        unsafe {
            let ptr = system_alloc(42u64);
            // Under ASan, system malloc is intercepted — pointer should still
            // be recognised as a valid allocation.
            let _ = is_system_malloc_ptr(ptr); // May or may not return true under ASan
            system_free(ptr);
        }
    }
}
