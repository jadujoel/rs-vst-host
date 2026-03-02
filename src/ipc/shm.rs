//! POSIX shared memory audio buffer for zero-copy audio transfer between processes.
//!
//! Uses `shm_open`/`mmap` to create a shared memory region that both the host
//! and plugin processes can read/write. Audio data is laid out as:
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────┐
//! │ Header (64 bytes)                                            │
//! │   ready_flag: u32     — set by writer, cleared by reader     │
//! │   num_samples: u32    — samples in current block             │
//! │   input_channels: u32                                        │
//! │   output_channels: u32                                       │
//! │   _reserved: [u8; 48]                                        │
//! ├──────────────────────────────────────────────────────────────┤
//! │ Input audio data (input_channels × max_block_size × f32)     │
//! ├──────────────────────────────────────────────────────────────┤
//! │ Output audio data (output_channels × max_block_size × f32)   │
//! └──────────────────────────────────────────────────────────────┘
//! ```

use std::ffi::CString;
use std::ptr;

/// Header at the start of shared memory — layout must be stable.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ShmHeader {
    /// Ready flag: 1 = data available, 0 = consumed.
    pub ready_flag: u32,
    /// Number of samples in the current block.
    pub num_samples: u32,
    /// Number of input channels.
    pub input_channels: u32,
    /// Number of output channels.
    pub output_channels: u32,
    /// Reserved for future use.
    pub _reserved: [u8; 48],
}

const HEADER_SIZE: usize = std::mem::size_of::<ShmHeader>();

// Verify header is 64 bytes as documented.
const _: () = assert!(HEADER_SIZE == 64);

/// A shared memory audio buffer region.
///
/// Manages POSIX shared memory for transferring audio data between
/// the host process and the plugin worker process.
pub struct ShmAudioBuffer {
    /// Name of the shared memory object (e.g., "/rs-vst-host-12345").
    name: String,
    /// Pointer to the mapped memory region.
    ptr: *mut u8,
    /// Total size of the mapped region in bytes.
    size: usize,
    /// Maximum number of samples per channel per block.
    max_block_size: usize,
    /// Number of input channels.
    input_channels: usize,
    /// Number of output channels.
    output_channels: usize,
    /// Whether this instance created (owns) the shm object.
    is_owner: bool,
}

// Safety: The shared memory region is accessed via atomic-style
// ready_flag coordination. Only one side writes at a time.
unsafe impl Send for ShmAudioBuffer {}

impl ShmAudioBuffer {
    /// Calculate the total shared memory size needed.
    pub fn required_size(
        input_channels: usize,
        output_channels: usize,
        max_block_size: usize,
    ) -> usize {
        let audio_bytes =
            (input_channels + output_channels) * max_block_size * std::mem::size_of::<f32>();
        HEADER_SIZE + audio_bytes
    }

    /// Create a new shared memory region (host side — creates and maps).
    ///
    /// The `name` should be unique per plugin process (e.g., include PID).
    pub fn create(
        name: &str,
        input_channels: usize,
        output_channels: usize,
        max_block_size: usize,
    ) -> Result<Self, String> {
        let size = Self::required_size(input_channels, output_channels, max_block_size);
        let c_name =
            CString::new(name).map_err(|e| format!("Invalid shm name '{}': {}", name, e))?;

        unsafe {
            // Create shared memory object
            let fd = libc::shm_open(
                c_name.as_ptr(),
                libc::O_CREAT | libc::O_RDWR | libc::O_EXCL,
                0o600,
            );
            if fd < 0 {
                return Err(format!(
                    "shm_open failed for '{}': {}",
                    name,
                    std::io::Error::last_os_error()
                ));
            }

            // Set size
            if libc::ftruncate(fd, size as libc::off_t) != 0 {
                let err = std::io::Error::last_os_error();
                libc::close(fd);
                libc::shm_unlink(c_name.as_ptr());
                return Err(format!("ftruncate failed for '{}': {}", name, err));
            }

            // Map into address space
            let ptr = libc::mmap(
                ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            );
            libc::close(fd);

            if ptr == libc::MAP_FAILED {
                libc::shm_unlink(c_name.as_ptr());
                return Err(format!(
                    "mmap failed for '{}': {}",
                    name,
                    std::io::Error::last_os_error()
                ));
            }

            // Zero-initialize
            ptr::write_bytes(ptr as *mut u8, 0, size);

            // Write header
            let header = ptr as *mut ShmHeader;
            (*header).input_channels = input_channels as u32;
            (*header).output_channels = output_channels as u32;

            Ok(Self {
                name: name.to_string(),
                ptr: ptr as *mut u8,
                size,
                max_block_size,
                input_channels,
                output_channels,
                is_owner: true,
            })
        }
    }

