//! Latency compensation delay line for plugin chains.
//!
//! When plugins report processing latency via `IAudioProcessor::getLatencySamples()`,
//! upstream signals must be delayed to keep all paths in sync. This module provides
//! a simple ring-buffer delay line that can be used per-channel.

#![allow(dead_code)]

/// A sample-accurate delay line using a ring buffer.
///
/// Used for latency compensation in multi-plugin chains where different
/// plugins have different processing latencies.
pub struct DelayLine {
    /// Ring buffer storage.
    buffer: Vec<f32>,
    /// Write position.
    write_pos: usize,
    /// Delay length in samples.
    delay: usize,
}

impl DelayLine {
    /// Create a new delay line with the given maximum delay.
    pub fn new(max_delay: usize) -> Self {
        let size = max_delay.max(1);
        Self {
            buffer: vec![0.0; size],
            write_pos: 0,
            delay: 0,
        }
    }

    /// Set the delay length in samples.
    ///
    /// If `delay` exceeds the buffer capacity, it is clamped.
    pub fn set_delay(&mut self, delay: usize) {
        self.delay = delay.min(self.buffer.len());
    }

    /// Get the current delay in samples.
    pub fn delay(&self) -> usize {
        self.delay
    }

    /// Process a single sample: write to the buffer and read the delayed output.
    pub fn process_sample(&mut self, input: f32) -> f32 {
        if self.delay == 0 {
            return input;
        }

        let len = self.buffer.len();
        self.buffer[self.write_pos] = input;

        // Read from (write_pos - delay), wrapped
        let read_pos = (self.write_pos + len - self.delay) % len;
        let output = self.buffer[read_pos];

        self.write_pos = (self.write_pos + 1) % len;
        output
    }

    /// Process a block of samples in-place.
    pub fn process_block(&mut self, samples: &mut [f32]) {
        if self.delay == 0 {
            return;
        }
        for sample in samples.iter_mut() {
            *sample = self.process_sample(*sample);
        }
    }

    /// Reset the delay line (zero the buffer).
    pub fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.write_pos = 0;
    }
}

/// Stereo delay line pair for latency compensation.
pub struct StereoDelayLine {
    pub left: DelayLine,
    pub right: DelayLine,
}

impl StereoDelayLine {
    /// Create a new stereo delay line.
    pub fn new(max_delay: usize) -> Self {
        Self {
            left: DelayLine::new(max_delay),
            right: DelayLine::new(max_delay),
        }
    }

    /// Set the delay for both channels.
    pub fn set_delay(&mut self, delay: usize) {
        self.left.set_delay(delay);
        self.right.set_delay(delay);
    }

    /// Get the current delay in samples.
    pub fn delay(&self) -> usize {
        self.left.delay()
    }

    /// Reset both channels.
    pub fn reset(&mut self) {
        self.left.reset();
        self.right.reset();
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delay_line_zero_delay() {
        let mut dl = DelayLine::new(16);
        dl.set_delay(0);
        // With 0 delay, output == input
        assert_eq!(dl.process_sample(1.0), 1.0);
        assert_eq!(dl.process_sample(2.0), 2.0);
    }

    #[test]
    fn test_delay_line_one_sample() {
        let mut dl = DelayLine::new(16);
        dl.set_delay(1);
        // First output is 0 (buffer starts zeroed)
        assert_eq!(dl.process_sample(1.0), 0.0);
        // Second output is the first input
        assert_eq!(dl.process_sample(2.0), 1.0);
        assert_eq!(dl.process_sample(3.0), 2.0);
    }

    #[test]
    fn test_delay_line_multiple_samples() {
        let mut dl = DelayLine::new(16);
        dl.set_delay(3);
        assert_eq!(dl.process_sample(1.0), 0.0);
        assert_eq!(dl.process_sample(2.0), 0.0);
        assert_eq!(dl.process_sample(3.0), 0.0);
        assert_eq!(dl.process_sample(4.0), 1.0);
        assert_eq!(dl.process_sample(5.0), 2.0);
    }

    #[test]
    fn test_delay_line_wrap_around() {
        let mut dl = DelayLine::new(4);
        dl.set_delay(2);
        // Push 6 samples through (wraps the 4-element buffer)
        let outputs: Vec<f32> = (1..=6).map(|i| dl.process_sample(i as f32)).collect();
        assert_eq!(outputs, vec![0.0, 0.0, 1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn test_delay_line_process_block() {
        let mut dl = DelayLine::new(16);
        dl.set_delay(2);
        let mut block = [1.0, 2.0, 3.0, 4.0, 5.0];
        dl.process_block(&mut block);
        assert_eq!(block, [0.0, 0.0, 1.0, 2.0, 3.0]);
    }

    #[test]
    fn test_delay_line_reset() {
        let mut dl = DelayLine::new(16);
        dl.set_delay(2);
        dl.process_sample(100.0);
        dl.process_sample(200.0);
        dl.reset();
        // After reset, buffer is zeroed
        assert_eq!(dl.process_sample(1.0), 0.0);
        assert_eq!(dl.process_sample(2.0), 0.0);
        assert_eq!(dl.process_sample(3.0), 1.0);
    }

    #[test]
    fn test_delay_line_clamp_delay() {
        let mut dl = DelayLine::new(4);
        dl.set_delay(100); // Exceeds capacity
        assert_eq!(dl.delay(), 4); // Clamped to buffer size
    }

    #[test]
    fn test_stereo_delay_line() {
        let mut sdl = StereoDelayLine::new(16);
        sdl.set_delay(1);
        assert_eq!(sdl.delay(), 1);
        assert_eq!(sdl.left.process_sample(1.0), 0.0);
        assert_eq!(sdl.right.process_sample(10.0), 0.0);
        assert_eq!(sdl.left.process_sample(2.0), 1.0);
        assert_eq!(sdl.right.process_sample(20.0), 10.0);
    }

    #[test]
    fn test_stereo_delay_line_reset() {
        let mut sdl = StereoDelayLine::new(16);
        sdl.set_delay(1);
        sdl.left.process_sample(100.0);
        sdl.right.process_sample(200.0);
        sdl.reset();
        assert_eq!(sdl.left.process_sample(1.0), 0.0);
        assert_eq!(sdl.right.process_sample(1.0), 0.0);
    }

    #[test]
    fn test_delay_line_min_capacity() {
        let dl = DelayLine::new(0);
        assert_eq!(dl.buffer.len(), 1); // Min capacity is 1
    }

    #[test]
    fn test_delay_line_block_zero_delay() {
        let mut dl = DelayLine::new(16);
        dl.set_delay(0);
        let mut block = [1.0, 2.0, 3.0];
        dl.process_block(&mut block);
        // Zero delay: block unchanged
        assert_eq!(block, [1.0, 2.0, 3.0]);
    }
}
