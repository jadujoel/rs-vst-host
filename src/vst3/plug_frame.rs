//! Host-side IPlugFrame implementation for VST3 plugin editor windows.
//!
//! The plugin calls `IPlugFrame::resizeView()` when it wants the host to
//! resize the editor window. The host stores the requested size so the
//! GUI can apply it on the next frame.

use crate::vst3::com::{FUNKNOWN_IID, IPLUG_FRAME_IID, IPlugFrameVtbl, ViewRect};
use std::ffi::c_void;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use tracing::debug;

/// Result code: success.
const K_RESULT_OK: i32 = 0;

/// Result code: not implemented.
const K_NOT_IMPLEMENTED: i32 = 1;

/// Result code: unknown interface.
const K_NO_INTERFACE: i32 = -1;

/// Host-side IPlugFrame COM object.
///
/// Stores a pending resize request that the GUI can poll each frame.
#[repr(C)]
pub struct HostPlugFrame {
    /// Pointer to the static vtable.
    vtbl: *const IPlugFrameVtbl,
    /// Reference count.
    ref_count: AtomicU32,
    /// Pending resize request from the plugin (width, height).
    pending_resize: Mutex<Option<(i32, i32)>>,
}

// Safety: HostPlugFrame uses atomic ref counting and Mutex for thread safety.
unsafe impl Send for HostPlugFrame {}
unsafe impl Sync for HostPlugFrame {}

/// Static vtable for HostPlugFrame.
static HOST_PLUG_FRAME_VTBL: IPlugFrameVtbl = IPlugFrameVtbl {
    query_interface: host_plug_frame_query_interface,
    add_ref: host_plug_frame_add_ref,
    release: host_plug_frame_release,
    resize_view: host_plug_frame_resize_view,
};

impl HostPlugFrame {
    /// Create a new HostPlugFrame on the heap.
    ///
    /// Returns a raw pointer. The caller must eventually call `destroy()`.
    pub fn new() -> *mut Self {
        let frame = Box::new(HostPlugFrame {
            vtbl: &HOST_PLUG_FRAME_VTBL,
            ref_count: AtomicU32::new(1),
            pending_resize: Mutex::new(None),
        });
        Box::into_raw(frame)
    }

    /// Get the COM pointer suitable for passing to `IPlugView::setFrame()`.
    pub unsafe fn as_ptr(frame: *mut Self) -> *mut c_void {
        frame as *mut c_void
    }

    /// Take the pending resize request, if any.
    ///
    /// Returns `Some((width, height))` if the plugin requested a resize
    /// since the last call.
    pub unsafe fn take_pending_resize(frame: *mut Self) -> Option<(i32, i32)> {
        unsafe {
            if frame.is_null() {
                return None;
            }
            let frame_ref = &*frame;
            frame_ref
                .pending_resize
                .lock()
                .ok()
                .and_then(|mut r| r.take())
        }
    }

    /// Destroy a previously created HostPlugFrame.
    pub unsafe fn destroy(frame: *mut Self) {
        unsafe {
            if !frame.is_null() {
                drop(Box::from_raw(frame));
            }
        }
    }
}

// ── COM vtable functions ────────────────────────────────────────────────────

unsafe extern "system" fn host_plug_frame_query_interface(
    this: *mut c_void,
    iid: *const u8,
    obj: *mut *mut c_void,
) -> i32 {
    unsafe {
        if this.is_null() || iid.is_null() || obj.is_null() {
            return K_NO_INTERFACE;
        }

        let iid_slice = std::slice::from_raw_parts(iid, 16);

        if iid_slice == IPLUG_FRAME_IID || iid_slice == FUNKNOWN_IID {
            host_plug_frame_add_ref(this);
            *obj = this;
            K_RESULT_OK
        } else {
            *obj = std::ptr::null_mut();
            K_NO_INTERFACE
        }
    }
}

unsafe extern "system" fn host_plug_frame_add_ref(this: *mut c_void) -> u32 {
    unsafe {
        if this.is_null() {
            return 0;
        }
        let frame = &*(this as *const HostPlugFrame);
        frame.ref_count.fetch_add(1, Ordering::SeqCst) + 1
    }
}

