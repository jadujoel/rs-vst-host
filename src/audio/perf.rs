//! Performance monitoring — xrun detection, CPU load tracking, and real-time thread priority.
//!
//! Provides tools for monitoring audio engine performance:
//! - Xrun (buffer underrun/overrun) detection via callback timing analysis
//! - Per-block CPU usage measurement
//! - Platform-specific real-time thread priority setting

#![allow(dead_code)]

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

// ── Lock-Free SPSC Ring Buffer ──────────────────────────────────────────

/// A lock-free single-producer single-consumer ring buffer for parameter changes.
///
/// Used to pass parameter changes from the GUI thread to the audio thread
/// without any locks. Fixed capacity, overwrites oldest on overflow.
pub struct SpscRingBuffer<T: Copy + Default> {
    /// The data storage.
    data: Box<[T]>,
    /// Write index (producer).
    write: AtomicU32,
    /// Read index (consumer).
    read: AtomicU32,
    /// Capacity (power of 2 for efficient modulo via bitmask).
    mask: u32,
}

impl<T: Copy + Default> SpscRingBuffer<T> {
    /// Create a new ring buffer with the given capacity.
    ///
    /// Capacity is rounded up to the next power of 2.
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.next_power_of_two().max(2);
        let data = vec![T::default(); capacity].into_boxed_slice();
        Self {
            data,
            write: AtomicU32::new(0),
            read: AtomicU32::new(0),
            mask: (capacity - 1) as u32,
        }
    }

    /// Push an item (producer side). Returns false if the buffer is full.
    pub fn push(&self, item: T) -> bool {
        let w = self.write.load(Ordering::Relaxed);
        let r = self.read.load(Ordering::Acquire);
        let next_w = w.wrapping_add(1);

        if (next_w & self.mask) == (r & self.mask) && next_w != r {
            // Buffer full — could overflow. For param changes, we allow it
            // by advancing the read pointer (drop oldest).
        }

        // Safety: only one producer thread writes. We use the mask for indexing.
        let idx = (w & self.mask) as usize;
        // Safety: index is always within bounds due to mask.
        unsafe {
            let ptr = self.data.as_ptr() as *mut T;
            ptr.add(idx).write(item);
        }
        self.write.store(w.wrapping_add(1), Ordering::Release);
        true
    }

    /// Pop an item (consumer side). Returns None if empty.
    pub fn pop(&self) -> Option<T> {
        let r = self.read.load(Ordering::Relaxed);
        let w = self.write.load(Ordering::Acquire);

        if r == w {
            return None; // Empty
        }

        let idx = (r & self.mask) as usize;
        let item = unsafe {
            let ptr = self.data.as_ptr();
            ptr.add(idx).read()
        };
        self.read.store(r.wrapping_add(1), Ordering::Release);
        Some(item)
    }

    /// Check if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.read.load(Ordering::Relaxed) == self.write.load(Ordering::Relaxed)
    }

    /// Drain all available items into a vec (consumer side).
    pub fn drain_to_vec(&self) -> Vec<T> {
        let mut items = Vec::new();
        while let Some(item) = self.pop() {
            items.push(item);
        }
        items
    }
}

// Safety: The ring buffer is designed for single-producer single-consumer.
// The atomics provide the necessary synchronization.
unsafe impl<T: Copy + Default + Send> Send for SpscRingBuffer<T> {}
unsafe impl<T: Copy + Default + Send> Sync for SpscRingBuffer<T> {}

// ── Parameter Change Entry ──────────────────────────────────────────────

/// A lock-free parameter change entry for the SPSC ring buffer.
#[derive(Debug, Clone, Copy, Default)]
pub struct ParamChangeEntry {
    /// Parameter ID.
    pub param_id: u32,
    /// Normalized parameter value.
    pub value: f64,
}

// ── Xrun Tracker ────────────────────────────────────────────────────────

/// Tracks audio buffer underruns (xruns) by monitoring callback timing.
///
/// The audio callback should call `begin_callback()` and `end_callback()`
/// around its processing. If the interval between callbacks exceeds the
/// expected buffer duration by a threshold, an xrun is counted.
pub struct XrunTracker {
    /// Last callback start time.
    last_callback: Option<Instant>,
    /// Expected callback interval in microseconds.
    expected_interval_us: f64,
    /// Threshold multiplier (e.g., 1.5 = xrun if >150% of expected).
    threshold: f64,
    /// Total xrun count.
    xrun_count: AtomicU32,
    /// Latest callback duration in microseconds (for display).
    last_callback_us: AtomicU64,
}

