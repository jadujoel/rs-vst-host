//! Process buffer management for VST3 audio processing.
//!
//! Pre-allocates and manages all buffers needed by `IAudioProcessor::process()`:
//! - Per-channel sample buffers (input and output)
//! - Channel pointer arrays
//! - AudioBusBuffers structs
//! - ProcessData struct
//!
//! All memory is allocated once and reused across process calls for real-time safety.

use crate::vst3::com::{AudioBusBuffers, K_REALTIME, K_SAMPLE_32, ProcessData};

/// Pre-allocated buffers for VST3 process calls.
///
/// Owns all memory and provides stable pointers for the duration of processing.
/// Call `prepare()` each block to fill input buffers, then `process_data_ptr()`
/// to get the pointer for `IAudioProcessor::process()`.
pub struct ProcessBuffers {
    /// Input channel sample data: `input_buffers[channel][sample]`.
    input_buffers: Vec<Vec<f32>>,
    /// Output channel sample data: `output_buffers[channel][sample]`.
    output_buffers: Vec<Vec<f32>>,
    /// Pointer arrays for input channels (stable addresses into input_buffers).
    input_ptrs: Vec<*mut f32>,
    /// Pointer arrays for output channels (stable addresses into output_buffers).
    output_ptrs: Vec<*mut f32>,
    /// Input AudioBusBuffers (single bus).
    input_bus: AudioBusBuffers,
    /// Output AudioBusBuffers (single bus).
    output_bus: AudioBusBuffers,
    /// The ProcessData struct passed to the plugin.
    process_data: ProcessData,
    /// Number of input channels.
    num_input_channels: usize,
    /// Number of output channels.
    num_output_channels: usize,
    /// Maximum block size (samples per channel).
    max_block_size: usize,
}

impl ProcessBuffers {
    /// Create new process buffers for the given channel configuration and block size.
    pub fn new(
        num_input_channels: usize,
        num_output_channels: usize,
        max_block_size: usize,
    ) -> Self {
        let input_buffers: Vec<Vec<f32>> = (0..num_input_channels)
            .map(|_| vec![0.0f32; max_block_size])
            .collect();

        let output_buffers: Vec<Vec<f32>> = (0..num_output_channels)
            .map(|_| vec![0.0f32; max_block_size])
            .collect();

        let input_ptrs: Vec<*mut f32> = Vec::with_capacity(num_input_channels);
        let output_ptrs: Vec<*mut f32> = Vec::with_capacity(num_output_channels);

        let mut buffers = Self {
            input_buffers,
            output_buffers,
            input_ptrs,
            output_ptrs,
            input_bus: AudioBusBuffers {
                num_channels: num_input_channels as i32,
                silence_flags: 0,
                channel_buffers_32: std::ptr::null_mut(),
            },
            output_bus: AudioBusBuffers {
                num_channels: num_output_channels as i32,
                silence_flags: 0,
                channel_buffers_32: std::ptr::null_mut(),
            },
            process_data: ProcessData {
                process_mode: K_REALTIME,
                symbolic_sample_size: K_SAMPLE_32,
                num_samples: 0,
                num_inputs: if num_input_channels > 0 { 1 } else { 0 },
                num_outputs: if num_output_channels > 0 { 1 } else { 0 },
                inputs: std::ptr::null_mut(),
                outputs: std::ptr::null_mut(),
                input_parameter_changes: std::ptr::null_mut(),
                output_parameter_changes: std::ptr::null_mut(),
                input_events: std::ptr::null_mut(),
                output_events: std::ptr::null_mut(),
                process_context: std::ptr::null_mut(),
            },
            num_input_channels,
            num_output_channels,
            max_block_size,
        };

        // Update pointer arrays to point to the actual buffer data.
        buffers.update_ptrs();
        buffers
    }

    /// Update the internal pointer arrays to reflect current buffer addresses.
    ///
    /// Must be called after construction and any time the buffers are reallocated.
    fn update_ptrs(&mut self) {
        // Build input channel pointer array
        self.input_ptrs.clear();
        for buf in &mut self.input_buffers {
            self.input_ptrs.push(buf.as_mut_ptr());
        }

        // Build output channel pointer array
        self.output_ptrs.clear();
        for buf in &mut self.output_buffers {
            self.output_ptrs.push(buf.as_mut_ptr());
        }

        // Point AudioBusBuffers to the pointer arrays
        if !self.input_ptrs.is_empty() {
            self.input_bus.channel_buffers_32 = self.input_ptrs.as_mut_ptr();
        }
        if !self.output_ptrs.is_empty() {
            self.output_bus.channel_buffers_32 = self.output_ptrs.as_mut_ptr();
        }

        // Point ProcessData to the bus structs
        if self.num_input_channels > 0 {
            self.process_data.inputs = &mut self.input_bus;
        }
        if self.num_output_channels > 0 {
            self.process_data.outputs = &mut self.output_bus;
        }
    }

