//! Miri-targeted dynamic analysis tests for unsafe code.
//!
//! These tests are designed to exercise the most safety-critical unsafe code paths
//! under Miri's strict provenance and aliasing model. They focus on:
//!   - COM vtable pointer chains (raw pointer casts, Box::into_raw/from_raw)
//!   - Self-referential buffer structures (ProcessBuffers)
//!   - Struct-to-bytes reinterpretation (Event union)
//!   - Cross-module integration (MIDI → EventList → ProcessData)
//!
//! Run with:
//!   cargo +nightly miri test --lib miri_tests
//!
//! These tests contain NO FFI, NO system calls, and NO platform-specific code,
//! making them fully compatible with Miri's interpreter.

#[cfg(test)]
mod tests {
    use crate::vst3::com::*;
    use crate::vst3::event_list::HostEventList;
    use crate::vst3::param_changes::HostParameterChanges;
    use crate::vst3::process::ProcessBuffers;
    use crate::vst3::process_context::ProcessContext as HostProcessContext;
    #[allow(unused_imports)]
    use std::ffi::c_void;

    // ═══════════════════════════════════════════════════════════════════
    // Event struct: byte-level reinterpretation
    // ═══════════════════════════════════════════════════════════════════

    /// Verify that `Event::note_on` writes valid NoteOnEvent data into the
    /// byte array, and that reading it back through a typed pointer
    /// produces correct values. Miri validates alignment and provenance.
    #[test]
    fn miri_event_note_on_roundtrip() {
        let event = make_note_on_event(42, 3, 60, 0.75, 99);

        assert_eq!(event.r#type, K_NOTE_ON_EVENT);
        assert_eq!(event.sampleOffset, 42);
        assert_eq!(event.flags, K_IS_LIVE);

        // Read back through typed union access — Miri checks alignment
        let note = unsafe { event_as_note_on(&event) };
        assert_eq!(note.channel, 3);
        assert_eq!(note.pitch, 60);
        assert!((note.velocity - 0.75).abs() < f32::EPSILON);
        assert_eq!(note.noteId, 99);
        assert_eq!(note.length, 0);
        assert!((note.tuning - 0.0).abs() < f32::EPSILON);
    }

    /// Verify that `Event::note_off` writes valid NoteOffEvent data.
    #[test]
    fn miri_event_note_off_roundtrip() {
        let event = make_note_off_event(128, 5, 72, 0.5, -1);

        assert_eq!(event.r#type, K_NOTE_OFF_EVENT);
        assert_eq!(event.sampleOffset, 128);

        let note = unsafe { event_as_note_off(&event) };
        assert_eq!(note.channel, 5);
        assert_eq!(note.pitch, 72);
        assert!((note.velocity - 0.5).abs() < f32::EPSILON);
        assert_eq!(note.noteId, -1);
    }

    /// Verify that all 20 bytes of the data field are deterministic
    /// (no uninitialized memory). Miri flags reads of uninit memory.
    #[test]
    fn miri_event_data_fully_initialized() {
        let event = make_note_on_event(0, 0, 60, 1.0, 42);
        // Read every byte of the Event struct — Miri will error if any byte is uninit
        let bytes: &[u8] = unsafe {
            std::slice::from_raw_parts(
                &event as *const Event as *const u8,
                std::mem::size_of::<Event>(),
            )
        };
        let mut checksum: u8 = 0;
        for byte in bytes {
            checksum = checksum.wrapping_add(*byte);
        }
        // Just ensure it doesn't trap — value doesn't matter
        let _ = checksum;
    }

    /// Boundary values for NoteOnEvent fields — exercise extremes.
    #[test]
    fn miri_event_extreme_values() {
        let event = make_note_on_event(i32::MAX, i16::MAX, i16::MAX, 1.0, i32::MIN);
        let note = unsafe { event_as_note_on(&event) };
        assert_eq!(note.channel, i16::MAX);
        assert_eq!(note.pitch, i16::MAX);
        assert_eq!(note.noteId, i32::MIN);

        let event = make_note_off_event(i32::MIN, i16::MIN, 0, 0.0, 0);
        let note = unsafe { event_as_note_off(&event) };
        assert_eq!(note.channel, i16::MIN);
        assert_eq!(note.pitch, 0);
    }

    // ═══════════════════════════════════════════════════════════════════
    // EventList COM: pointer chain correctness
    // ═══════════════════════════════════════════════════════════════════

    /// Full lifecycle: create → add events → read via vtable → destroy.
    /// Miri validates the entire Box::into_raw → raw pointer cast → vtable
    /// dispatch → Box::from_raw chain.
    #[test]
    fn miri_event_list_full_lifecycle() {
        let list = HostEventList::new();
        assert!(!list.is_null());

        unsafe {
            // Add multiple events
            for i in 0..10 {
                HostEventList::add(list, make_note_on_event(i, 0, 60 + i as i16, 0.8, i));
            }
            assert_eq!(HostEventList::event_count(list), 10);

            // Read back via vtable (as a plugin would do)
            // The vtable ptr is at offset 0 of the #[repr(C)] struct
            let vtbl_ptr = *(list as *const *const IEventListVtbl);
            let vtbl = &*vtbl_ptr;
            assert_eq!((vtbl.getEventCount)(list as *mut IEventList), 10);

            for i in 0..10 {
                let mut evt = std::mem::zeroed::<Event>();
                let result = (vtbl.getEvent)(list as *mut IEventList, i, &mut evt);
                assert_eq!(result, K_RESULT_OK);
                assert_eq!(evt.sampleOffset, i);
                assert_eq!(evt.r#type, K_NOTE_ON_EVENT);

                let note = event_as_note_on(&evt);
                assert_eq!(note.pitch, 60 + i as i16);
                assert_eq!(note.noteId, i);
            }

            // Clear and verify empty
            HostEventList::clear(list);
            assert_eq!(HostEventList::event_count(list), 0);

            HostEventList::destroy(list);
        }
    }

    /// Verify that QueryInterface returns valid self-pointer and doesn't
    /// corrupt the object.
    #[test]
    fn miri_event_list_query_interface_preserves_object() {
        let list = HostEventList::new();

        unsafe {
            // Add an event before QI
            HostEventList::add(list, make_note_on_event(0, 0, 60, 0.8, 1));

            let vtbl_ptr = *(list as *const *const IEventListVtbl);
            let vtbl = &*vtbl_ptr;
            let mut obj: *mut c_void = std::ptr::null_mut();

            // QI for IEventList
            let result =
                (vtbl.base.queryInterface)(list as *mut FUnknown, IEVENT_LIST_IID.as_ptr() as *const _, &mut obj);
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(obj, list as *mut c_void);

            // Object still usable after QI
            assert_eq!(HostEventList::event_count(list), 1);

            // QI for FUnknown
            let result =
                (vtbl.base.queryInterface)(list as *mut FUnknown, FUNKNOWN_IID.as_ptr() as *const _, &mut obj);
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(HostEventList::event_count(list), 1);

            HostEventList::destroy(list);
        }
    }

    /// Stress: fill to capacity, verify all events survive.
    #[test]
    fn miri_event_list_capacity_stress() {
        let list = HostEventList::new();

        unsafe {
            // Fill to the MAX_EVENTS_PER_BLOCK limit (512)
            for i in 0..512 {
                HostEventList::add(list, make_note_on_event(i, 0, (i % 128) as i16, 0.5, i));
            }
            assert_eq!(HostEventList::event_count(list), 512);

            // Adding beyond capacity should be silently dropped
            HostEventList::add(list, make_note_on_event(999, 0, 60, 1.0, 999));
            assert_eq!(HostEventList::event_count(list), 512);

            // Verify first and last via vtable
            let vtbl_ptr = *(list as *const *const IEventListVtbl);
            let vtbl = &*vtbl_ptr;
            let mut evt = std::mem::zeroed::<Event>();
            (vtbl.getEvent)(list as *mut IEventList, 0, &mut evt);
            assert_eq!(evt.sampleOffset, 0);

            (vtbl.getEvent)(list as *mut IEventList, 511, &mut evt);
            assert_eq!(evt.sampleOffset, 511);

            HostEventList::destroy(list);
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // ParameterChanges COM: lifecycle and public API
    // ═══════════════════════════════════════════════════════════════════

    /// Full lifecycle for parameter changes via public API.
    #[test]
    fn miri_param_changes_full_lifecycle() {
        let changes = HostParameterChanges::new();
        assert!(!changes.is_null());

        unsafe {
            // Add changes for different parameters
            assert!(HostParameterChanges::add_change(changes, 100, 0, 0.5));
            assert!(HostParameterChanges::add_change(changes, 200, 0, 0.75));
            assert_eq!(HostParameterChanges::change_count(changes), 2);

            // Clear and verify
            HostParameterChanges::clear(changes);
            assert_eq!(HostParameterChanges::change_count(changes), 0);

            // Re-add after clear
            assert!(HostParameterChanges::add_change(changes, 300, 0, 0.9));
            assert_eq!(HostParameterChanges::change_count(changes), 1);

            HostParameterChanges::destroy(changes);
        }
    }

    /// Multiple parameters with reuse of existing queue.
    #[test]
    fn miri_param_changes_queue_reuse() {
        let changes = HostParameterChanges::new();

        unsafe {
            // Add changes for 3 different parameters
            assert!(HostParameterChanges::add_change(changes, 10, 0, 0.25));
            assert!(HostParameterChanges::add_change(changes, 20, 0, 0.50));
            assert!(HostParameterChanges::add_change(changes, 30, 0, 0.75));
            assert_eq!(HostParameterChanges::change_count(changes), 3);

            // Add another point to the first param — should reuse existing queue
            assert!(HostParameterChanges::add_change(changes, 10, 64, 0.30));
            // Still 3 queues (not 4)
            assert_eq!(HostParameterChanges::change_count(changes), 3);

            HostParameterChanges::clear(changes);
            HostParameterChanges::destroy(changes);
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // ProcessBuffers: self-referential pointer chain
    // ═══════════════════════════════════════════════════════════════════

    /// Verify the full pointer chain: ProcessData → AudioBusBuffers →
    /// channel_buffers_32 → per-channel sample data. This is the most
    /// critical unsafe structure — a dangling pointer here causes UB
    /// in the real-time audio callback.
    #[test]
    fn miri_process_buffers_pointer_chain() {
        let mut bufs = ProcessBuffers::new(2, 2, 256);
        bufs.prepare(128);

        unsafe {
            let pd = &*bufs.process_data_ptr();
            assert_eq!(pd.numSamples, 128);
            assert_eq!(pd.numInputs, 1);
            assert_eq!(pd.numOutputs, 1);

            // Walk input bus → channel pointers → sample data
            let input_bus = &*pd.inputs;
            assert_eq!(input_bus.numChannels, 2);
            assert!(!input_bus.__field0.channelBuffers32.is_null());

            // Read channel 0 data pointer and write a sample
            let ch0_ptr = *input_bus.__field0.channelBuffers32;
            assert!(!ch0_ptr.is_null());
            *ch0_ptr = 0.42;

            // Verify through safe API
            let ch0 = bufs.input_buffer_mut(0).unwrap();
            assert_eq!(ch0[0], 0.42);

            // Walk output bus similarly
            let output_bus = &*pd.outputs;
            assert_eq!(output_bus.numChannels, 2);
            assert!(!output_bus.__field0.channelBuffers32.is_null());

            let out_ch1_ptr = *output_bus.__field0.channelBuffers32.add(1);
            assert!(!out_ch1_ptr.is_null());
            *out_ch1_ptr.add(10) = 0.99;

            let out_ch1 = bufs.output_buffer(1).unwrap();
            assert_eq!(out_ch1[10], 0.99);
        }
    }

    /// Verify that `prepare()` re-establishes valid pointers and doesn't
    /// leave dangling references from the previous call.
    #[test]
    fn miri_process_buffers_prepare_stability() {
        let mut bufs = ProcessBuffers::new(2, 2, 512);

        for block_size in [64, 128, 256, 512, 32] {
            bufs.prepare(block_size);

            // Write through pointer chain
            unsafe {
                let pd = &*bufs.process_data_ptr();
                let input_bus = &*pd.inputs;
                let ch0_ptr = *input_bus.__field0.channelBuffers32;
                *ch0_ptr = block_size as f32;

                let output_bus = &*pd.outputs;
                let out_ch0_ptr = *output_bus.__field0.channelBuffers32;
                assert_eq!(*out_ch0_ptr, 0.0); // Output cleared by prepare()
            }

            // Verify through safe API
            let ch0 = bufs.input_buffer_mut(0).unwrap();
            assert_eq!(ch0[0], block_size as f32);
        }
    }

    /// Interleave/deinterleave roundtrip through the full buffer chain.
    #[test]
    fn miri_process_buffers_interleave_roundtrip() {
        let mut bufs = ProcessBuffers::new(2, 2, 8);
        bufs.prepare(8);

        // Write stereo interleaved data
        let input: Vec<f32> = (0..16).map(|i| i as f32 * 0.1).collect();
        bufs.write_input_interleaved(&input, 2);

        // Simulate plugin passthrough: copy input → output via pointer chain
        unsafe {
            let pd = &*bufs.process_data_ptr();
            let in_bus = &*pd.inputs;
            let out_bus = &*pd.outputs;
            for ch in 0..2 {
                let in_ptr = *in_bus.__field0.channelBuffers32.add(ch);
                let out_ptr = *out_bus.__field0.channelBuffers32.add(ch);
                std::ptr::copy_nonoverlapping(in_ptr, out_ptr, 8);
            }
        }

        // Read back interleaved
        let mut output = vec![0.0f32; 16];
        bufs.read_output_interleaved(&mut output, 2);

        for i in 0..16 {
            assert!(
                (output[i] - input[i]).abs() < f32::EPSILON,
                "mismatch at index {i}: {} != {}",
                output[i],
                input[i]
            );
        }
    }

    /// ProcessBuffers with zero channels should not produce null dereferences.
    #[test]
    fn miri_process_buffers_zero_channels() {
        let mut bufs = ProcessBuffers::new(0, 0, 256);
        bufs.prepare(128);

        let pd = bufs.process_data_ptr();
        unsafe {
            assert_eq!((*pd).numInputs, 0);
            assert_eq!((*pd).numOutputs, 0);
            assert!((*pd).inputs.is_null());
            assert!((*pd).outputs.is_null());
        }
    }

    /// Asymmetric channels: mono in, stereo out.
    #[test]
    fn miri_process_buffers_asymmetric_channels() {
        let mut bufs = ProcessBuffers::new(1, 2, 128);
        bufs.prepare(64);

        unsafe {
            let pd = &*bufs.process_data_ptr();

            // 1 input channel
            let input_bus = &*pd.inputs;
            assert_eq!(input_bus.numChannels, 1);
            let ch0 = *input_bus.__field0.channelBuffers32;
            *ch0 = 0.5;

            // 2 output channels
            let output_bus = &*pd.outputs;
            assert_eq!(output_bus.numChannels, 2);
            let out0 = *output_bus.__field0.channelBuffers32;
            let out1 = *output_bus.__field0.channelBuffers32.add(1);
            *out0 = 0.1;
            *out1.add(1) = 0.2;
        }

        let ch0 = bufs.input_buffer_mut(0).unwrap();
        assert_eq!(ch0[0], 0.5);
        let out0 = bufs.output_buffer(0).unwrap();
        assert_eq!(out0[0], 0.1);
        let out1 = bufs.output_buffer(1).unwrap();
        assert_eq!(out1[1], 0.2);
    }

    // ═══════════════════════════════════════════════════════════════════
    // ProcessContext: pointer validity
    // ═══════════════════════════════════════════════════════════════════

    /// Verify ProcessContext can be wired into ProcessData and accessed.
    #[test]
    fn miri_process_context_in_process_data() {
        let mut ctx = HostProcessContext::new(44100.0);
        ctx.set_tempo(140.0);
        ctx.set_playing(true);

        let mut bufs = ProcessBuffers::new(2, 2, 256);
        bufs.prepare(128);
        bufs.set_process_context(ctx.as_ptr() as *mut ProcessContext);

        // Read back through the ProcessData pointer — verifies the full chain
        unsafe {
            let pd = &*bufs.process_data_ptr();
            assert!(!pd.processContext.is_null());
        }

        // Clear context pointer
        bufs.set_process_context(std::ptr::null_mut());
    }

    // ═══════════════════════════════════════════════════════════════════
    // Cross-module integration: MIDI → EventList → ProcessData
    // ═══════════════════════════════════════════════════════════════════

    /// End-to-end: create MIDI messages → translate to VST3 events →
    /// add to EventList → wire into ProcessData → read back via vtable.
    /// This exercises the full pointer web that runs in the real-time
    /// audio callback, under Miri's strict provenance model.
    #[test]
    fn miri_midi_to_process_data_integration() {
        use crate::midi::device::RawMidiMessage;
        use crate::midi::translate::translate_midi_batch;

        // Create MIDI Note On + Note Off
        let messages = vec![
            RawMidiMessage {
                timestamp_us: 0,
                data: [0x90, 60, 100],
                len: 3,
            },
            RawMidiMessage {
                timestamp_us: 100,
                data: [0x80, 60, 64],
                len: 3,
            },
        ];

        // Translate MIDI → VST3 events
        let events = translate_midi_batch(&messages);
        assert_eq!(events.len(), 2);

        // Create event list and add translated events
        let event_list = HostEventList::new();
        unsafe {
            for event in &events {
                HostEventList::add(event_list, *event);
            }
            assert_eq!(HostEventList::event_count(event_list), 2);
        }

        // Wire into ProcessBuffers
        let mut bufs = ProcessBuffers::new(2, 2, 256);
        bufs.prepare(128);
        bufs.set_input_events(HostEventList::as_ptr(event_list) as *mut IEventList);

        // Read back through ProcessData — full pointer chain
        unsafe {
            let pd = &*bufs.process_data_ptr();
            assert!(!pd.inputEvents.is_null());

            // Cast back to HostEventList and read through vtable
            let el = pd.inputEvents as *mut HostEventList;
            let vtbl_ptr = *(el as *const *const IEventListVtbl);
            let vtbl = &*vtbl_ptr;
            assert_eq!((vtbl.getEventCount)(el as *mut IEventList), 2);

            // Verify first event (Note On)
            let mut evt = std::mem::zeroed::<Event>();
            let result = (vtbl.getEvent)(el as *mut IEventList, 0, &mut evt);
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(evt.r#type, K_NOTE_ON_EVENT);

            let note = event_as_note_on(&evt);
            assert_eq!(note.pitch, 60);
            assert!((note.velocity - 100.0 / 127.0).abs() < 0.01);

            // Verify second event (Note Off)
            let result = (vtbl.getEvent)(el as *mut IEventList, 1, &mut evt);
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(evt.r#type, K_NOTE_OFF_EVENT);

            HostEventList::destroy(event_list);
        }
    }

    /// Full mock process call: ProcessBuffers + EventList + ParamChanges + ProcessContext
    /// all wired together through ProcessData. This exercises the maximum pointer web
    /// that Miri can validate in one test.
    #[test]
    fn miri_full_mock_process_call() {
        // Set up all components
        let event_list = HostEventList::new();
        let param_changes = HostParameterChanges::new();
        let mut process_ctx = HostProcessContext::new(44100.0);
        process_ctx.set_tempo(120.0);
        process_ctx.set_playing(true);

        let mut bufs = ProcessBuffers::new(2, 2, 512);
        bufs.prepare(256);

        // Add events
        unsafe {
            HostEventList::add(event_list, make_note_on_event(0, 0, 60, 0.8, 1));
            HostEventList::add(event_list, make_note_on_event(64, 0, 64, 0.6, 2));
            HostEventList::add(event_list, make_note_off_event(128, 0, 60, 0.0, 1));
        }

        // Add param changes
        unsafe {
            HostParameterChanges::add_change(param_changes, 1, 0, 0.5);
            HostParameterChanges::add_change(param_changes, 2, 0, 0.75);
        }

        // Wire everything into ProcessData
        bufs.set_input_events(HostEventList::as_ptr(event_list) as *mut IEventList);
        bufs.set_input_parameter_changes(HostParameterChanges::as_ptr(param_changes) as *mut IParameterChanges);
        bufs.set_process_context(process_ctx.as_ptr() as *mut ProcessContext);

        // Write input audio
        if let Some(ch0) = bufs.input_buffer_mut(0) {
            for (i, sample) in ch0.iter_mut().enumerate() {
                *sample = (i as f32 * 0.001).sin();
            }
        }

        // Verify full ProcessData is consistent
        unsafe {
            let pd = &*bufs.process_data_ptr();
            assert_eq!(pd.numSamples, 256);
            assert!(!pd.inputEvents.is_null());
            assert!(!pd.inputParameterChanges.is_null());
            assert!(!pd.processContext.is_null());
            assert!(!pd.inputs.is_null());
            assert!(!pd.outputs.is_null());

            // Walk event list through vtable
            let el = pd.inputEvents as *mut HostEventList;
            let vtbl_ptr = *(el as *const *const IEventListVtbl);
            let vtbl = &*vtbl_ptr;
            assert_eq!((vtbl.getEventCount)(el as *mut IEventList), 3);

            // Walk parameter changes — verify count through public API
            let pc = pd.inputParameterChanges as *mut HostParameterChanges;
            assert_eq!(HostParameterChanges::change_count(pc), 2);

            // Walk input audio through bus pointer chain
            let input_bus = &*pd.inputs;
            let ch0 = *input_bus.__field0.channelBuffers32;
            assert!((*ch0).abs() < 0.01); // sin(0) ≈ 0

            // Output should be zeroed (prepare clears it)
            let output_bus = &*pd.outputs;
            let out0 = *output_bus.__field0.channelBuffers32;
            assert_eq!(*out0, 0.0);
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

    /// Verify ProcessBuffers can be moved across threads without UB.
    /// Exercises the `unsafe impl Send for ProcessBuffers` declaration.
    #[test]
    fn miri_process_buffers_send_across_thread() {
        let mut bufs = ProcessBuffers::new(2, 2, 128);
        bufs.prepare(64);

        // Write data on this thread
        if let Some(ch0) = bufs.input_buffer_mut(0) {
            ch0[0] = 0.42;
        }

        // Move to another thread and verify
        let handle = std::thread::spawn(move || {
            // Re-prepare on the new thread (re-establishes pointers)
            bufs.prepare(32);

            unsafe {
                let pd = &*bufs.process_data_ptr();
                assert_eq!(pd.numSamples, 32);

                // Pointer chain still valid after move + re-prepare
                let input_bus = &*pd.inputs;
                let ch0 = *input_bus.__field0.channelBuffers32;
                *ch0 = 0.99;
            }

            let ch0 = bufs.input_buffer_mut(0).unwrap();
            assert_eq!(ch0[0], 0.99);
            bufs
        });

        let bufs = handle.join().unwrap();
        let ch0 = bufs.output_buffer(0).unwrap();
        // Output was cleared by prepare(32) on the other thread
        assert_eq!(ch0[0], 0.0);
    }

    /// Repeated create/destroy cycles to catch any leak or double-free in
    /// COM object lifecycle management.
    #[test]
    fn miri_com_lifecycle_stress() {
        for _ in 0..50 {
            let el = HostEventList::new();
            let pc = HostParameterChanges::new();

            unsafe {
                HostEventList::add(el, make_note_on_event(0, 0, 60, 0.8, 1));
                HostParameterChanges::add_change(pc, 42, 0, 0.5);

                HostEventList::clear(el);
                HostParameterChanges::clear(pc);

                HostEventList::destroy(el);
                HostParameterChanges::destroy(pc);
            }
        }
    }

    /// Verify Event::Clone doesn't produce UB — the data field is a byte
    /// array copied bitwise.
    #[test]
    fn miri_event_clone() {
        let original = make_note_on_event(10, 3, 72, 0.9, 42);
        let cloned = original;

        // Both should have identical data
        assert_eq!(original.sampleOffset, cloned.sampleOffset);
        assert_eq!(original.r#type, cloned.r#type);
        // Compare raw bytes of both events
        let orig_bytes: &[u8] = unsafe {
            std::slice::from_raw_parts(
                &original as *const Event as *const u8,
                std::mem::size_of::<Event>(),
            )
        };
        let clone_bytes: &[u8] = unsafe {
            std::slice::from_raw_parts(
                &cloned as *const Event as *const u8,
                std::mem::size_of::<Event>(),
            )
        };
        assert_eq!(orig_bytes, clone_bytes);

        // Read typed data from clone
        let note = unsafe { event_as_note_on(&cloned) };
        assert_eq!(note.pitch, 72);
        assert_eq!(note.noteId, 42);
    }

    /// Destroy null pointers — should be no-ops without UB.
    #[test]
    fn miri_null_destroy_safety() {
        unsafe {
            HostEventList::destroy(std::ptr::null_mut());
            HostParameterChanges::destroy(std::ptr::null_mut());
        }
    }
}
