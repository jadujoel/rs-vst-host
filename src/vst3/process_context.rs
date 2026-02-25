//! ProcessContext — transport and timing information for VST3 plugins.
//!
//! Plugins receive a pointer to `ProcessContext` in `ProcessData::processContext`.
//! This provides tempo, time signature, sample position, and transport state.

use std::ffi::c_void;

// ─── State flags ──────────────────────────────────────────────────────────

/// Transport is playing.
pub const K_PLAYING: u32 = 1 << 1;
/// Tempo is valid.
pub const K_TEMPO_VALID: u32 = 1 << 10;
/// Time signature is valid.
pub const K_TIME_SIG_VALID: u32 = 1 << 13;
/// System time is valid.
#[allow(dead_code)]
pub const K_SYSTEM_TIME_VALID: u32 = 1 << 8;
/// Project time (sample position) is valid.
pub const K_PROJECT_TIME_VALID: u32 = 1 << 9;
/// Bar position is valid.
pub const K_BAR_POSITION_VALID: u32 = 1 << 11;
/// Cycle (loop) is active.
#[allow(dead_code)]
pub const K_CYCLE_ACTIVE: u32 = 1 << 2;

/// ProcessContext — matches the VST3 SDK `ProcessContext` struct layout.
///
/// This struct is passed to plugins via `ProcessData::processContext`.
/// Fields are valid only when their corresponding state flag is set.
#[repr(C)]
#[derive(Clone)]
pub struct ProcessContext {
    /// Combination of state flags (K_PLAYING, K_TEMPO_VALID, etc.).
    pub state: u32,
    /// Sample rate as provided by setup.
    pub sample_rate: f64,
    /// Project time in samples (since project start).
    pub project_time_samples: i64,
    /// System time in nanoseconds (from system clock).
    pub system_time: i64,
    /// Musical position in quarter notes (from bar 1, beat 1).
    pub project_time_music: f64,
    /// Last bar start position in quarter notes.
    pub bar_position_music: f64,
    /// Cycle start in quarter notes.
    pub cycle_start_music: f64,
    /// Cycle end in quarter notes.
    pub cycle_end_music: f64,
    /// Tempo in BPM.
    pub tempo: f64,
    /// Time signature numerator.
    pub time_sig_numerator: i32,
    /// Time signature denominator.
    pub time_sig_denominator: i32,
    /// MIDI chord (not commonly used).
    pub chord: i32,
    /// SMPTE offset sub-frames.
    pub smpte_offset_sub_frames: i32,
    /// Frame rate (encoded).
    pub frame_rate: FrameRate,
    /// Samples to next clock.
    pub samples_to_next_clock: i32,
}

/// SMPTE frame rate encoding.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FrameRate {
    /// Frames per second.
    pub frames_per_second: u32,
    /// Flags (e.g., drop frame).
    pub flags: u32,
}

impl ProcessContext {
    /// Create a new ProcessContext with sensible defaults.
    ///
    /// Sets tempo = 120 BPM, 4/4 time signature, and marks those fields valid.
    pub fn new(sample_rate: f64) -> Self {
        Self {
            state: K_TEMPO_VALID | K_TIME_SIG_VALID | K_PROJECT_TIME_VALID,
            sample_rate,
            project_time_samples: 0,
            system_time: 0,
            project_time_music: 0.0,
            bar_position_music: 0.0,
            cycle_start_music: 0.0,
            cycle_end_music: 0.0,
            tempo: 120.0,
            time_sig_numerator: 4,
            time_sig_denominator: 4,
            chord: 0,
            smpte_offset_sub_frames: 0,
            frame_rate: FrameRate::default(),
            samples_to_next_clock: 0,
        }
    }

    /// Set tempo in BPM.
    pub fn set_tempo(&mut self, bpm: f64) {
        self.tempo = bpm;
        self.state |= K_TEMPO_VALID;
    }

    /// Set time signature.
    #[allow(dead_code)]
    pub fn set_time_signature(&mut self, numerator: i32, denominator: i32) {
        self.time_sig_numerator = numerator;
        self.time_sig_denominator = denominator;
        self.state |= K_TIME_SIG_VALID;
    }

    /// Set transport to playing.
    pub fn set_playing(&mut self, playing: bool) {
        if playing {
            self.state |= K_PLAYING;
        } else {
            self.state &= !K_PLAYING;
        }
    }

