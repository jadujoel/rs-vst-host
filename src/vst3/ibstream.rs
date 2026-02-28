//! Minimal IBStream COM implementation for VST3 state transfer.
//!
//! IBStream is the VST3 SDK's binary stream interface used by:
//! - `IComponent::getState()` / `IComponent::setState()` — component state
//! - `IEditController::setComponentState()` — transfer component state to controller
//! - `IEditController::getState()` / `IEditController::setState()` — controller state
//!
//! This implementation backs the stream with a `Vec<u8>` buffer, supporting
//! read, write, seek, and tell operations. It is allocated on the system
//! malloc heap (via `host_alloc`) so plugins can safely interact with it.

use std::ffi::c_void;
use std::sync::atomic::AtomicU32;

use super::host_alloc;
use super::module::K_RESULT_OK;

/// IBStream IID: {C3BF6EA2-3099-4752-9B6B-F9901EE33E9B}
#[cfg(target_os = "macos")]
const IBSTREAM_IID: [u8; 16] = [
    0xC3, 0xBF, 0x6E, 0xA2, 0x30, 0x99, 0x47, 0x52, 0x9B, 0x6B, 0xF9, 0x90, 0x1E, 0xE3, 0x3E,
    0x9B,
];

/// FUnknown IID: {00000000-0000-0000-C000-000000000046}
#[cfg(target_os = "macos")]
const FUNKNOWN_IID: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x46,
];

const K_RESULT_FALSE: i32 = 1;
const K_INVALID_ARGUMENT: i32 = 4;

/// Seek origin constants (matching VST3 SDK).
const K_IB_SEEK_SET: i32 = 0; // From beginning
const K_IB_SEEK_CUR: i32 = 1; // From current position
const K_IB_SEEK_END: i32 = 2; // From end

/// IBStream vtable layout.
///
/// ```text
/// [0] queryInterface
/// [1] addRef
/// [2] release
/// [3] read
/// [4] write
/// [5] seek
/// [6] tell
/// ```
#[repr(C)]
struct IBStreamVtbl {
    query_interface:
        unsafe extern "system" fn(this: *mut c_void, iid: *const u8, obj: *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(this: *mut c_void) -> u32,
    release: unsafe extern "system" fn(this: *mut c_void) -> u32,
    read: unsafe extern "system" fn(
        this: *mut c_void,
        buffer: *mut c_void,
        num_bytes: i32,
        num_bytes_read: *mut i32,
    ) -> i32,
    write: unsafe extern "system" fn(
        this: *mut c_void,
        buffer: *const c_void,
        num_bytes: i32,
        num_bytes_written: *mut i32,
    ) -> i32,
    seek: unsafe extern "system" fn(this: *mut c_void, pos: i64, mode: i32, result: *mut i64) -> i32,
    tell: unsafe extern "system" fn(this: *mut c_void, pos: *mut i64) -> i32,
}

/// Host-side IBStream COM object backed by a `Vec<u8>`.
#[repr(C)]
pub struct HostBStream {
    vtbl: *const IBStreamVtbl,
    ref_count: AtomicU32,
    data: Vec<u8>,
    cursor: usize,
}

/// Static vtable for HostBStream.
static HOST_BSTREAM_VTBL: IBStreamVtbl = IBStreamVtbl {
    query_interface: host_bstream_query_interface,
    add_ref: host_bstream_add_ref,
    release: host_bstream_release,
    read: host_bstream_read,
    write: host_bstream_write,
    seek: host_bstream_seek,
    tell: host_bstream_tell,
};

impl HostBStream {
    /// Create a new empty IBStream (for writing into, e.g., via `getState`).
    pub fn new() -> *mut Self {
        unsafe {
            host_alloc::system_alloc(Self {
                vtbl: &HOST_BSTREAM_VTBL,
                ref_count: AtomicU32::new(1),
                data: Vec::new(),
                cursor: 0,
            })
        }
    }

    /// Create a new IBStream pre-filled with data (for reading, e.g., via `setState`).
    pub fn from_data(data: Vec<u8>) -> *mut Self {
        unsafe {
            host_alloc::system_alloc(Self {
                vtbl: &HOST_BSTREAM_VTBL,
                ref_count: AtomicU32::new(1),
                data,
                cursor: 0,
            })
        }
    }

    /// Get the COM pointer for passing to plugin methods.
    pub fn as_ptr(stream: *mut Self) -> *mut c_void {
        stream as *mut c_void
    }

    /// Extract the written data from the stream.
    ///
    /// # Safety
    ///
    /// `stream` must be a valid pointer returned by `HostBStream::new()`.
    pub unsafe fn take_data(stream: *mut Self) -> Vec<u8> {
        unsafe { std::mem::take(&mut (*stream).data) }
    }

