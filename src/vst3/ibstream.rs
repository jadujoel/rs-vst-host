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
use crate::vst3::com::{
    FUnknown, FUnknownVtbl, IBStream, IBStreamVtbl, TUID,
    FUNKNOWN_IID, IBSTREAM_IID, K_RESULT_OK, K_RESULT_FALSE, K_INVALID_ARGUMENT,
};

/// Seek origin constants (matching VST3 SDK).
const K_IB_SEEK_SET: i32 = 0; // From beginning
const K_IB_SEEK_CUR: i32 = 1; // From current position
const K_IB_SEEK_END: i32 = 2; // From end

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
    base: FUnknownVtbl {
        queryInterface: host_bstream_query_interface,
        addRef: host_bstream_add_ref,
        release: host_bstream_release,
    },
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
    this: *mut FUnknown,
    iid: *const TUID,
    obj: *mut *mut c_void,
) -> i32 {
    if obj.is_null() || iid.is_null() {
        return K_INVALID_ARGUMENT;
    }

    let iid_slice = unsafe { std::slice::from_raw_parts(iid as *const u8, 16) };

    if iid_slice == IBSTREAM_IID || iid_slice == FUNKNOWN_IID {
        unsafe {
            *obj = this as *mut c_void;
            host_bstream_add_ref(this);
        }
        return K_RESULT_OK;
    }

    unsafe { *obj = std::ptr::null_mut() };
    K_RESULT_FALSE
}

unsafe extern "system" fn host_bstream_add_ref(this: *mut FUnknown) -> u32 {
    let stream = this as *mut HostBStream;
    unsafe {
        (*stream)
            .ref_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1
    }
}

unsafe extern "system" fn host_bstream_release(this: *mut FUnknown) -> u32 {
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
    this: *mut IBStream,
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
    this: *mut IBStream,
    buffer: *mut c_void,
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
    this: *mut IBStream,
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

unsafe extern "system" fn host_bstream_tell(this: *mut IBStream, pos: *mut i64) -> i32 {
    if pos.is_null() {
        return K_INVALID_ARGUMENT;
    }
    let stream = unsafe { &*(this as *mut HostBStream) };
    unsafe { *pos = stream.cursor as i64 };
    K_RESULT_OK
}

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
                stream as *mut IBStream,
                data.as_ptr() as *mut c_void,
                data.len() as i32,
                &mut written,
            );
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(written, data.len() as i32);

            // Seek back to start
            let mut new_pos = 0i64;
            let result = host_bstream_seek(stream as *mut IBStream, 0, K_IB_SEEK_SET, &mut new_pos);
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(new_pos, 0);

            // Read back
            let mut buf = [0u8; 32];
            let mut bytes_read = 0i32;
            let result = host_bstream_read(
                stream as *mut IBStream,
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
                stream as *mut IBStream,
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
            let result = host_bstream_seek(stream as *mut IBStream, 50, K_IB_SEEK_SET, &mut pos);
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(pos, 50);

            // Tell should return 50
            let mut tell_pos = 0i64;
            let result = host_bstream_tell(stream as *mut IBStream, &mut tell_pos);
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(tell_pos, 50);

            // Seek relative +10
            let result = host_bstream_seek(stream as *mut IBStream, 10, K_IB_SEEK_CUR, &mut pos);
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(pos, 60);

            // Seek from end -20
            let result = host_bstream_seek(stream as *mut IBStream, -20, K_IB_SEEK_END, &mut pos);
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(pos, 80);

            // Seek to negative position should fail
            let result = host_bstream_seek(stream as *mut IBStream, -200, K_IB_SEEK_SET, &mut pos);
            assert_eq!(result, K_INVALID_ARGUMENT);

            HostBStream::destroy(stream);
        }
    }

    #[test]
    fn test_bstream_ref_counting() {
        let stream = HostBStream::new();
        unsafe {
            // Initial ref count is 1
            let count = host_bstream_add_ref(stream as *mut FUnknown);
            assert_eq!(count, 2);

            let count = host_bstream_release(stream as *mut FUnknown);
            assert_eq!(count, 1);

            // Final release destroys
            let count = host_bstream_release(stream as *mut FUnknown);
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
                stream as *mut IBStream,
                data.as_ptr() as *mut c_void,
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
            {
                let result =
                    host_bstream_query_interface(stream as *mut FUnknown, IBSTREAM_IID.as_ptr() as *const TUID, &mut obj);
                assert_eq!(result, K_RESULT_OK);
                assert_eq!(obj, stream as *mut c_void);
                // Release the extra ref from QI
                host_bstream_release(stream as *mut FUnknown);
            }

            // Query for FUnknown should succeed
            {
                let result =
                    host_bstream_query_interface(stream as *mut FUnknown, FUNKNOWN_IID.as_ptr() as *const TUID, &mut obj);
                assert_eq!(result, K_RESULT_OK);
                host_bstream_release(stream as *mut FUnknown);
            }

            HostBStream::destroy(stream);
        }
    }
}
