//! Audio processing engine: bridges cpal audio stream with VST3 plugin processing.
//!
//! The engine owns the VST3 instance and process buffers, and runs inside
//! the cpal audio callback. A test tone generator provides input signal
//! for effect plugins. MIDI events are received via a lock-free queue and
//! translated to VST3 events for the plugin.

use crate::midi::device::MidiReceiver;
use crate::midi::translate;
use crate::vst3::event_list::HostEventList;
use crate::vst3::instance::Vst3Instance;
use crate::vst3::param_changes::HostParameterChanges;
use crate::vst3::process::ProcessBuffers;
use crate::vst3::process_context::ProcessContext;
use std::sync::Arc;
use tracing::debug;

/// Generates a sine wave test tone for effect plugin testing.
pub struct TestToneGenerator {
    /// Current phase (0..1).
    phase: f64,
    /// Phase increment per sample.
    phase_inc: f64,
    /// Amplitude (0..1).
    amplitude: f32,
    /// Whether the tone is enabled.
    pub enabled: bool,
}

impl TestToneGenerator {
    /// Create a new test tone generator.
    ///
    /// Frequency 440 Hz (A4), amplitude 0.25 (-12 dB).
    pub fn new(sample_rate: f64) -> Self {
        let frequency = 440.0;
        Self {
            phase: 0.0,
            phase_inc: frequency / sample_rate,
            amplitude: 0.25,
            enabled: true,
        }
    }

    /// Create a test tone generator with custom frequency and amplitude.
    #[allow(dead_code)]
    pub fn with_params(sample_rate: f64, frequency: f64, amplitude: f32) -> Self {
        Self {
            phase: 0.0,
            phase_inc: frequency / sample_rate,
            amplitude,
            enabled: true,
        }
    }

    /// Generate the next sample.
    pub fn next_sample(&mut self) -> f32 {
        if !self.enabled {
            return 0.0;
        }

        let sample = (self.phase * std::f64::consts::TAU).sin() as f32 * self.amplitude;
        self.phase += self.phase_inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        sample
    }

    /// Fill a buffer with tone samples.
    pub fn fill_buffer(&mut self, buffer: &mut [f32]) {
        for sample in buffer.iter_mut() {
            *sample = self.next_sample();
        }
    }
}

/// The audio processing engine that runs inside the cpal callback.
///
/// Manages the VST3 instance, process buffers, and test tone generation.
/// Optionally receives MIDI events and passes them to the plugin.
pub struct AudioEngine {
    /// The VST3 plugin instance.
    instance: Vst3Instance,
    /// Pre-allocated process buffers.
    buffers: ProcessBuffers,
    /// Test tone generator for input signal.
    tone: TestToneGenerator,
    /// Number of output channels from the audio device.
    device_channels: usize,
    /// Temporary buffer for tone generation.
    tone_buffer: Vec<f32>,
    /// MIDI receiver (if MIDI input is connected).
    midi_receiver: Option<Arc<MidiReceiver>>,
    /// Host-side event list for passing events to the plugin.
    event_list: *mut HostEventList,
    /// Transport and timing context passed to the plugin.
    process_context: ProcessContext,
    /// Host-side parameter changes queue.
    param_changes: *mut HostParameterChanges,
    /// Pending parameter changes from control thread.
    pending_param_changes: Arc<std::sync::Mutex<Vec<(u32, f64)>>>,
    /// Whether the engine has been shut down (prevents process calls after deactivation).
    is_shutdown: bool,
}

// Safety: AudioEngine is protected by Mutex in the shared Arc. The event_list
// raw pointer is only accessed from one thread at a time via the Mutex lock.
// The Vst3Instance already implements Send.
unsafe impl Send for AudioEngine {}