    /// Open an existing shared memory region (worker side — opens and maps).
    pub fn open(
        name: &str,
        input_channels: usize,
        output_channels: usize,
        max_block_size: usize,
    ) -> Result<Self, String> {
        let size = Self::required_size(input_channels, output_channels, max_block_size);
        let c_name =
            CString::new(name).map_err(|e| format!("Invalid shm name '{}': {}", name, e))?;

        unsafe {
            let fd = libc::shm_open(c_name.as_ptr(), libc::O_RDWR, 0);
            if fd < 0 {
                return Err(format!(
                    "shm_open failed for '{}': {}",
                    name,
                    std::io::Error::last_os_error()
                ));
            }

            let ptr = libc::mmap(
                ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            );
            libc::close(fd);

            if ptr == libc::MAP_FAILED {
                return Err(format!(
                    "mmap failed for '{}': {}",
                    name,
                    std::io::Error::last_os_error()
                ));
            }

            Ok(Self {
                name: name.to_string(),
                ptr: ptr as *mut u8,
                size,
                max_block_size,
                input_channels,
                output_channels,
                is_owner: false,
            })
        }
    }

    /// Get the shared memory name (for passing to the child process).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get a pointer to the header.
    fn header(&self) -> *mut ShmHeader {
        self.ptr as *mut ShmHeader
    }

    /// Get the input audio buffer for a specific channel as a mutable slice.
    ///
    /// # Safety
    /// Caller must ensure exclusive write access (via ready_flag protocol).
    pub unsafe fn input_channel_mut(&self, channel: usize) -> Option<&mut [f32]> {
        if channel >= self.input_channels {
            return None;
        }
        let offset = HEADER_SIZE + channel * self.max_block_size * std::mem::size_of::<f32>();
        let ptr = unsafe { self.ptr.add(offset) as *mut f32 };
        Some(unsafe { std::slice::from_raw_parts_mut(ptr, self.max_block_size) })
    }

    /// Get the output audio buffer for a specific channel as a mutable slice.
    ///
    /// # Safety
    /// Caller must ensure exclusive write access (via ready_flag protocol).
    pub unsafe fn output_channel_mut(&self, channel: usize) -> Option<&mut [f32]> {
        if channel >= self.output_channels {
            return None;
        }
        let input_size = self.input_channels * self.max_block_size * std::mem::size_of::<f32>();
        let offset =
            HEADER_SIZE + input_size + channel * self.max_block_size * std::mem::size_of::<f32>();
        let ptr = unsafe { self.ptr.add(offset) as *mut f32 };
        Some(unsafe { std::slice::from_raw_parts_mut(ptr, self.max_block_size) })
    }

    /// Get the output audio buffer for a specific channel as a read-only slice.
    ///
    /// # Safety
    /// Caller must ensure the writer has finished (via ready_flag protocol).
    pub unsafe fn output_channel(&self, channel: usize) -> Option<&[f32]> {
        if channel >= self.output_channels {
            return None;
        }
        let input_size = self.input_channels * self.max_block_size * std::mem::size_of::<f32>();
        let offset =
            HEADER_SIZE + input_size + channel * self.max_block_size * std::mem::size_of::<f32>();
        let ptr = unsafe { self.ptr.add(offset) as *const f32 };
        Some(unsafe { std::slice::from_raw_parts(ptr, self.max_block_size) })
    }