    /// Advance the transport by `num_samples` frames.
    ///
    /// Updates project_time_samples and project_time_music.
    pub fn advance(&mut self, num_samples: i32) {
        self.project_time_samples += num_samples as i64;

        // Calculate musical position: quarters = samples / (sample_rate * 60 / tempo)
        if self.sample_rate > 0.0 && self.tempo > 0.0 {
            let seconds = self.project_time_samples as f64 / self.sample_rate;
            let beats = seconds * self.tempo / 60.0;
            self.project_time_music = beats;

            // Calculate bar position
            if self.time_sig_numerator > 0 {
                let beats_per_bar = self.time_sig_numerator as f64;
                let bar_number = (beats / beats_per_bar).floor();
                self.bar_position_music = bar_number * beats_per_bar;
                self.state |= K_BAR_POSITION_VALID;
            }
        }
    }

    /// Get a raw pointer suitable for setting on ProcessData::processContext.
    pub fn as_ptr(&mut self) -> *mut c_void {
        self as *mut Self as *mut c_void
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem;

    #[test]
    fn test_process_context_new_defaults() {
        let ctx = ProcessContext::new(44100.0);
        assert_eq!(ctx.sample_rate, 44100.0);
        assert_eq!(ctx.tempo, 120.0);
        assert_eq!(ctx.time_sig_numerator, 4);
        assert_eq!(ctx.time_sig_denominator, 4);
        assert_eq!(ctx.project_time_samples, 0);
        assert_eq!(ctx.project_time_music, 0.0);
    }

    #[test]
    fn test_process_context_state_flags() {
        let ctx = ProcessContext::new(44100.0);
        assert_ne!(ctx.state & K_TEMPO_VALID, 0);
        assert_ne!(ctx.state & K_TIME_SIG_VALID, 0);
        assert_ne!(ctx.state & K_PROJECT_TIME_VALID, 0);
        assert_eq!(ctx.state & K_PLAYING, 0); // Not playing by default
    }

    #[test]
    fn test_set_playing() {
        let mut ctx = ProcessContext::new(44100.0);
        ctx.set_playing(true);
        assert_ne!(ctx.state & K_PLAYING, 0);
        ctx.set_playing(false);
        assert_eq!(ctx.state & K_PLAYING, 0);
    }

    #[test]
    fn test_set_tempo() {
        let mut ctx = ProcessContext::new(44100.0);
        ctx.set_tempo(140.0);
        assert_eq!(ctx.tempo, 140.0);
        assert_ne!(ctx.state & K_TEMPO_VALID, 0);
    }

    #[test]
    fn test_set_time_signature() {
        let mut ctx = ProcessContext::new(44100.0);
        ctx.set_time_signature(3, 4);
        assert_eq!(ctx.time_sig_numerator, 3);
        assert_eq!(ctx.time_sig_denominator, 4);
    }

    #[test]
    fn test_advance_transport() {
        let mut ctx = ProcessContext::new(44100.0);
        ctx.set_tempo(120.0);
        ctx.set_playing(true);

        // At 120 BPM, 44100 samples = 1 second = 2 beats
        ctx.advance(44100);
        assert_eq!(ctx.project_time_samples, 44100);
        assert!((ctx.project_time_music - 2.0).abs() < 0.001);
    }

    #[test]
    fn test_advance_bar_position() {
        let mut ctx = ProcessContext::new(44100.0);
        ctx.set_tempo(120.0);
        ctx.set_time_signature(4, 4);

        // Advance to 4 beats (1 bar at 4/4)
        // 4 beats at 120 BPM = 2 seconds = 88200 samples
        ctx.advance(88200);
        assert!((ctx.project_time_music - 4.0).abs() < 0.001);
        assert!((ctx.bar_position_music - 4.0).abs() < 0.001);
        assert_ne!(ctx.state & K_BAR_POSITION_VALID, 0);
    }

    #[test]
    fn test_advance_incremental() {
        let mut ctx = ProcessContext::new(44100.0);
        ctx.set_tempo(120.0);

        // Advance 512 samples at a time
        for _ in 0..86 {
            ctx.advance(512);
        }
        // 86 * 512 = 44032 samples ≈ 0.998 seconds ≈ 1.997 beats
        assert!(ctx.project_time_music > 1.9);
        assert!(ctx.project_time_music < 2.1);
    }

    #[test]
    fn test_process_context_size_reasonable() {
        // The struct should be a reasonable size
        let size = mem::size_of::<ProcessContext>();
        assert!(size >= 100, "ProcessContext should be at least 100 bytes, got {}", size);
        assert!(size <= 200, "ProcessContext should not exceed 200 bytes, got {}", size);
    }

    #[test]
    fn test_as_ptr_not_null() {
        let mut ctx = ProcessContext::new(44100.0);
        let ptr = ctx.as_ptr();
        assert!(!ptr.is_null());
    }
}
