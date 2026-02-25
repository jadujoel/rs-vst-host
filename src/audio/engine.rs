//! Audio processing engine: bridges cpal audio stream with VST3 plugin processing.
//!
//! The engine owns the VST3 instance and process buffers, and runs inside
//! the cpal audio callback. A test tone generator provides input signal
//! for effect plugins.

use crate::vst3::instance::Vst3Instance;
use crate::vst3::process::ProcessBuffers;
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
}

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

        debug!(
            input_channels,
            output_channels,
            max_block_size,
            device_channels,
            "Audio engine created"
        );

        Self {
            instance,
            buffers,
            tone,
            device_channels,
            tone_buffer,
        }
    }

    /// Process one block of audio.
    ///
    /// `output` is the interleaved output buffer from cpal.
    /// The engine fills input from the test tone, calls the VST3 plugin,
    /// and writes the result to `output`.
    pub fn process(&mut self, output: &mut [f32]) {
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

        // Call VST3 process
        unsafe {
            let data = self.buffers.process_data_ptr();
            self.instance.process(data);
        }

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
    pub fn shutdown(&mut self) {
        self.instance.shutdown();
        debug!("Audio engine shut down");
    }

    /// Get a reference to the test tone generator.
    pub fn tone(&mut self) -> &mut TestToneGenerator {
        &mut self.tone
    }

    /// Get the plugin name.
    #[allow(dead_code)]
    pub fn plugin_name(&self) -> &str {
        &self.instance.name
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
}
