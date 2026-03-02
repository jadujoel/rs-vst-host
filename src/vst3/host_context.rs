//! Minimal IHostApplication COM object for plugin initialization.
//!
//! Plugins receive this context during `IComponent::initialize()`.
//! We implement the bare minimum: host name query and stub `createInstance`.

use crate::vst3::com::{
    FUNKNOWN_IID, FUnknown, FUnknownVtbl, IHOST_APPLICATION_IID, IHostApplication,
    IHostApplicationVtbl, K_NOT_IMPLEMENTED, K_RESULT_OK, String128, TUID,
};
use crate::vst3::host_alloc;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

/// Host application name (UTF-16 encoded, null-terminated).
const HOST_NAME: &str = "rs-vst-host";

// ─── IHostApplication vtable (from vst3-rs) ──────────────────────────────

/// Static vtable instance — shared by all HostApplication instances.
static HOST_APP_VTBL: IHostApplicationVtbl = IHostApplicationVtbl {
    base: FUnknownVtbl {
        queryInterface: host_query_interface,
        addRef: host_add_ref,
        release: host_release,
    },
    getName: host_get_name,
    createInstance: host_create_instance,
};

// ─── HostApplication COM object ───────────────────────────────────────────

/// A COM-compatible host application object.
///
/// Layout: the first field is a pointer to the vtable, matching COM convention.
/// This object is reference-counted but we manage its lifetime manually
/// (it lives as long as the host session).
#[repr(C)]
pub struct HostApplication {
    vtbl: *const IHostApplicationVtbl,
    ref_count: AtomicU32,
}

impl HostApplication {
    /// Create a new host application instance on the **system** malloc heap.
    ///
    /// Returns a raw pointer suitable for passing to VST3 plugin `initialize()`.
    /// The caller is responsible for eventually calling `destroy()`.
    ///
    /// Uses the system allocator (bypassing mimalloc) so that if a plugin
    /// incorrectly calls `free()` / `operator delete` on this pointer
    /// (instead of COM `Release()`), the pointer is recognised by macOS
    /// system malloc and the process does not abort.
    pub fn new() -> *mut Self {
        unsafe {
            host_alloc::system_alloc(Self {
                vtbl: &HOST_APP_VTBL,
                ref_count: AtomicU32::new(1),
            })
        }
    }

    /// Destroy a host application instance previously created with `new()`.
    ///
    /// # Safety
    /// The pointer must have been returned by `HostApplication::new()` and
    /// must not be used after this call.
    pub unsafe fn destroy(ptr: *mut Self) {
        unsafe { host_alloc::system_free(ptr) };
    }

    /// Get a raw pointer suitable for passing as FUnknown* to plugins.
    pub fn as_unknown(ptr: *mut Self) -> *mut c_void {
        ptr as *mut c_void
    }
}

// ─── COM method implementations ───────────────────────────────────────────

unsafe extern "system" fn host_query_interface(
    this: *mut FUnknown,
    iid: *const TUID,
    obj: *mut *mut c_void,
) -> i32 {
    if iid.is_null() || obj.is_null() {
        return K_NOT_IMPLEMENTED;
    }

    let iid_bytes: [u8; 16] = unsafe { *(iid as *const [u8; 16]) };

    // We respond to FUnknown and IHostApplication queries
    if iid_bytes == FUNKNOWN_IID || iid_bytes == IHOST_APPLICATION_IID {
        // AddRef before returning
        unsafe { host_add_ref(this) };
        unsafe { *obj = this as *mut c_void };
        return K_RESULT_OK;
    }

    unsafe { *obj = std::ptr::null_mut() };
    K_NOT_IMPLEMENTED
}

unsafe extern "system" fn host_add_ref(this: *mut FUnknown) -> u32 {
    let host = this as *mut HostApplication;
    unsafe { (*host).ref_count.fetch_add(1, Ordering::Relaxed) + 1 }
}

unsafe extern "system" fn host_release(this: *mut FUnknown) -> u32 {
    let host = this as *mut HostApplication;
    // Note: We don't auto-destroy here because the host manages the lifetime.
    // The ref_count is maintained for protocol correctness.
    unsafe {
        let prev = (*host).ref_count.fetch_sub(1, Ordering::Relaxed);
        prev - 1
    }
}

unsafe extern "system" fn host_get_name(_this: *mut IHostApplication, name: *mut String128) -> i32 {
    if name.is_null() {
        return K_NOT_IMPLEMENTED;
    }

    let name_ptr = name as *mut u16;

    // Write host name as UTF-16, null-terminated, max 128 chars (String128)
    let utf16: Vec<u16> = HOST_NAME.encode_utf16().collect();
    let max_chars = 127; // Leave room for null terminator
    let copy_len = utf16.len().min(max_chars);

    unsafe {
        std::ptr::copy_nonoverlapping(utf16.as_ptr(), name_ptr, copy_len);
        // Null terminator
        *name_ptr.add(copy_len) = 0;
    }

    K_RESULT_OK
}