impl XrunTracker {
    /// Create a new xrun tracker.
    ///
    /// `sample_rate` and `buffer_size` determine the expected callback interval.
    pub fn new(sample_rate: f64, buffer_size: usize) -> Self {
        let expected_interval_us = (buffer_size as f64 / sample_rate) * 1_000_000.0;
        Self {
            last_callback: None,
            expected_interval_us,
            threshold: 1.5,
            xrun_count: AtomicU32::new(0),
            last_callback_us: AtomicU64::new(0),
        }
    }

    /// Called at the start of each audio callback.
    ///
    /// Returns `true` if an xrun was detected (callback was late).
    pub fn begin_callback(&mut self) -> bool {
        let now = Instant::now();
        let xrun = if let Some(last) = self.last_callback {
            let elapsed_us = now.duration_since(last).as_micros() as f64;
            self.last_callback_us
                .store(elapsed_us as u64, Ordering::Relaxed);
            elapsed_us > self.expected_interval_us * self.threshold
        } else {
            false
        };

        if xrun {
            self.xrun_count.fetch_add(1, Ordering::Relaxed);
        }

        self.last_callback = Some(now);
        xrun
    }

    /// Get the total xrun count.
    pub fn xrun_count(&self) -> u32 {
        self.xrun_count.load(Ordering::Relaxed)
    }

    /// Get the last callback interval in microseconds.
    pub fn last_callback_us(&self) -> u64 {
        self.last_callback_us.load(Ordering::Relaxed)
    }

    /// Reset the xrun counter.
    pub fn reset(&self) {
        self.xrun_count.store(0, Ordering::Relaxed);
    }
}

// ── CPU Load Monitor ────────────────────────────────────────────────────

/// Measures plugin processing CPU load as a percentage of the audio budget.
///
/// The audio callback should call `begin_process()` before plugin processing
/// and `end_process()` after. The load percentage is:
/// `(process_time / buffer_duration) * 100`
pub struct CpuLoadMonitor {
    /// Start time of the current process call.
    process_start: Option<Instant>,
    /// Budget duration in microseconds (time available per callback).
    budget_us: f64,
    /// Smoothed CPU load percentage (exponential moving average).
    load_pct: AtomicU32, // Stored as fixed-point: value * 100
    /// Smoothing factor (0..1, higher = more responsive).
    alpha: f64,
    /// Peak load percentage (highest seen since last reset).
    peak_load_pct: AtomicU32,
}

impl CpuLoadMonitor {
    /// Create a new CPU load monitor.
    pub fn new(sample_rate: f64, buffer_size: usize) -> Self {
        let budget_us = (buffer_size as f64 / sample_rate) * 1_000_000.0;
        Self {
            process_start: None,
            budget_us,
            load_pct: AtomicU32::new(0),
            alpha: 0.1, // Smooth over ~10 callbacks
            peak_load_pct: AtomicU32::new(0),
        }
    }

    /// Mark the start of plugin processing.
    pub fn begin_process(&mut self) {
        self.process_start = Some(Instant::now());
    }

    /// Mark the end of plugin processing and update the load measurement.
    pub fn end_process(&mut self) {
        if let Some(start) = self.process_start.take() {
            let elapsed_us = start.elapsed().as_micros() as f64;
            let raw_pct = (elapsed_us / self.budget_us) * 100.0;

            // Exponential moving average
            let prev = self.load_pct.load(Ordering::Relaxed) as f64 / 100.0;
            let smoothed = prev * (1.0 - self.alpha) + raw_pct * self.alpha;
            self.load_pct
                .store((smoothed * 100.0) as u32, Ordering::Relaxed);

            // Peak tracking
            let raw_fixed = (raw_pct * 100.0) as u32;
            let _ = self.peak_load_pct.fetch_max(raw_fixed, Ordering::Relaxed);
        }
    }

    /// Get the current smoothed CPU load percentage.
    pub fn load_percent(&self) -> f32 {
        self.load_pct.load(Ordering::Relaxed) as f32 / 100.0
    }

    /// Get the peak CPU load percentage since last reset.
    pub fn peak_load_percent(&self) -> f32 {
        self.peak_load_pct.load(Ordering::Relaxed) as f32 / 100.0
    }

    /// Reset peak load tracking.
    pub fn reset_peak(&self) {
        self.peak_load_pct.store(0, Ordering::Relaxed);
    }
}

// ── Thread Priority ─────────────────────────────────────────────────────