impl AudioEngine {
    /// Create a new audio engine with the given VST3 instance.
    pub fn new(
        instance: Vst3Instance,
        sample_rate: f64,
        max_block_size: usize,
        device_channels: usize,
    ) -> Self {
        let input_channels = instance.input_channels;
        let output_channels = instance.output_channels;
        let buffers = ProcessBuffers::new(input_channels, output_channels, max_block_size);
        let tone = TestToneGenerator::new(sample_rate);
        let tone_buffer = vec![0.0f32; max_block_size];
        let event_list = HostEventList::new();
        let mut process_context = ProcessContext::new(sample_rate);
        process_context.set_playing(true);
        let param_changes = HostParameterChanges::new();

        debug!(
            input_channels,
            output_channels, max_block_size, device_channels, "Audio engine created"
        );

        Self {
            instance,
            buffers,
            tone,
            device_channels,
            tone_buffer,
            midi_receiver: None,
            event_list,
            process_context,
            param_changes,
            pending_param_changes: Arc::new(std::sync::Mutex::new(Vec::new())),
            is_shutdown: false,
        }
    }

    /// Set the MIDI receiver for receiving real-time MIDI events.
    pub fn set_midi_receiver(&mut self, receiver: Arc<MidiReceiver>) {
        self.midi_receiver = Some(receiver);
        debug!("MIDI receiver connected to audio engine");
    }

    /// Process one block of audio.
    ///
    /// `output` is the interleaved output buffer from cpal.
    /// The engine fills input from the test tone, processes MIDI events,
    /// calls the VST3 plugin, and writes the result to `output`.
    pub fn process(&mut self, output: &mut [f32]) {
        // Guard: do not call the VST3 plugin after shutdown or crash.
        if self.is_shutdown || self.instance.is_crashed() {
            output.fill(0.0);
            return;
        }

        let num_channels = self.device_channels;
        if num_channels == 0 {
            return;
        }

        let num_samples = output.len() / num_channels;
        if num_samples == 0 {
            return;
        }

        // Clamp to max block size
        let num_samples = num_samples.min(self.buffers.max_block_size());

        // Prepare process buffers
        self.buffers.prepare(num_samples);

        // Generate test tone input (if plugin has inputs)
        if self.buffers.num_input_channels() > 0 {
            // Generate mono tone
            self.tone.fill_buffer(&mut self.tone_buffer[..num_samples]);

            // Copy to all input channels
            for ch in 0..self.buffers.num_input_channels() {
                if let Some(buf) = self.buffers.input_buffer_mut(ch) {
                    buf.copy_from_slice(&self.tone_buffer[..num_samples]);
                }
            }
        }

        // Handle MIDI events
        unsafe {
            HostEventList::clear(self.event_list);

            if let Some(ref receiver) = self.midi_receiver {
                let messages = receiver.drain();
                if !messages.is_empty() {
                    let events = translate::translate_midi_batch(&messages);
                    for event in events {
                        HostEventList::add(self.event_list, event);
                    }
                }
            }

            // Set the event list on the process data
            self.buffers
                .set_input_events(HostEventList::as_ptr(self.event_list));
        }

        // Handle parameter changes from the control thread
        unsafe {
            HostParameterChanges::clear(self.param_changes);

            if let Ok(mut pending) = self.pending_param_changes.try_lock() {
                for (param_id, value) in pending.drain(..) {
                    HostParameterChanges::add_change(self.param_changes, param_id, 0, value);
                }
            }

            self.buffers
                .set_input_parameter_changes(HostParameterChanges::as_ptr(self.param_changes));
        }

        // Set process context (transport info)
        self.buffers
            .set_process_context(self.process_context.as_ptr());

        // Call VST3 process (sandboxed — crash protection)
        unsafe {
            let data = self.buffers.process_data_ptr();
            if !self.instance.process(data) {
                // Plugin crashed during processing — output silence
                self.is_shutdown = true;
                output.fill(0.0);
                return;
            }
        }

        // Advance transport
        self.process_context.advance(num_samples as i32);

        // Clear pointers after processing
        self.buffers.set_input_events(std::ptr::null_mut());
        self.buffers
            .set_input_parameter_changes(std::ptr::null_mut());
        self.buffers.set_process_context(std::ptr::null_mut());

        // Read output back to interleaved cpal buffer
        let actual_output_len = num_samples * num_channels;
        self.buffers
            .read_output_interleaved(&mut output[..actual_output_len], num_channels);

        // Fill any remaining samples with silence (shouldn't happen normally)
        if actual_output_len < output.len() {
            output[actual_output_len..].fill(0.0);
        }
    }