unsafe extern "system" fn host_create_instance(
    _this: *mut IHostApplication,
    _cid: *mut TUID,
    _iid: *mut TUID,
    obj: *mut *mut c_void,
) -> i32 {
    // We don't support creating instances from the host side.
    if !obj.is_null() {
        unsafe { *obj = std::ptr::null_mut() };
    }
    K_NOT_IMPLEMENTED
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_application_create_destroy() {
        let host = HostApplication::new();
        assert!(!host.is_null());
        unsafe {
            HostApplication::destroy(host);
        }
    }

    #[test]
    fn test_host_application_vtable_valid() {
        let host = HostApplication::new();
        unsafe {
            let obj = &*host;
            assert!(!obj.vtbl.is_null());
        }
        unsafe {
            HostApplication::destroy(host);
        }
    }

    #[test]
    fn test_host_get_name() {
        let host = HostApplication::new();
        let mut name_buf: String128 = [0u16; 128];
        unsafe {
            let result = host_get_name(
                host as *mut IHostApplication,
                &mut name_buf as *mut String128,
            );
            assert_eq!(result, K_RESULT_OK);

            // Find null terminator
            let len = name_buf.iter().position(|&c| c == 0).unwrap_or(128);
            let name = String::from_utf16_lossy(&name_buf[..len]);
            assert_eq!(name, "rs-vst-host");

            HostApplication::destroy(host);
        }
    }

    #[test]
    fn test_host_query_interface_funknown() {
        let host = HostApplication::new();
        unsafe {
            let mut obj: *mut c_void = std::ptr::null_mut();
            let result = host_query_interface(
                host as *mut FUnknown,
                FUNKNOWN_IID.as_ptr() as *const TUID,
                &mut obj,
            );
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(obj, host as *mut c_void);

            HostApplication::destroy(host);
        }
    }

    #[test]
    fn test_host_create_instance_returns_not_implemented() {
        let host = HostApplication::new();
        unsafe {
            let mut obj: *mut c_void = std::ptr::null_mut();
            let result = host_create_instance(
                host as *mut IHostApplication,
                [0u8; 16].as_ptr() as *mut TUID,
                [0u8; 16].as_ptr() as *mut TUID,
                &mut obj,
            );
            assert_eq!(result, K_NOT_IMPLEMENTED);
            assert!(obj.is_null());

            HostApplication::destroy(host);
        }
    }

    #[test]
    fn test_host_query_interface_ihost_application() {
        let host = HostApplication::new();
        unsafe {
            let mut obj: *mut c_void = std::ptr::null_mut();
            let result = host_query_interface(
                host as *mut FUnknown,
                IHOST_APPLICATION_IID.as_ptr() as *const TUID,
                &mut obj,
            );
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(obj, host as *mut c_void);

            HostApplication::destroy(host);
        }
    }

    #[test]
    fn test_host_query_interface_unknown_iid() {
        let host = HostApplication::new();
        unsafe {
            let mut obj: *mut c_void = std::ptr::null_mut();
            // Use a random IID that shouldn't be supported
            let random_iid: [u8; 16] = [0xFF; 16];
            let result = host_query_interface(
                host as *mut FUnknown,
                random_iid.as_ptr() as *const TUID,
                &mut obj,
            );
            assert_eq!(result, K_NOT_IMPLEMENTED);
            assert!(obj.is_null());

            HostApplication::destroy(host);
        }
    }

    #[test]
    fn test_host_add_ref_release_counting() {
        let host = HostApplication::new();
        unsafe {
            // Initial ref count is 1 (from new())
            let count = host_add_ref(host as *mut FUnknown);
            assert_eq!(count, 2);

            let count = host_add_ref(host as *mut FUnknown);
            assert_eq!(count, 3);

            let count = host_release(host as *mut FUnknown);
            assert_eq!(count, 2);

            let count = host_release(host as *mut FUnknown);
            assert_eq!(count, 1);

            HostApplication::destroy(host);
        }
    }

    #[test]
    fn test_host_get_name_null_ptr() {
        let result = unsafe { host_get_name(std::ptr::null_mut(), std::ptr::null_mut()) };
        assert_eq!(result, K_NOT_IMPLEMENTED);
    }

    #[test]
    fn test_host_as_unknown() {
        let host = HostApplication::new();
        let unknown = HostApplication::as_unknown(host);
        assert!(!unknown.is_null());
        assert_eq!(unknown, host as *mut c_void);
        unsafe { HostApplication::destroy(host) };
    }

    #[test]
    fn test_host_destroy_null() {
        // Should not panic with null pointer
        unsafe { HostApplication::destroy(std::ptr::null_mut()) };
    }

    #[test]
    fn test_host_query_interface_null_params() {
        let host = HostApplication::new();
        unsafe {
            // Null iid
            let mut obj: *mut c_void = std::ptr::null_mut();
            let result = host_query_interface(host as *mut FUnknown, std::ptr::null(), &mut obj);
            assert_eq!(result, K_NOT_IMPLEMENTED);

            // Null obj
            let result = host_query_interface(
                host as *mut FUnknown,
                FUNKNOWN_IID.as_ptr() as *const TUID,
                std::ptr::null_mut(),
            );
            assert_eq!(result, K_NOT_IMPLEMENTED);

            HostApplication::destroy(host);
        }
    }

    /// Verify HostApplication is allocated on the system malloc heap.
    /// On macOS with mimalloc as global allocator, this confirms the
    /// host_alloc::system_alloc path is being used.
    #[test]
    fn test_host_application_on_system_heap() {
        let host = HostApplication::new();
        assert!(host_alloc::is_system_malloc_ptr(host));
        unsafe { HostApplication::destroy(host) };
    }
}