    /// Prepare buffers for a process call with the given number of samples.
    ///
    /// Clears output buffers to zero. Input buffers should be filled by the caller
    /// after this call (via `input_buffer_mut`).
    pub fn prepare(&mut self, num_samples: usize) {
        let samples = num_samples.min(self.max_block_size);
        self.process_data.num_samples = samples as i32;

        // Clear output buffers
        for buf in &mut self.output_buffers {
            buf[..samples].fill(0.0);
        }

        // Reset silence flags
        self.input_bus.silence_flags = 0;
        self.output_bus.silence_flags = 0;

        // Refresh self-referential pointers (process_data.inputs/outputs point
        // into this struct's own fields, so they become dangling after a move).
        // The channel pointer arrays and their heap allocations are stable and
        // do not need rebuilding.
        if self.num_input_channels > 0 {
            self.process_data.inputs = &mut self.input_bus;
        }
        if self.num_output_channels > 0 {
            self.process_data.outputs = &mut self.output_bus;
        }
    }

    /// Write interleaved input samples into the deinterleaved input buffers.
    ///
    /// `interleaved` contains `num_channels * num_samples` samples in
    /// channel-interleaved order (L, R, L, R, ...).
    #[allow(dead_code)]
    pub fn write_input_interleaved(&mut self, interleaved: &[f32], num_channels: usize) {
        if num_channels == 0 || self.num_input_channels == 0 {
            return;
        }

        let num_samples = self.process_data.num_samples as usize;
        let channels_to_write = num_channels.min(self.num_input_channels);

        // Fast path: stereo — avoids inner loop and uses sequential reads
        if channels_to_write == 2 && num_channels == 2 {
            let total = num_samples * 2;
            if total <= interleaved.len() {
                let (left_bufs, rest) = self.input_buffers.split_at_mut(1);
                let left = &mut left_bufs[0][..num_samples];
                let right = &mut rest[0][..num_samples];
                for (i, chunk) in interleaved[..total].chunks_exact(2).enumerate() {
                    left[i] = chunk[0];
                    right[i] = chunk[1];
                }
                return;
            }
        }

        // General path
        for sample in 0..num_samples {
            for ch in 0..channels_to_write {
                let interleaved_idx = sample * num_channels + ch;
                if interleaved_idx < interleaved.len() {
                    self.input_buffers[ch][sample] = interleaved[interleaved_idx];
                }
            }
        }
    }

    /// Read output buffers into interleaved format.
    ///
    /// Writes `num_channels * num_samples` samples in channel-interleaved order.
    pub fn read_output_interleaved(&self, interleaved: &mut [f32], num_channels: usize) {
        if num_channels == 0 || self.num_output_channels == 0 {
            interleaved.fill(0.0);
            return;
        }

        let num_samples = self.process_data.num_samples as usize;
        let channels_to_read = num_channels.min(self.num_output_channels);

        // Fast path: stereo output to stereo interleaved — avoids inner loop
        if channels_to_read == 2 && num_channels == 2 {
            let total = num_samples * 2;
            if total <= interleaved.len() {
                let left = &self.output_buffers[0];
                let right = &self.output_buffers[1];
                for (i, chunk) in interleaved[..total].chunks_exact_mut(2).enumerate() {
                    chunk[0] = left[i];
                    chunk[1] = right[i];
                }
                return;
            }
        }

        // General path
        for sample in 0..num_samples {
            for ch in 0..num_channels {
                let interleaved_idx = sample * num_channels + ch;
                if interleaved_idx < interleaved.len() {
                    if ch < channels_to_read {
                        interleaved[interleaved_idx] = self.output_buffers[ch][sample];
                    } else {
                        interleaved[interleaved_idx] = 0.0;
                    }
                }
            }
        }
    }

    /// Get a mutable reference to an input channel buffer.
    pub fn input_buffer_mut(&mut self, channel: usize) -> Option<&mut [f32]> {
        let samples = self.process_data.num_samples as usize;
        self.input_buffers
            .get_mut(channel)
            .map(|buf| &mut buf[..samples])
    }

    /// Get a reference to an output channel buffer.
    #[allow(dead_code)]
    pub fn output_buffer(&self, channel: usize) -> Option<&[f32]> {
        let samples = self.process_data.num_samples as usize;
        self.output_buffers.get(channel).map(|buf| &buf[..samples])
    }

    /// Get a pointer to the ProcessData for passing to `IAudioProcessor::process()`.
    pub fn process_data_ptr(&mut self) -> *mut ProcessData {
        &mut self.process_data
    }

    /// Maximum block size this buffer was allocated for.
    pub fn max_block_size(&self) -> usize {
        self.max_block_size
    }