    /// Shut down the VST3 instance (stop processing + deactivate).
    ///
    /// Sets the shutdown flag first so that any subsequent audio callback
    /// (racing between lock release and stream stop) will output silence
    /// instead of calling the deactivated plugin's process method.
    pub fn shutdown(&mut self) {
        self.is_shutdown = true;
        self.instance.shutdown();
        debug!("Audio engine shut down");
    }

    /// Whether the engine has been shut down.
    #[allow(dead_code)]
    pub fn is_shutdown(&self) -> bool {
        self.is_shutdown
    }

    /// Whether the plugin instance has crashed.
    ///
    /// When true, the engine outputs silence and all further plugin
    /// COM calls are skipped. The GUI should deactivate the plugin.
    pub fn is_crashed(&self) -> bool {
        self.instance.is_crashed()
    }

    /// Get a reference to the test tone generator.
    pub fn tone(&mut self) -> &mut TestToneGenerator {
        &mut self.tone
    }

    /// Get a clone of the pending parameter changes queue.
    ///
    /// The interactive control thread pushes `(param_id, value)` pairs here;
    /// the audio callback drains them into `HostParameterChanges` each block.
    pub fn pending_param_queue(&self) -> Arc<std::sync::Mutex<Vec<(u32, f64)>>> {
        self.pending_param_changes.clone()
    }

    /// Set the tempo in BPM.
    pub fn set_tempo(&mut self, bpm: f64) {
        self.process_context.set_tempo(bpm);
    }

    /// Set the playing state.
    pub fn set_playing(&mut self, playing: bool) {
        self.process_context.set_playing(playing);
    }

    /// Set the time signature.
    pub fn set_time_signature(&mut self, numerator: u32, denominator: u32) {
        self.process_context
            .set_time_signature(numerator as i32, denominator as i32);
    }

    /// Create an IPlugView for the plugin's editor.
    ///
    /// This must be called from the main/GUI thread. The returned pointer
    /// is a COM object that the caller must release.
    pub fn create_editor_view(
        &mut self,
    ) -> Option<*mut crate::vst3::com::ComPtr<crate::vst3::com::IPlugViewVtbl>> {
        self.instance.create_editor_view()
    }

    /// Get the plugin name.
    #[allow(dead_code)]
    pub fn plugin_name(&self) -> &str {
        &self.instance.name
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        unsafe {
            HostEventList::destroy(self.event_list);
            HostParameterChanges::destroy(self.param_changes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tone_generator_basic() {
        let mut tone = TestToneGenerator::new(44100.0);
        let sample = tone.next_sample();
        // First sample should be ~0 (sin(0))
        assert!(sample.abs() < 0.01, "First sample should be near zero");
    }

    #[test]
    fn test_tone_generator_disabled() {
        let mut tone = TestToneGenerator::new(44100.0);
        tone.enabled = false;
        let sample = tone.next_sample();
        assert_eq!(sample, 0.0);
    }

    #[test]
    fn test_tone_generator_fill_buffer() {
        let mut tone = TestToneGenerator::new(44100.0);
        let mut buf = vec![0.0f32; 128];
        tone.fill_buffer(&mut buf);

        // Should have non-zero samples (sine wave)
        let max = buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(max > 0.0, "Buffer should contain non-zero samples");
        assert!(max <= 0.25, "Amplitude should not exceed 0.25");
    }

    #[test]
    fn test_tone_generator_custom_params() {
        let mut tone = TestToneGenerator::with_params(48000.0, 1000.0, 0.5);
        let mut buf = vec![0.0f32; 48];
        tone.fill_buffer(&mut buf);

        let max = buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(max > 0.0);
        assert!(max <= 0.5);
    }

    #[test]
    fn test_tone_generator_phase_wrap() {
        let mut tone = TestToneGenerator::new(44100.0);
        // Generate enough samples to wrap the phase multiple times
        let mut buf = vec![0.0f32; 44100 * 2];
        tone.fill_buffer(&mut buf);
        // Phase should have wrapped many times without issues
        assert!(tone.phase >= 0.0 && tone.phase < 1.0);
    }

    #[test]
    fn test_tone_generator_zero_amplitude_when_disabled() {
        let mut tone = TestToneGenerator::new(44100.0);
        tone.enabled = false;
        let mut buf = vec![1.0f32; 64];
        tone.fill_buffer(&mut buf);
        assert!(buf.iter().all(|&s| s == 0.0));
    }
}