    /// Get the input audio buffer for a specific channel as a read-only slice.
    ///
    /// # Safety
    /// Caller must ensure the writer has finished (via ready_flag protocol).
    pub unsafe fn input_channel(&self, channel: usize) -> Option<&[f32]> {
        if channel >= self.input_channels {
            return None;
        }
        let offset = HEADER_SIZE + channel * self.max_block_size * std::mem::size_of::<f32>();
        let ptr = unsafe { self.ptr.add(offset) as *const f32 };
        Some(unsafe { std::slice::from_raw_parts(ptr, self.max_block_size) })
    }

    /// Set the number of samples in the current block.
    pub fn set_num_samples(&self, num_samples: u32) {
        unsafe {
            (*self.header()).num_samples = num_samples;
        }
    }

    /// Get the number of samples in the current block.
    pub fn num_samples(&self) -> u32 {
        unsafe { (*self.header()).num_samples }
    }

    /// Set the ready flag (writer signals that data is available).
    pub fn set_ready(&self) {
        unsafe {
            std::sync::atomic::fence(std::sync::atomic::Ordering::Release);
            (*self.header()).ready_flag = 1;
        }
    }

    /// Clear the ready flag (reader signals data has been consumed).
    pub fn clear_ready(&self) {
        unsafe {
            (*self.header()).ready_flag = 0;
            std::sync::atomic::fence(std::sync::atomic::Ordering::Release);
        }
    }

    /// Check if the ready flag is set.
    pub fn is_ready(&self) -> bool {
        unsafe {
            std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
            (*self.header()).ready_flag != 0
        }
    }

    /// Maximum block size.
    pub fn max_block_size(&self) -> usize {
        self.max_block_size
    }

    /// Number of input channels.
    pub fn input_channels(&self) -> usize {
        self.input_channels
    }

    /// Number of output channels.
    pub fn output_channels(&self) -> usize {
        self.output_channels
    }

    /// Total mapped size in bytes.
    #[allow(dead_code)]
    pub fn size(&self) -> usize {
        self.size
    }
}