    /// Number of input channels.
    pub fn num_input_channels(&self) -> usize {
        self.num_input_channels
    }

    /// Number of output channels.
    #[allow(dead_code)]
    pub fn num_output_channels(&self) -> usize {
        self.num_output_channels
    }

    /// Set the input events pointer on the ProcessData.
    ///
    /// This should point to a valid IEventList COM object, or null.
    pub fn set_input_events(&mut self, events: *mut std::ffi::c_void) {
        self.process_data.input_events = events;
    }

    /// Set the input parameter changes pointer on the ProcessData.
    ///
    /// This should point to a valid IParameterChanges COM object, or null.
    #[allow(dead_code)]
    pub fn set_input_parameter_changes(&mut self, changes: *mut std::ffi::c_void) {
        self.process_data.input_parameter_changes = changes;
    }

    /// Set the process context pointer on the ProcessData.
    ///
    /// This should point to a valid ProcessContext struct, or null.
    pub fn set_process_context(&mut self, context: *mut std::ffi::c_void) {
        self.process_data.process_context = context;
    }
}

// Safety: ProcessBuffers owns all its data exclusively and is only used
// from one thread at a time (protected by Mutex in the engine).
unsafe impl Send for ProcessBuffers {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_stereo_buffers() {
        let bufs = ProcessBuffers::new(2, 2, 512);
        assert_eq!(bufs.num_input_channels(), 2);
        assert_eq!(bufs.num_output_channels(), 2);
        assert_eq!(bufs.max_block_size(), 512);
    }

    #[test]
    fn test_new_no_input_buffers() {
        let bufs = ProcessBuffers::new(0, 2, 256);
        assert_eq!(bufs.num_input_channels(), 0);
        assert_eq!(bufs.num_output_channels(), 2);
    }

    #[test]
    fn test_prepare_sets_num_samples() {
        let mut bufs = ProcessBuffers::new(2, 2, 512);
        bufs.prepare(128);
        assert_eq!(bufs.process_data.num_samples, 128);
    }

    #[test]
    fn test_prepare_clamps_to_max() {
        let mut bufs = ProcessBuffers::new(2, 2, 512);
        bufs.prepare(1024);
        assert_eq!(bufs.process_data.num_samples, 512);
    }

    #[test]
    fn test_prepare_clears_output() {
        let mut bufs = ProcessBuffers::new(2, 2, 512);
        // Dirty the output
        bufs.output_buffers[0][0] = 1.0;
        bufs.output_buffers[1][0] = 1.0;
        bufs.prepare(512);
        assert_eq!(bufs.output_buffers[0][0], 0.0);
        assert_eq!(bufs.output_buffers[1][0], 0.0);
    }

    #[test]
    fn test_input_buffer_mut() {
        let mut bufs = ProcessBuffers::new(2, 2, 512);
        bufs.prepare(128);

        let ch0 = bufs.input_buffer_mut(0).unwrap();
        assert_eq!(ch0.len(), 128);
        ch0[0] = 0.5;

        assert_eq!(bufs.input_buffers[0][0], 0.5);
    }

    #[test]
    fn test_output_buffer() {
        let mut bufs = ProcessBuffers::new(2, 2, 512);
        bufs.prepare(64);
        bufs.output_buffers[0][10] = 0.75;

        let ch0 = bufs.output_buffer(0).unwrap();
        assert_eq!(ch0.len(), 64);
        assert_eq!(ch0[10], 0.75);
    }

    #[test]
    fn test_interleaved_roundtrip() {
        let mut bufs = ProcessBuffers::new(2, 2, 4);
        bufs.prepare(4);

        // Write interleaved stereo: L0, R0, L1, R1, L2, R2, L3, R3
        let input = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8];
        bufs.write_input_interleaved(&input, 2);

        // Verify deinterleaved
        assert_eq!(bufs.input_buffers[0][0], 0.1); // L0
        assert_eq!(bufs.input_buffers[1][0], 0.2); // R0
        assert_eq!(bufs.input_buffers[0][1], 0.3); // L1
        assert_eq!(bufs.input_buffers[1][1], 0.4); // R1

        // Copy input to output (simulate passthrough plugin)
        for ch in 0..2 {
            bufs.output_buffers[ch][..4].copy_from_slice(&bufs.input_buffers[ch][..4]);
        }