unsafe extern "system" fn host_plug_frame_release(this: *mut c_void) -> u32 {
    unsafe {
        if this.is_null() {
            return 0;
        }
        let frame = &*(this as *const HostPlugFrame);
        // Note: We don't auto-destroy here because the host manages the lifetime
        // via `HostPlugFrame::destroy()`. Self-destruct on ref_count==0 would
        // cause a double-free when `destroy()` is called after the plugin has
        // already released all its references (e.g. during editor close sequence
        // where removed() + setFrame(null) + release() can drop the count to 0
        // before the host calls destroy()).
        let prev = frame.ref_count.fetch_sub(1, Ordering::SeqCst);
        prev - 1
    }
}

unsafe extern "system" fn host_plug_frame_resize_view(
    this: *mut c_void,
    _view: *mut c_void,
    new_size: *mut ViewRect,
) -> i32 {
    unsafe {
        if this.is_null() || new_size.is_null() {
            return K_NOT_IMPLEMENTED;
        }

        let frame = &*(this as *const HostPlugFrame);
        let rect = &*new_size;
        let width = rect.width();
        let height = rect.height();

        debug!(width, height, "Plugin requested editor resize");

        if let Ok(mut pending) = frame.pending_resize.lock() {
            *pending = Some((width, height));
        }

        K_RESULT_OK
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plug_frame_create_destroy() {
        unsafe {
            let frame = HostPlugFrame::new();
            assert!(!frame.is_null());
            HostPlugFrame::destroy(frame);
        }
    }

    #[test]
    fn test_plug_frame_destroy_null() {
        unsafe {
            HostPlugFrame::destroy(std::ptr::null_mut());
        }
    }

    #[test]
    fn test_plug_frame_as_ptr() {
        unsafe {
            let frame = HostPlugFrame::new();
            let ptr = HostPlugFrame::as_ptr(frame);
            assert!(!ptr.is_null());
            assert_eq!(ptr, frame as *mut c_void);
            HostPlugFrame::destroy(frame);
        }
    }

    #[test]
    fn test_plug_frame_no_pending_resize() {
        unsafe {
            let frame = HostPlugFrame::new();
            assert!(HostPlugFrame::take_pending_resize(frame).is_none());
            HostPlugFrame::destroy(frame);
        }
    }

    #[test]
    fn test_plug_frame_pending_resize_null() {
        unsafe {
            assert!(HostPlugFrame::take_pending_resize(std::ptr::null_mut()).is_none());
        }
    }

    #[test]
    fn test_plug_frame_resize_via_vtable() {
        unsafe {
            let frame = HostPlugFrame::new();
            let ptr = HostPlugFrame::as_ptr(frame);

            // Simulate the plugin calling resizeView through the vtable
            let vtbl = &*(*frame).vtbl;
            let mut rect = ViewRect {
                left: 0,
                top: 0,
                right: 800,
                bottom: 600,
            };
            let result = (vtbl.resize_view)(ptr, std::ptr::null_mut(), &mut rect);
            assert_eq!(result, K_RESULT_OK);

            // Check pending
            let pending = HostPlugFrame::take_pending_resize(frame);
            assert_eq!(pending, Some((800, 600)));

            // Should be consumed
            assert!(HostPlugFrame::take_pending_resize(frame).is_none());

            HostPlugFrame::destroy(frame);
        }
    }

    #[test]
    fn test_plug_frame_query_interface_iplug_frame() {
        unsafe {
            let frame = HostPlugFrame::new();
            let ptr = HostPlugFrame::as_ptr(frame);

            let vtbl = &*(*frame).vtbl;
            let mut out: *mut c_void = std::ptr::null_mut();
            let result = (vtbl.query_interface)(ptr, IPLUG_FRAME_IID.as_ptr(), &mut out);
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(out, ptr);

            // Release the extra ref from QI
            (vtbl.release)(ptr);
            HostPlugFrame::destroy(frame);
        }
    }

    #[test]
    fn test_plug_frame_query_interface_funknown() {
        unsafe {
            let frame = HostPlugFrame::new();
            let ptr = HostPlugFrame::as_ptr(frame);

            let vtbl = &*(*frame).vtbl;
            let mut out: *mut c_void = std::ptr::null_mut();
            let result = (vtbl.query_interface)(ptr, FUNKNOWN_IID.as_ptr(), &mut out);
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(out, ptr);

            (vtbl.release)(ptr);
            HostPlugFrame::destroy(frame);
        }
    }

    #[test]
    fn test_plug_frame_query_interface_unknown_iid() {
        unsafe {
            let frame = HostPlugFrame::new();
            let ptr = HostPlugFrame::as_ptr(frame);

            let vtbl = &*(*frame).vtbl;
            let mut out: *mut c_void = std::ptr::null_mut();
            let fake_iid = [0xFFu8; 16];
            let result = (vtbl.query_interface)(ptr, fake_iid.as_ptr(), &mut out);
            assert_eq!(result, K_NO_INTERFACE);
            assert!(out.is_null());

            HostPlugFrame::destroy(frame);
        }
    }

    #[test]
    fn test_plug_frame_ref_counting() {
        unsafe {
            let frame = HostPlugFrame::new();
            let ptr = HostPlugFrame::as_ptr(frame);
            let vtbl = &*(*frame).vtbl;

            // Initial ref count is 1
            let count = (vtbl.add_ref)(ptr); // now 2
            assert_eq!(count, 2);

            let count = (vtbl.add_ref)(ptr); // now 3
            assert_eq!(count, 3);

            let count = (vtbl.release)(ptr); // now 2
            assert_eq!(count, 2);

            let count = (vtbl.release)(ptr); // now 1
            assert_eq!(count, 1);

            // Don't call release again — use destroy instead to avoid double-free
            HostPlugFrame::destroy(frame);
        }
    }

    #[test]
    fn test_plug_frame_null_safety() {
        unsafe {
            let vtbl = &HOST_PLUG_FRAME_VTBL;

            // Null this pointer
            let result = (vtbl.resize_view)(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            assert_ne!(result, K_RESULT_OK);

            // Null iid
            let result = (vtbl.query_interface)(
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null_mut(),
            );
            assert_eq!(result, K_NO_INTERFACE);

            // Null add_ref / release
            assert_eq!((vtbl.add_ref)(std::ptr::null_mut()), 0);
            assert_eq!((vtbl.release)(std::ptr::null_mut()), 0);
        }
    }

    /// Regression test: release() must NOT self-destruct when ref_count
    /// drops to zero, because the host calls destroy() explicitly afterward.
    /// A self-destructing release caused a double-free ("Corruption of tiny
    /// freelist") when the plugin released all its refs during editor close
    /// before the host called destroy().
    #[test]
    fn test_plug_frame_release_does_not_self_destruct() {
        unsafe {
            let frame = HostPlugFrame::new();
            let ptr = HostPlugFrame::as_ptr(frame);
            let vtbl = &*(*frame).vtbl;

            // Simulate plugin calling AddRef (e.g. during setFrame)
            let count = (vtbl.add_ref)(ptr); // ref_count: 1 → 2
            assert_eq!(count, 2);

            // Simulate plugin releasing during removed() / setFrame(null)
            let count = (vtbl.release)(ptr); // ref_count: 2 → 1
            assert_eq!(count, 1);

            // Simulate plugin releasing again during view destructor (release())
            let count = (vtbl.release)(ptr); // ref_count: 1 → 0
            assert_eq!(count, 0);

            // Host calls destroy() — this must NOT be a double-free.
            // If release() had self-destructed at ref_count==0, this would
            // be a use-after-free / double-free.
            HostPlugFrame::destroy(frame);
        }
    }

    /// Simulates the full editor open/close lifecycle to verify no double-free.
    /// Mirrors the sequence: setFrame(frame) → removed() → setFrame(null) → release()
    #[test]
    fn test_plug_frame_editor_close_lifecycle() {
        unsafe {
            let frame = HostPlugFrame::new();
            let ptr = HostPlugFrame::as_ptr(frame);
            let vtbl = &*(*frame).vtbl;

            // Plugin AddRef during setFrame (ref_count: 1 → 2)
            (vtbl.add_ref)(ptr);

            // Plugin may call resizeView during its lifetime
            let mut rect = ViewRect {
                left: 0,
                top: 0,
                right: 640,
                bottom: 480,
            };
            (vtbl.resize_view)(ptr, std::ptr::null_mut(), &mut rect);

            // Editor close: plugin releases (ref_count: 2 → 1)
            (vtbl.release)(ptr);

            // View destructor releases again (ref_count: 1 → 0)
            // This must NOT free the memory
            (vtbl.release)(ptr);

            // Host safely destroys the frame
            HostPlugFrame::destroy(frame);
        }
    }
}
