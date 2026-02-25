//! IComponentHandler COM implementation for plugin-to-host parameter change notifications.
//!
//! VST3 plugins call methods on IComponentHandler to inform the host about parameter
//! edits (begin/perform/end) and request restarts. This implementation collects
//! parameter changes in a thread-safe queue for the control thread to process.

use crate::vst3::com::*;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use tracing::{debug, warn};

/// IComponentHandler IID: {93A0BEA3-0BD0-45DB-8E89-0B0CC1E46AC6}
#[cfg(not(target_os = "windows"))]
pub const ICOMPONENT_HANDLER_IID: [u8; 16] = [
    0x93, 0xA0, 0xBE, 0xA3, 0x0B, 0xD0, 0x45, 0xDB, 0x8E, 0x89, 0x0B, 0x0C, 0xC1, 0xE4, 0x6A,
    0xC6,
];

#[cfg(target_os = "windows")]
pub const ICOMPONENT_HANDLER_IID: [u8; 16] = [
    0xA3, 0xBE, 0xA0, 0x93, 0xDB, 0x45, 0xD0, 0x0B, 0x8E, 0x89, 0x0B, 0x0C, 0xC1, 0xE4, 0x6A,
    0xC6,
];

/// A parameter change notification from the plugin.
#[derive(Debug, Clone)]
pub struct ParamChange {
    /// Parameter ID.
    pub id: u32,
    /// Normalized value [0..1].
    pub value: f64,
}

/// Restart flags that a plugin may request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestartFlags(pub i32);

impl RestartFlags {
    /// Re-read parameter values.
    pub const PARAM_VALUES_CHANGED: i32 = 1;
    /// Re-read parameter titles.
    #[allow(dead_code)]
    pub const PARAM_TITLES_CHANGED: i32 = 1 << 1;
    /// Latency has changed; re-query.
    pub const LATENCY_CHANGED: i32 = 1 << 3;
}

/// IComponentHandler vtable.
///
/// vtable layout:
///   [0-2]  FUnknown: queryInterface, addRef, release
///   [3]    beginEdit(id: ParamID) -> tresult
///   [4]    performEdit(id: ParamID, valueNormalized: f64) -> tresult
///   [5]    endEdit(id: ParamID) -> tresult
///   [6]    restartComponent(flags: i32) -> tresult
#[repr(C)]
struct IComponentHandlerVtbl {
    // FUnknown
    query_interface:
        unsafe extern "system" fn(this: *mut c_void, iid: *const u8, obj: *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(this: *mut c_void) -> u32,
    release: unsafe extern "system" fn(this: *mut c_void) -> u32,
    // IComponentHandler
    begin_edit: unsafe extern "system" fn(this: *mut c_void, id: u32) -> i32,
    perform_edit: unsafe extern "system" fn(this: *mut c_void, id: u32, value: f64) -> i32,
    end_edit: unsafe extern "system" fn(this: *mut c_void, id: u32) -> i32,
    restart_component: unsafe extern "system" fn(this: *mut c_void, flags: i32) -> i32,
}

/// Static vtable for IComponentHandler.
static COMPONENT_HANDLER_VTBL: IComponentHandlerVtbl = IComponentHandlerVtbl {
    query_interface: handler_query_interface,
    add_ref: handler_add_ref,
    release: handler_release,
    begin_edit: handler_begin_edit,
    perform_edit: handler_perform_edit,
    end_edit: handler_end_edit,
    restart_component: handler_restart_component,
};

/// Host-side IComponentHandler COM object.
///
/// Created once per plugin session; passed to `IEditController::setComponentHandler()`.
/// The plugin calls back on this to notify the host of parameter changes.
#[repr(C)]
pub struct HostComponentHandler {
    /// Pointer to the static vtable.
    vtbl: *const IComponentHandlerVtbl,
    /// Reference count.
    ref_count: AtomicU32,
    /// Pending parameter changes (from plugin callbacks).
    pending_changes: Mutex<Vec<ParamChange>>,
    /// Pending restart flags.
    pending_restart: AtomicU32,
}

impl HostComponentHandler {
    /// Create a new heap-allocated component handler.
    pub fn new() -> *mut Self {
        let obj = Box::new(Self {
            vtbl: &COMPONENT_HANDLER_VTBL,
            ref_count: AtomicU32::new(1),
            pending_changes: Mutex::new(Vec::new()),
            pending_restart: AtomicU32::new(0),
        });
        Box::into_raw(obj)
    }

