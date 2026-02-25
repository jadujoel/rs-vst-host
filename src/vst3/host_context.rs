//! Minimal IHostApplication COM object for plugin initialization.
//!
//! Plugins receive this context during `IComponent::initialize()`.
//! We implement the bare minimum: host name query and stub `createInstance`.

use crate::vst3::com::{FUNKNOWN_IID, IHOST_APPLICATION_IID, K_NOT_IMPLEMENTED, K_RESULT_OK};
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

/// Host application name (UTF-16 encoded, null-terminated).
const HOST_NAME: &str = "rs-vst-host";

// ─── IHostApplication vtable ──────────────────────────────────────────────

/// IHostApplication vtable layout:
///   [0-2] FUnknown: queryInterface, addRef, release
///   [3]   getName(name: *mut u16) -> tresult
///   [4]   createInstance(cid, iid, obj) -> tresult
#[repr(C)]
struct IHostApplicationVtbl {
    // FUnknown
    query_interface:
        unsafe extern "system" fn(this: *mut c_void, iid: *const u8, obj: *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(this: *mut c_void) -> u32,
    release: unsafe extern "system" fn(this: *mut c_void) -> u32,
    // IHostApplication
    get_name: unsafe extern "system" fn(this: *mut c_void, name: *mut u16) -> i32,
    create_instance: unsafe extern "system" fn(
        this: *mut c_void,
        cid: *const u8,
        iid: *const u8,
        obj: *mut *mut c_void,
    ) -> i32,
}

/// Static vtable instance — shared by all HostApplication instances.
static HOST_APP_VTBL: IHostApplicationVtbl = IHostApplicationVtbl {
    query_interface: host_query_interface,
    add_ref: host_add_ref,
    release: host_release,
    get_name: host_get_name,
    create_instance: host_create_instance,
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
    /// Create a new host application instance on the heap.
    ///
    /// Returns a raw pointer suitable for passing to VST3 plugin `initialize()`.
    /// The caller is responsible for eventually calling `destroy()`.
    pub fn new() -> *mut Self {
        let obj = Box::new(Self {
            vtbl: &HOST_APP_VTBL,
            ref_count: AtomicU32::new(1),
        });
        Box::into_raw(obj)
    }

    /// Destroy a host application instance previously created with `new()`.
    ///
    /// # Safety
    /// The pointer must have been returned by `HostApplication::new()` and
    /// must not be used after this call.
    pub unsafe fn destroy(ptr: *mut Self) {
        if !ptr.is_null() {
            unsafe { drop(Box::from_raw(ptr)) };
        }
    }

    /// Get a raw pointer suitable for passing as FUnknown* to plugins.
    pub fn as_unknown(ptr: *mut Self) -> *mut c_void {
        ptr as *mut c_void
    }
}

// ─── COM method implementations ───────────────────────────────────────────

unsafe extern "system" fn host_query_interface(
    this: *mut c_void,
    iid: *const u8,
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
        unsafe { *obj = this };
        return K_RESULT_OK;
    }

    unsafe { *obj = std::ptr::null_mut() };
    K_NOT_IMPLEMENTED
}

unsafe extern "system" fn host_add_ref(this: *mut c_void) -> u32 {
    let host = this as *mut HostApplication;
    unsafe { (*host).ref_count.fetch_add(1, Ordering::Relaxed) + 1 }
}

unsafe extern "system" fn host_release(this: *mut c_void) -> u32 {
    let host = this as *mut HostApplication;
    // Note: We don't auto-destroy here because the host manages the lifetime.
    // The ref_count is maintained for protocol correctness.
    unsafe {
        let prev = (*host).ref_count.fetch_sub(1, Ordering::Relaxed);
        prev - 1
    }
}

unsafe extern "system" fn host_get_name(_this: *mut c_void, name: *mut u16) -> i32 {
    if name.is_null() {
        return K_NOT_IMPLEMENTED;
    }

    // Write host name as UTF-16, null-terminated, max 128 chars (String128)
    let utf16: Vec<u16> = HOST_NAME.encode_utf16().collect();
    let max_chars = 127; // Leave room for null terminator
    let copy_len = utf16.len().min(max_chars);

    unsafe {
        std::ptr::copy_nonoverlapping(utf16.as_ptr(), name, copy_len);
        // Null terminator
        *name.add(copy_len) = 0;
    }

    K_RESULT_OK
}

unsafe extern "system" fn host_create_instance(
    _this: *mut c_void,
    _cid: *const u8,
    _iid: *const u8,
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
        let mut name_buf = [0u16; 128];
        unsafe {
            let result = host_get_name(host as *mut c_void, name_buf.as_mut_ptr());
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
                host as *mut c_void,
                FUNKNOWN_IID.as_ptr(),
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
                host as *mut c_void,
                [0u8; 16].as_ptr(),
                [0u8; 16].as_ptr(),
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
                host as *mut c_void,
                IHOST_APPLICATION_IID.as_ptr(),
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
                host as *mut c_void,
                random_iid.as_ptr(),
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
            let count = host_add_ref(host as *mut c_void);
            assert_eq!(count, 2);

            let count = host_add_ref(host as *mut c_void);
            assert_eq!(count, 3);

            let count = host_release(host as *mut c_void);
            assert_eq!(count, 2);

            let count = host_release(host as *mut c_void);
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
            let result = host_query_interface(
                host as *mut c_void,
                std::ptr::null(),
                &mut obj,
            );
            assert_eq!(result, K_NOT_IMPLEMENTED);

            // Null obj
            let result = host_query_interface(
                host as *mut c_void,
                FUNKNOWN_IID.as_ptr(),
                std::ptr::null_mut(),
            );
            assert_eq!(result, K_NOT_IMPLEMENTED);

            HostApplication::destroy(host);
        }
    }
}