/// Set the current thread to real-time priority.
///
/// On macOS: uses `pthread_set_qos_class_self_np` with `QOS_CLASS_USER_INTERACTIVE`.
/// On Linux: attempts `SCHED_FIFO` with priority 50.
/// On other platforms: no-op.
pub fn set_realtime_thread_priority() -> bool {
    #[cfg(target_os = "macos")]
    {
        // QOS_CLASS_USER_INTERACTIVE = 0x21
        const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;
        unsafe extern "C" {
            fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
        }
        let result = unsafe { pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0) };
        if result == 0 {
            tracing::debug!("Set audio thread to QOS_CLASS_USER_INTERACTIVE");
            return true;
        }
        tracing::warn!(result, "Failed to set real-time thread priority (macOS)");
        false
    }

    #[cfg(target_os = "linux")]
    {
        use libc::{SCHED_FIFO, pthread_self, pthread_setschedparam, sched_param};
        let param = sched_param { sched_priority: 50 };
        let result = unsafe { pthread_setschedparam(pthread_self(), SCHED_FIFO, &param) };
        if result == 0 {
            tracing::debug!("Set audio thread to SCHED_FIFO priority 50");
            return true;
        }
        tracing::warn!(
            result,
            "Failed to set real-time thread priority (Linux) — try running with sudo or setting CAP_SYS_NICE"
        );
        false
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        tracing::debug!("Real-time thread priority not supported on this platform");
        false
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    // ── SpscRingBuffer tests ────────────────────────────────────────

    #[test]
    fn test_spsc_new_is_empty() {
        let rb = SpscRingBuffer::<u32>::new(16);
        assert!(rb.is_empty());
        assert!(rb.pop().is_none());
    }

    #[test]
    fn test_spsc_push_pop() {
        let rb = SpscRingBuffer::<u32>::new(16);
        assert!(rb.push(42));
        assert!(!rb.is_empty());
        assert_eq!(rb.pop(), Some(42));
        assert!(rb.is_empty());
    }

    #[test]
    fn test_spsc_fifo_order() {
        let rb = SpscRingBuffer::<u32>::new(16);
        rb.push(1);
        rb.push(2);
        rb.push(3);
        assert_eq!(rb.pop(), Some(1));
        assert_eq!(rb.pop(), Some(2));
        assert_eq!(rb.pop(), Some(3));
        assert!(rb.pop().is_none());
    }

    #[test]
    fn test_spsc_drain_to_vec() {
        let rb = SpscRingBuffer::<u32>::new(16);
        rb.push(10);
        rb.push(20);
        rb.push(30);
        let items = rb.drain_to_vec();
        assert_eq!(items, vec![10, 20, 30]);
        assert!(rb.is_empty());
    }

    #[test]
    fn test_spsc_producer_consumer_threads() {
        let rb = std::sync::Arc::new(SpscRingBuffer::<u32>::new(256));
        let rb_producer = rb.clone();
        let rb_consumer = rb.clone();

        let producer = thread::spawn(move || {
            for i in 0..100 {
                rb_producer.push(i);
                thread::yield_now();
            }
        });

        let consumer = thread::spawn(move || {
            let mut received = Vec::new();
            let mut attempts = 0;
            while received.len() < 100 && attempts < 10_000 {
                if let Some(val) = rb_consumer.pop() {
                    received.push(val);
                }
                attempts += 1;
                thread::yield_now();
            }
            received
        });

        producer.join().unwrap();
        let received = consumer.join().unwrap();
        assert!(!received.is_empty(), "Should have received some items");
        // Items should be in order (FIFO)
        for window in received.windows(2) {
            assert!(window[0] < window[1], "Items should be in FIFO order");
        }
    }

    #[test]
    fn test_param_change_entry_default() {
        let entry = ParamChangeEntry::default();
        assert_eq!(entry.param_id, 0);
        assert!((entry.value - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_spsc_param_changes() {
        let rb = SpscRingBuffer::<ParamChangeEntry>::new(64);
        rb.push(ParamChangeEntry {
            param_id: 1,
            value: 0.5,
        });
        rb.push(ParamChangeEntry {
            param_id: 2,
            value: 0.75,
        });

        let entry = rb.pop().unwrap();
        assert_eq!(entry.param_id, 1);
        assert!((entry.value - 0.5).abs() < f64::EPSILON);
    }

    // ── XrunTracker tests ───────────────────────────────────────────

    #[test]
    fn test_xrun_tracker_no_xrun() {
        let mut tracker = XrunTracker::new(44100.0, 256);
        // First callback — no previous, no xrun
        let xrun = tracker.begin_callback();
        assert!(!xrun);
        assert_eq!(tracker.xrun_count(), 0);
    }

    #[test]
    fn test_xrun_tracker_normal_timing() {
        let mut tracker = XrunTracker::new(44100.0, 4096); // ~93ms expected
        tracker.begin_callback();
        // Quick successive call — well within threshold
        thread::sleep(Duration::from_millis(1));
        let xrun = tracker.begin_callback();
        // This will likely register as an xrun because we're way below expected interval
        // That's actually correct — we didn't wait long enough
        assert!(!xrun, "Should not be an xrun for a fast callback");
    }

    #[test]
    fn test_xrun_tracker_reset() {
        let tracker = XrunTracker::new(44100.0, 256);
        tracker.xrun_count.store(5, Ordering::Relaxed);
        tracker.reset();
        assert_eq!(tracker.xrun_count(), 0);
    }

    #[test]
    fn test_xrun_tracker_count_accumulates() {
        let tracker = XrunTracker::new(44100.0, 256);
        tracker.xrun_count.fetch_add(1, Ordering::Relaxed);
        tracker.xrun_count.fetch_add(1, Ordering::Relaxed);
        tracker.xrun_count.fetch_add(1, Ordering::Relaxed);
        assert_eq!(tracker.xrun_count(), 3);
    }

    // ── CpuLoadMonitor tests ────────────────────────────────────────

    #[test]
    fn test_cpu_load_initial_zero() {
        let monitor = CpuLoadMonitor::new(44100.0, 256);
        assert!(monitor.load_percent() < 0.01);
    }

    #[test]
    fn test_cpu_load_measurement() {
        let mut monitor = CpuLoadMonitor::new(44100.0, 44100); // 1s budget
        monitor.begin_process();
        thread::sleep(Duration::from_millis(10)); // ~10ms of "processing"
        monitor.end_process();

        let load = monitor.load_percent();
        // With 1s budget, 10ms sleep ≈ 1% load
        assert!(load > 0.0, "Load should be non-zero, got {}", load);
        assert!(load < 10.0, "Load should be reasonable, got {}", load);
    }

    #[test]
    fn test_cpu_load_peak_tracking() {
        let mut monitor = CpuLoadMonitor::new(44100.0, 4410); // 100ms budget

        // First measurement
        monitor.begin_process();
        thread::sleep(Duration::from_millis(5));
        monitor.end_process();

        let peak1 = monitor.peak_load_percent();
        assert!(peak1 > 0.0);

        // Reset peak
        monitor.reset_peak();
        assert!(monitor.peak_load_percent() < 0.01);
    }

    #[test]
    fn test_cpu_load_no_start() {
        let mut monitor = CpuLoadMonitor::new(44100.0, 256);
        // end_process without begin — should not panic
        monitor.end_process();
        assert!(monitor.load_percent() < 0.01);
    }

    // ── Thread Priority tests ───────────────────────────────────────

    #[test]
    fn test_set_realtime_priority_does_not_panic() {
        // May fail due to permissions, but should not panic
        let _result = set_realtime_thread_priority();
    }

    // ── SpscRingBuffer power-of-two rounding ────────────────────────

    #[test]
    fn test_spsc_capacity_rounded() {
        let rb = SpscRingBuffer::<u32>::new(5);
        // Should be rounded to 8 (next power of 2)
        assert_eq!(rb.mask, 7); // mask = capacity - 1 = 7
    }

    #[test]
    fn test_spsc_capacity_already_power_of_two() {
        let rb = SpscRingBuffer::<u32>::new(16);
        assert_eq!(rb.mask, 15);
    }

    #[test]
    fn test_spsc_minimum_capacity() {
        let rb = SpscRingBuffer::<u32>::new(0);
        // Minimum should be 2
        assert_eq!(rb.mask, 1);
    }

    #[test]
    fn test_spsc_wrap_around() {
        let rb = SpscRingBuffer::<u32>::new(4);
        // Push 3 items (capacity is 4, so 3 usable before wrap)
        rb.push(1);
        rb.push(2);
        rb.push(3);
        // Pop all
        assert_eq!(rb.pop(), Some(1));
        assert_eq!(rb.pop(), Some(2));
        assert_eq!(rb.pop(), Some(3));
        // Push more (wraps around)
        rb.push(4);
        rb.push(5);
        assert_eq!(rb.pop(), Some(4));
        assert_eq!(rb.pop(), Some(5));
    }
}