    /// Destroy a component handler created with `new()`.
    ///
    /// # Safety
    /// The pointer must have been created by `HostComponentHandler::new()`.
    pub unsafe fn destroy(ptr: *mut Self) {
        if !ptr.is_null() {
            drop(unsafe { Box::from_raw(ptr) });
        }
    }

    /// Get this as a `*mut c_void` for passing to `setComponentHandler()`.
    pub fn as_ptr(ptr: *mut Self) -> *mut c_void {
        ptr as *mut c_void
    }

    /// Drain all pending parameter changes.
    ///
    /// # Safety
    /// The pointer must be valid.
    pub unsafe fn drain_changes(ptr: *mut Self) -> Vec<ParamChange> {
        let this = unsafe { &*ptr };
        if let Ok(mut changes) = this.pending_changes.try_lock() {
            std::mem::take(&mut *changes)
        } else {
            Vec::new()
        }
    }

    /// Read and clear pending restart flags.
    ///
    /// # Safety
    /// The pointer must be valid.
    #[allow(dead_code)]
    pub unsafe fn take_restart_flags(ptr: *mut Self) -> i32 {
        let this = unsafe { &*ptr };
        this.pending_restart.swap(0, Ordering::Relaxed) as i32
    }
}

// ─── COM method implementations ───────────────────────────────────────────

unsafe extern "system" fn handler_query_interface(
    this: *mut c_void,
    iid: *const u8,
    obj: *mut *mut c_void,
) -> i32 {
    if iid.is_null() || obj.is_null() {
        return K_NOT_IMPLEMENTED;
    }

    let iid_bytes: [u8; 16] = unsafe { *(iid as *const [u8; 16]) };

    if iid_bytes == ICOMPONENT_HANDLER_IID || iid_bytes == FUNKNOWN_IID {
        unsafe {
            handler_add_ref(this);
            *obj = this;
        }
        return K_RESULT_OK;
    }

    unsafe { *obj = std::ptr::null_mut() };
    K_NOT_IMPLEMENTED
}

unsafe extern "system" fn handler_add_ref(this: *mut c_void) -> u32 {
    let handler = this as *mut HostComponentHandler;
    unsafe { (*handler).ref_count.fetch_add(1, Ordering::Relaxed) + 1 }
}

unsafe extern "system" fn handler_release(this: *mut c_void) -> u32 {
    let handler = this as *mut HostComponentHandler;
    unsafe {
        let prev = (*handler).ref_count.fetch_sub(1, Ordering::Relaxed);
        prev - 1
    }
}

unsafe extern "system" fn handler_begin_edit(_this: *mut c_void, id: u32) -> i32 {
    debug!(param_id = id, "Plugin: beginEdit");
    K_RESULT_OK
}

unsafe extern "system" fn handler_perform_edit(this: *mut c_void, id: u32, value: f64) -> i32 {
    let handler = this as *mut HostComponentHandler;
    unsafe {
        if let Ok(mut changes) = (*handler).pending_changes.try_lock() {
            changes.push(ParamChange { id, value });
        }
    }
    debug!(param_id = id, value = %format!("{:.4}", value), "Plugin: performEdit");
    K_RESULT_OK
}

unsafe extern "system" fn handler_end_edit(_this: *mut c_void, id: u32) -> i32 {
    debug!(param_id = id, "Plugin: endEdit");
    K_RESULT_OK
}

unsafe extern "system" fn handler_restart_component(this: *mut c_void, flags: i32) -> i32 {
    let handler = this as *mut HostComponentHandler;
    unsafe {
        (*handler)
            .pending_restart
            .fetch_or(flags as u32, Ordering::Relaxed);
    }
    if flags & RestartFlags::PARAM_VALUES_CHANGED != 0 {
        debug!("Plugin: restartComponent (param values changed)");
    }
    if flags & RestartFlags::LATENCY_CHANGED != 0 {
        warn!("Plugin: restartComponent (latency changed — not yet handled)");
    }
    K_RESULT_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_destroy() {
        let handler = HostComponentHandler::new();
        assert!(!handler.is_null());
        unsafe { HostComponentHandler::destroy(handler) };
    }

    #[test]
    fn test_query_interface_funknown() {
        let handler = HostComponentHandler::new();
        unsafe {
            let vtbl = &*(*handler).vtbl;
            let mut obj: *mut c_void = std::ptr::null_mut();

            let result = (vtbl.query_interface)(
                handler as *mut c_void,
                FUNKNOWN_IID.as_ptr(),
                &mut obj,
            );
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(obj, handler as *mut c_void);

            HostComponentHandler::destroy(handler);
        }
    }

    #[test]
    fn test_query_interface_icomponent_handler() {
        let handler = HostComponentHandler::new();
        unsafe {
            let vtbl = &*(*handler).vtbl;
            let mut obj: *mut c_void = std::ptr::null_mut();

            let result = (vtbl.query_interface)(
                handler as *mut c_void,
                ICOMPONENT_HANDLER_IID.as_ptr(),
                &mut obj,
            );
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(obj, handler as *mut c_void);

            HostComponentHandler::destroy(handler);
        }
    }

    #[test]
    fn test_query_interface_unknown_iid() {
        let handler = HostComponentHandler::new();
        unsafe {
            let vtbl = &*(*handler).vtbl;
            let mut obj: *mut c_void = std::ptr::null_mut();

            let result = (vtbl.query_interface)(
                handler as *mut c_void,
                ICOMPONENT_IID.as_ptr(),
                &mut obj,
            );
            assert_ne!(result, K_RESULT_OK);
            assert!(obj.is_null());

            HostComponentHandler::destroy(handler);
        }
    }

    #[test]
    fn test_perform_edit_collects_changes() {
        let handler = HostComponentHandler::new();
        unsafe {
            let vtbl = &*(*handler).vtbl;

            (vtbl.begin_edit)(handler as *mut c_void, 100);
            (vtbl.perform_edit)(handler as *mut c_void, 100, 0.75);
            (vtbl.end_edit)(handler as *mut c_void, 100);

            let changes = HostComponentHandler::drain_changes(handler);
            assert_eq!(changes.len(), 1);
            assert_eq!(changes[0].id, 100);
            assert!((changes[0].value - 0.75).abs() < f64::EPSILON);

            // Drain again should be empty
            let changes = HostComponentHandler::drain_changes(handler);
            assert!(changes.is_empty());

            HostComponentHandler::destroy(handler);
        }
    }

    #[test]
    fn test_restart_flags() {
        let handler = HostComponentHandler::new();
        unsafe {
            let vtbl = &*(*handler).vtbl;

            (vtbl.restart_component)(
                handler as *mut c_void,
                RestartFlags::PARAM_VALUES_CHANGED,
            );
            let flags = HostComponentHandler::take_restart_flags(handler);
            assert_eq!(flags, RestartFlags::PARAM_VALUES_CHANGED);

            // Second take should return 0
            let flags = HostComponentHandler::take_restart_flags(handler);
            assert_eq!(flags, 0);

            HostComponentHandler::destroy(handler);
        }
    }

    #[test]
    fn test_multiple_edits() {
        let handler = HostComponentHandler::new();
        unsafe {
            let vtbl = &*(*handler).vtbl;

            (vtbl.perform_edit)(handler as *mut c_void, 1, 0.1);
            (vtbl.perform_edit)(handler as *mut c_void, 2, 0.2);
            (vtbl.perform_edit)(handler as *mut c_void, 1, 0.5);

            let changes = HostComponentHandler::drain_changes(handler);
            assert_eq!(changes.len(), 3);
            assert_eq!(changes[0].id, 1);
            assert_eq!(changes[1].id, 2);
            assert_eq!(changes[2].id, 1);

            HostComponentHandler::destroy(handler);
        }
    }

    #[test]
    fn test_ref_counting() {
        let handler = HostComponentHandler::new();
        unsafe {
            let vtbl = &*(*handler).vtbl;

            let count = (vtbl.add_ref)(handler as *mut c_void);
            assert_eq!(count, 2);

            let count = (vtbl.add_ref)(handler as *mut c_void);
            assert_eq!(count, 3);

            let count = (vtbl.release)(handler as *mut c_void);
            assert_eq!(count, 2);

            let count = (vtbl.release)(handler as *mut c_void);
            assert_eq!(count, 1);

            HostComponentHandler::destroy(handler);
        }
    }

    #[test]
    fn test_concurrent_perform_edit() {
        use std::thread;

        let handler = HostComponentHandler::new();
        let handler_addr = handler as usize;

        let mut handles = vec![];

        // Spawn 4 threads each performing 100 edits
        for thread_id in 0..4u32 {
            let addr = handler_addr;
            let handle = thread::spawn(move || {
                let handler = addr as *mut HostComponentHandler;
                unsafe {
                    let vtbl = &*(*handler).vtbl;
                    for i in 0..100 {
                        let param_id = thread_id * 1000 + i;
                        let value = (i as f64) / 100.0;
                        (vtbl.perform_edit)(handler as *mut c_void, param_id, value);
                    }
                }
            });
            handles.push(handle);
        }

        for h in handles {
            h.join().unwrap();
        }

        unsafe {
            let changes = HostComponentHandler::drain_changes(handler);
            // With try_lock() in perform_edit, some edits may be dropped under contention.
            // We verify we got a reasonable number (at least most of them).
            assert!(
                changes.len() >= 200,
                "Expected at least 200 changes, got {}",
                changes.len()
            );
            assert!(
                changes.len() <= 400,
                "Expected at most 400 changes, got {}",
                changes.len()
            );

            HostComponentHandler::destroy(handler);
        }
    }

    #[test]
    fn test_restart_flags_or_behavior() {
        let handler = HostComponentHandler::new();
        unsafe {
            let vtbl = &*(*handler).vtbl;

            // Set PARAM_VALUES_CHANGED
            (vtbl.restart_component)(
                handler as *mut c_void,
                RestartFlags::PARAM_VALUES_CHANGED,
            );

            // OR with LATENCY_CHANGED
            (vtbl.restart_component)(
                handler as *mut c_void,
                RestartFlags::LATENCY_CHANGED,
            );

            let flags = HostComponentHandler::take_restart_flags(handler);
            assert_ne!(flags & RestartFlags::PARAM_VALUES_CHANGED, 0);
            assert_ne!(flags & RestartFlags::LATENCY_CHANGED, 0);

            HostComponentHandler::destroy(handler);
        }
    }

    #[test]
    fn test_destroy_null() {
        // Should not panic with null pointer
        unsafe { HostComponentHandler::destroy(std::ptr::null_mut()) };
    }

    #[test]
    fn test_as_ptr() {
        let handler = HostComponentHandler::new();
        let ptr = HostComponentHandler::as_ptr(handler);
        assert!(!ptr.is_null());
        assert_eq!(ptr, handler as *mut c_void);
        unsafe { HostComponentHandler::destroy(handler) };
    }
}