    /// Destroy a previously created HostBStream.
    ///
    /// # Safety
    ///
    /// `stream` must be a valid pointer returned by `HostBStream::new()` or
    /// `HostBStream::from_data()` and must not be used after this call.
    pub unsafe fn destroy(stream: *mut Self) {
        // system_free calls drop_in_place (which drops the Vec<u8>) then libc::free
        unsafe { host_alloc::system_free(stream) };
    }
}

// ── COM vtable functions ────────────────────────────────────────────────────

unsafe extern "system" fn host_bstream_query_interface(
    this: *mut c_void,
    iid: *const u8,
    obj: *mut *mut c_void,
) -> i32 {
    if obj.is_null() || iid.is_null() {
        return K_INVALID_ARGUMENT;
    }

    let iid_slice = unsafe { std::slice::from_raw_parts(iid, 16) };

    #[cfg(target_os = "macos")]
    if iid_slice == IBSTREAM_IID || iid_slice == FUNKNOWN_IID {
        unsafe {
            *obj = this;
            host_bstream_add_ref(this);
        }
        return K_RESULT_OK;
    }

    unsafe { *obj = std::ptr::null_mut() };
    K_RESULT_FALSE
}

unsafe extern "system" fn host_bstream_add_ref(this: *mut c_void) -> u32 {
    let stream = this as *mut HostBStream;
    unsafe {
        (*stream)
            .ref_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1
    }
}

unsafe extern "system" fn host_bstream_release(this: *mut c_void) -> u32 {
    let stream = this as *mut HostBStream;
    let prev = unsafe {
        (*stream)
            .ref_count
            .fetch_sub(1, std::sync::atomic::Ordering::AcqRel)
    };
    if prev == 1 {
        unsafe { HostBStream::destroy(stream) };
        0
    } else {
        prev - 1
    }
}

unsafe extern "system" fn host_bstream_read(
    this: *mut c_void,
    buffer: *mut c_void,
    num_bytes: i32,
    num_bytes_read: *mut i32,
) -> i32 {
    if buffer.is_null() || num_bytes < 0 {
        return K_INVALID_ARGUMENT;
    }
    let stream = unsafe { &mut *(this as *mut HostBStream) };
    let available = stream.data.len().saturating_sub(stream.cursor);
    let to_read = (num_bytes as usize).min(available);

    if to_read > 0 {
        unsafe {
            std::ptr::copy_nonoverlapping(
                stream.data.as_ptr().add(stream.cursor),
                buffer as *mut u8,
                to_read,
            );
        }
        stream.cursor += to_read;
    }

    if !num_bytes_read.is_null() {
        unsafe { *num_bytes_read = to_read as i32 };
    }

    K_RESULT_OK
}

unsafe extern "system" fn host_bstream_write(
    this: *mut c_void,
    buffer: *const c_void,
    num_bytes: i32,
    num_bytes_written: *mut i32,
) -> i32 {
    if buffer.is_null() || num_bytes < 0 {
        return K_INVALID_ARGUMENT;
    }
    let stream = unsafe { &mut *(this as *mut HostBStream) };
    let bytes = unsafe { std::slice::from_raw_parts(buffer as *const u8, num_bytes as usize) };

    let cursor = stream.cursor;
    let end = cursor + bytes.len();

    if end > stream.data.len() {
        stream.data.resize(end, 0);
    }
    stream.data[cursor..end].copy_from_slice(bytes);
    stream.cursor = end;

    if !num_bytes_written.is_null() {
        unsafe { *num_bytes_written = num_bytes };
    }

    K_RESULT_OK
}

unsafe extern "system" fn host_bstream_seek(
    this: *mut c_void,
    pos: i64,
    mode: i32,
    result: *mut i64,
) -> i32 {
    let stream = unsafe { &mut *(this as *mut HostBStream) };
    let new_pos: i64 = match mode {
        K_IB_SEEK_SET => pos,
        K_IB_SEEK_CUR => stream.cursor as i64 + pos,
        K_IB_SEEK_END => stream.data.len() as i64 + pos,
        _ => return K_INVALID_ARGUMENT,
    };

    if new_pos < 0 {
        return K_INVALID_ARGUMENT;
    }

    stream.cursor = new_pos as usize;

    if !result.is_null() {
        unsafe { *result = new_pos };
    }

    K_RESULT_OK
}

unsafe extern "system" fn host_bstream_tell(this: *mut c_void, pos: *mut i64) -> i32 {
    if pos.is_null() {
        return K_INVALID_ARGUMENT;
    }
    let stream = unsafe { &*(this as *mut HostBStream) };
    unsafe { *pos = stream.cursor as i64 };
    K_RESULT_OK
}

// ── Suppress unused constant warnings on non-macOS ──────────────────────────
#[cfg(not(target_os = "macos"))]
const _: () = {
    let _ = K_RESULT_FALSE;
    let _ = K_INVALID_ARGUMENT;
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bstream_write_and_read() {
        let stream = HostBStream::new();
        let data = b"Hello, IBStream!";
        unsafe {
            // Write data
            let mut written = 0i32;
            let result = host_bstream_write(
                stream as *mut c_void,
                data.as_ptr() as *const c_void,
                data.len() as i32,
                &mut written,
            );
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(written, data.len() as i32);

            // Seek back to start
            let mut new_pos = 0i64;
            let result = host_bstream_seek(stream as *mut c_void, 0, K_IB_SEEK_SET, &mut new_pos);
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(new_pos, 0);

            // Read back
            let mut buf = [0u8; 32];
            let mut bytes_read = 0i32;
            let result = host_bstream_read(
                stream as *mut c_void,
                buf.as_mut_ptr() as *mut c_void,
                32,
                &mut bytes_read,
            );
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(bytes_read, data.len() as i32);
            assert_eq!(&buf[..data.len()], data);

            HostBStream::destroy(stream);
        }
    }

    #[test]
    fn test_bstream_from_data() {
        let original = vec![1u8, 2, 3, 4, 5];
        let stream = HostBStream::from_data(original.clone());
        unsafe {
            let mut buf = [0u8; 5];
            let mut bytes_read = 0i32;
            let result = host_bstream_read(
                stream as *mut c_void,
                buf.as_mut_ptr() as *mut c_void,
                5,
                &mut bytes_read,
            );
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(bytes_read, 5);
            assert_eq!(&buf, original.as_slice());

            HostBStream::destroy(stream);
        }
    }

    #[test]
    fn test_bstream_seek_and_tell() {
        let stream = HostBStream::from_data(vec![0u8; 100]);
        unsafe {
            // Seek to position 50
            let mut pos = 0i64;
            let result = host_bstream_seek(stream as *mut c_void, 50, K_IB_SEEK_SET, &mut pos);
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(pos, 50);

            // Tell should return 50
            let mut tell_pos = 0i64;
            let result = host_bstream_tell(stream as *mut c_void, &mut tell_pos);
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(tell_pos, 50);

            // Seek relative +10
            let result = host_bstream_seek(stream as *mut c_void, 10, K_IB_SEEK_CUR, &mut pos);
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(pos, 60);

            // Seek from end -20
            let result = host_bstream_seek(stream as *mut c_void, -20, K_IB_SEEK_END, &mut pos);
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(pos, 80);

            // Seek to negative position should fail
            let result = host_bstream_seek(stream as *mut c_void, -200, K_IB_SEEK_SET, &mut pos);
            assert_eq!(result, K_INVALID_ARGUMENT);

            HostBStream::destroy(stream);
        }
    }

    #[test]
    fn test_bstream_ref_counting() {
        let stream = HostBStream::new();
        unsafe {
            // Initial ref count is 1
            let count = host_bstream_add_ref(stream as *mut c_void);
            assert_eq!(count, 2);

            let count = host_bstream_release(stream as *mut c_void);
            assert_eq!(count, 1);

            // Final release destroys
            let count = host_bstream_release(stream as *mut c_void);
            assert_eq!(count, 0);
            // stream is now freed — don't use it
        }
    }

    #[test]
    fn test_bstream_take_data() {
        let stream = HostBStream::new();
        unsafe {
            let data = b"test data";
            let mut written = 0i32;
            host_bstream_write(
                stream as *mut c_void,
                data.as_ptr() as *const c_void,
                data.len() as i32,
                &mut written,
            );

            let result = HostBStream::take_data(stream);
            assert_eq!(result, data);

            HostBStream::destroy(stream);
        }
    }

    #[test]
    fn test_bstream_query_interface() {
        let stream = HostBStream::new();
        unsafe {
            let mut obj: *mut c_void = std::ptr::null_mut();

            // Query for IBStream IID should succeed
            #[cfg(target_os = "macos")]
            {
                let result =
                    host_bstream_query_interface(stream as *mut c_void, IBSTREAM_IID.as_ptr(), &mut obj);
                assert_eq!(result, K_RESULT_OK);
                assert_eq!(obj, stream as *mut c_void);
                // Release the extra ref from QI
                host_bstream_release(stream as *mut c_void);
            }

            // Query for FUnknown should succeed
            #[cfg(target_os = "macos")]
            {
                let result =
                    host_bstream_query_interface(stream as *mut c_void, FUNKNOWN_IID.as_ptr(), &mut obj);
                assert_eq!(result, K_RESULT_OK);
                host_bstream_release(stream as *mut c_void);
            }

            HostBStream::destroy(stream);
        }
    }
}