impl Drop for ShmAudioBuffer {
    fn drop(&mut self) {
        unsafe {
            if !self.ptr.is_null() {
                libc::munmap(self.ptr as *mut libc::c_void, self.size);
            }
            if self.is_owner {
                if let Ok(c_name) = CString::new(self.name.as_str()) {
                    libc::shm_unlink(c_name.as_ptr());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_name() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        format!(
            "/rs-vst-host-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        )
    }

    #[test]
    fn test_header_size_is_64() {
        assert_eq!(std::mem::size_of::<ShmHeader>(), 64);
    }

    #[test]
    fn test_required_size_calculation() {
        // 2 input + 2 output channels, 1024 samples, f32 = 4 bytes
        let size = ShmAudioBuffer::required_size(2, 2, 1024);
        assert_eq!(size, 64 + (2 + 2) * 1024 * 4);
    }

    #[test]
    fn test_required_size_zero_channels() {
        let size = ShmAudioBuffer::required_size(0, 0, 1024);
        assert_eq!(size, 64); // Just the header
    }

    #[test]
    fn test_create_and_drop() {
        let name = unique_name();
        let shm = ShmAudioBuffer::create(&name, 2, 2, 1024).unwrap();
        assert_eq!(shm.input_channels(), 2);
        assert_eq!(shm.output_channels(), 2);
        assert_eq!(shm.max_block_size(), 1024);
        assert!(shm.is_owner);
        drop(shm);
    }

    #[test]
    fn test_create_and_open() {
        let name = unique_name();
        let shm_host = ShmAudioBuffer::create(&name, 2, 2, 512).unwrap();

        let shm_worker = ShmAudioBuffer::open(&name, 2, 2, 512).unwrap();
        assert!(!shm_worker.is_owner);
        assert_eq!(shm_worker.size, shm_host.size);

        drop(shm_worker);
        drop(shm_host);
    }

    #[test]
    fn test_write_input_read_from_worker() {
        let name = unique_name();
        let shm_host = ShmAudioBuffer::create(&name, 1, 1, 256).unwrap();
        let shm_worker = ShmAudioBuffer::open(&name, 1, 1, 256).unwrap();

        // Host writes input
        unsafe {
            let buf = shm_host.input_channel_mut(0).unwrap();
            for (i, s) in buf.iter_mut().enumerate() {
                *s = i as f32 * 0.01;
            }
        }
        shm_host.set_num_samples(256);
        shm_host.set_ready();

        // Worker reads input
        assert!(shm_worker.is_ready());
        assert_eq!(shm_worker.num_samples(), 256);
        unsafe {
            let buf = shm_worker.input_channel(0).unwrap();
            assert!((buf[0] - 0.0).abs() < f32::EPSILON);
            assert!((buf[1] - 0.01).abs() < f32::EPSILON);
            assert!((buf[100] - 1.0).abs() < f32::EPSILON);
        }

        drop(shm_worker);
        drop(shm_host);
    }

    #[test]
    fn test_write_output_read_from_host() {
        let name = unique_name();
        let shm_host = ShmAudioBuffer::create(&name, 2, 2, 128).unwrap();
        let shm_worker = ShmAudioBuffer::open(&name, 2, 2, 128).unwrap();

        // Worker writes output
        unsafe {
            let ch0 = shm_worker.output_channel_mut(0).unwrap();
            ch0[0] = 0.5;
            ch0[1] = -0.5;
            let ch1 = shm_worker.output_channel_mut(1).unwrap();
            ch1[0] = 0.25;
            ch1[1] = -0.25;
        }
        shm_worker.set_ready();

        // Host reads output
        assert!(shm_host.is_ready());
        unsafe {
            let ch0 = shm_host.output_channel(0).unwrap();
            assert!((ch0[0] - 0.5).abs() < f32::EPSILON);
            assert!((ch0[1] - (-0.5)).abs() < f32::EPSILON);
            let ch1 = shm_host.output_channel(1).unwrap();
            assert!((ch1[0] - 0.25).abs() < f32::EPSILON);
        }
        shm_host.clear_ready();
        assert!(!shm_worker.is_ready());

        drop(shm_worker);
        drop(shm_host);
    }

    #[test]
    fn test_invalid_channel_returns_none() {
        let name = unique_name();
        let shm = ShmAudioBuffer::create(&name, 1, 2, 64).unwrap();
        unsafe {
            assert!(shm.input_channel(0).is_some());
            assert!(shm.input_channel(1).is_none()); // only 1 input
            assert!(shm.output_channel(0).is_some());
            assert!(shm.output_channel(1).is_some());
            assert!(shm.output_channel(2).is_none()); // only 2 outputs
        }
        drop(shm);
    }

    #[test]
    fn test_ready_flag_protocol() {
        let name = unique_name();
        let shm = ShmAudioBuffer::create(&name, 1, 1, 32).unwrap();
        assert!(!shm.is_ready());
        shm.set_ready();
        assert!(shm.is_ready());
        shm.clear_ready();
        assert!(!shm.is_ready());
        drop(shm);
    }

    #[test]
    fn test_open_nonexistent_fails() {
        let result = ShmAudioBuffer::open("/rs-vst-host-nonexistent-99999", 1, 1, 64);
        assert!(result.is_err());
    }

    #[test]
    fn test_zero_input_channels() {
        let name = unique_name();
        let shm = ShmAudioBuffer::create(&name, 0, 2, 128).unwrap();
        assert_eq!(shm.input_channels(), 0);
        unsafe {
            assert!(shm.input_channel(0).is_none());
            assert!(shm.output_channel(0).is_some());
        }
        drop(shm);
    }

    #[test]
    fn test_large_block_size() {
        let name = unique_name();
        let shm = ShmAudioBuffer::create(&name, 2, 2, 8192).unwrap();
        let expected = ShmAudioBuffer::required_size(2, 2, 8192);
        assert_eq!(shm.size, expected);
        unsafe {
            let buf = shm.output_channel_mut(1).unwrap();
            buf[8191] = 42.0;
            let read = shm.output_channel(1).unwrap();
            assert!((read[8191] - 42.0).abs() < f32::EPSILON);
        }
        drop(shm);
    }
}