        // Read back interleaved
        let mut output = [0.0f32; 8];
        bufs.read_output_interleaved(&mut output, 2);
        assert_eq!(output, input);
    }

    #[test]
    fn test_process_data_ptr_not_null() {
        let mut bufs = ProcessBuffers::new(2, 2, 512);
        bufs.prepare(256);
        let ptr = bufs.process_data_ptr();
        assert!(!ptr.is_null());

        unsafe {
            assert_eq!((*ptr).num_samples, 256);
            assert_eq!((*ptr).num_inputs, 1);
            assert_eq!((*ptr).num_outputs, 1);
            assert_eq!((*ptr).symbolic_sample_size, K_SAMPLE_32);
            assert_eq!((*ptr).process_mode, K_REALTIME);
        }
    }

    #[test]
    fn test_read_output_fills_silence_for_missing_channels() {
        let mut bufs = ProcessBuffers::new(1, 1, 4);
        bufs.prepare(4);
        bufs.output_buffers[0][0] = 1.0;
        bufs.output_buffers[0][1] = 2.0;

        // Read as stereo (but plugin only has mono output)
        let mut output = [0.0f32; 8];
        bufs.read_output_interleaved(&mut output, 2);

        assert_eq!(output[0], 1.0); // L0 from plugin
        assert_eq!(output[1], 0.0); // R0 = silence (no channel 1)
        assert_eq!(output[2], 2.0); // L1 from plugin
        assert_eq!(output[3], 0.0); // R1 = silence
    }

    #[test]
    fn test_set_input_events() {
        let mut bufs = ProcessBuffers::new(2, 2, 512);
        assert!(bufs.process_data.input_events.is_null());

        let fake_ptr = 0x1234 as *mut std::ffi::c_void;
        bufs.set_input_events(fake_ptr);
        assert_eq!(bufs.process_data.input_events, fake_ptr);

        bufs.set_input_events(std::ptr::null_mut());
        assert!(bufs.process_data.input_events.is_null());
    }

    #[test]
    fn test_set_input_parameter_changes() {
        let mut bufs = ProcessBuffers::new(2, 2, 512);
        assert!(bufs.process_data.input_parameter_changes.is_null());

        let fake_ptr = 0x5678 as *mut std::ffi::c_void;
        bufs.set_input_parameter_changes(fake_ptr);
        assert_eq!(bufs.process_data.input_parameter_changes, fake_ptr);
    }

    #[test]
    fn test_set_process_context() {
        let mut bufs = ProcessBuffers::new(2, 2, 512);
        assert!(bufs.process_data.process_context.is_null());

        let fake_ptr = 0xABCD as *mut std::ffi::c_void;
        bufs.set_process_context(fake_ptr);
        assert_eq!(bufs.process_data.process_context, fake_ptr);
    }

    #[test]
    fn test_zero_channels() {
        let bufs = ProcessBuffers::new(0, 0, 512);
        assert_eq!(bufs.num_input_channels(), 0);
        assert_eq!(bufs.num_output_channels(), 0);
        assert_eq!(bufs.process_data.num_inputs, 0);
        assert_eq!(bufs.process_data.num_outputs, 0);
    }

    #[test]
    fn test_input_buffer_mut_out_of_range() {
        let mut bufs = ProcessBuffers::new(2, 2, 512);
        bufs.prepare(128);
        assert!(bufs.input_buffer_mut(0).is_some());
        assert!(bufs.input_buffer_mut(1).is_some());
        assert!(bufs.input_buffer_mut(2).is_none()); // Out of range
        assert!(bufs.input_buffer_mut(100).is_none());
    }

    #[test]
    fn test_output_buffer_out_of_range() {
        let mut bufs = ProcessBuffers::new(2, 2, 512);
        bufs.prepare(128);
        assert!(bufs.output_buffer(0).is_some());
        assert!(bufs.output_buffer(1).is_some());
        assert!(bufs.output_buffer(2).is_none());
    }

    #[test]
    fn test_write_input_interleaved_zero_channels() {
        let mut bufs = ProcessBuffers::new(0, 2, 512);
        bufs.prepare(4);
        // Should not panic with zero input channels
        bufs.write_input_interleaved(&[1.0, 2.0, 3.0, 4.0], 2);
    }

    #[test]
    fn test_read_output_interleaved_zero_channels() {
        let bufs = ProcessBuffers::new(2, 0, 512);
        let mut output = [1.0f32; 8];
        bufs.read_output_interleaved(&mut output, 2);
        // Should fill with silence
        assert!(output.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn test_mono_input_stereo_output() {
        let mut bufs = ProcessBuffers::new(1, 2, 4);
        bufs.prepare(4);

        // Write mono input
        let mono_input = [0.5, 0.6, 0.7, 0.8];
        bufs.write_input_interleaved(&mono_input, 1);
        assert_eq!(bufs.input_buffers[0][0], 0.5);
        assert_eq!(bufs.input_buffers[0][3], 0.8);
    }

    #[test]
    fn test_prepare_consecutive_calls() {
        let mut bufs = ProcessBuffers::new(2, 2, 512);

        bufs.prepare(128);
        assert_eq!(bufs.process_data.num_samples, 128);

        bufs.prepare(64);
        assert_eq!(bufs.process_data.num_samples, 64);

        bufs.prepare(256);
        assert_eq!(bufs.process_data.num_samples, 256);
    }
}
