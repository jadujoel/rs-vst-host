//! Plugin editor window management.
//!
//! Creates and manages native OS windows for hosting VST3 plugin editor views.
//! Each editor window wraps an `IPlugView` attached to a platform-native parent
//! view (NSView on macOS, HWND on Windows).
//!
//! On macOS, uses Objective-C runtime FFI to create NSWindow + NSView.

use crate::vst3::com::{ComPtr, IPlugViewVtbl, K_PLATFORM_TYPE_NSVIEW, ViewRect};
use crate::vst3::plug_frame::HostPlugFrame;
use std::ffi::c_void;
use tracing::{debug, info, warn};

/// Result code: success.
const K_RESULT_OK: i32 = 0;

/// Represents an open plugin editor window.
pub struct EditorWindow {
    /// The IPlugView COM pointer (owned — released on close).
    view: *mut ComPtr<IPlugViewVtbl>,
    /// The host-side IPlugFrame (owned — destroyed on close).
    plug_frame: *mut HostPlugFrame,
    /// Platform-specific native window handle.
    native_window: NativeWindow,
    /// Plugin name (for display/logging).
    pub plugin_name: String,
    /// Whether the view is currently attached.
    attached: bool,
}

// Safety: EditorWindow is managed from the main/GUI thread only.
// The COM pointers are valid for the lifetime of the window.
unsafe impl Send for EditorWindow {}

/// Platform-specific native window wrapper.
#[cfg(target_os = "macos")]
struct NativeWindow {
    /// NSWindow pointer.
    window: *mut c_void,
    /// NSView (contentView) pointer.
    view: *mut c_void,
}

#[cfg(not(target_os = "macos"))]
struct NativeWindow {
    // Stub for non-macOS platforms
    _placeholder: (),
}

impl EditorWindow {
    /// Open a new editor window for the given plugin view.
    ///
    /// Creates a native window, attaches the IPlugView, and shows the window.
    /// Returns `None` if the platform is not supported or attachment fails.
    #[cfg(target_os = "macos")]
    pub fn open(view: *mut ComPtr<IPlugViewVtbl>, plugin_name: &str) -> Option<Self> {
        unsafe {
            let vtbl = &*(*view).vtbl;

            // Check if the plugin supports NSView
            let result = (vtbl.is_platform_type_supported)(
                view as *mut c_void,
                K_PLATFORM_TYPE_NSVIEW.as_ptr(),
            );
            if result != K_RESULT_OK {
                warn!(plugin = %plugin_name, "Plugin does not support NSView editor");
                (vtbl.release)(view as *mut c_void);
                return None;
            }

            // Get the preferred editor size
            let mut rect = ViewRect::default();
            let size_result = (vtbl.get_size)(view as *mut c_void, &mut rect);
            let (width, height) =
                if size_result == K_RESULT_OK && rect.width() > 0 && rect.height() > 0 {
                    (rect.width() as f64, rect.height() as f64)
                } else {
                    (800.0, 600.0) // Fallback size
                };

            // Create the native window
            let native_window = macos::create_window(plugin_name, width, height)?;

            // Create and install the IPlugFrame
            let plug_frame = HostPlugFrame::new();
            let frame_result =
                (vtbl.set_frame)(view as *mut c_void, HostPlugFrame::as_ptr(plug_frame));
            if frame_result != K_RESULT_OK {
                debug!(plugin = %plugin_name, "setFrame returned {}", frame_result);
                // Continue anyway — some plugins don't use it
            }

            // Attach the view to the native window's NSView
            let attach_result = (vtbl.attached)(
                view as *mut c_void,
                native_window.view,
                K_PLATFORM_TYPE_NSVIEW.as_ptr(),
            );

            if attach_result != K_RESULT_OK {
                warn!(plugin = %plugin_name, result = attach_result, "IPlugView::attached failed");
                macos::close_window(&native_window);
                HostPlugFrame::destroy(plug_frame);
                (vtbl.release)(view as *mut c_void);
                return None;
            }

            // Notify the view of its size
            let mut view_rect = ViewRect {
                left: 0,
                top: 0,
                right: width as i32,
                bottom: height as i32,
            };
            (vtbl.on_size)(view as *mut c_void, &mut view_rect);

            // Show the window
            macos::show_window(&native_window);

            info!(plugin = %plugin_name, width, height, "Plugin editor window opened");

            Some(EditorWindow {
                view,
                plug_frame,
                native_window,
                plugin_name: plugin_name.to_string(),
                attached: true,
            })
        }
    }

    /// Stub for non-macOS platforms.
    #[cfg(not(target_os = "macos"))]
    pub fn open(view: *mut ComPtr<IPlugViewVtbl>, plugin_name: &str) -> Option<Self> {
        warn!(plugin = %plugin_name, "Plugin editor windows not supported on this platform");
        unsafe {
            let vtbl = &*(*view).vtbl;
            (vtbl.release)(view as *mut c_void);
        }
        None
    }

    /// Poll for pending resize requests from the plugin.
    ///
    /// If the plugin requested a resize via `IPlugFrame::resizeView()`,
    /// this applies the new size to the native window and notifies the view.
    pub fn poll_resize(&mut self) {
        unsafe {
            if let Some((width, height)) = HostPlugFrame::take_pending_resize(self.plug_frame) {
                #[cfg(target_os = "macos")]
                macos::resize_window(&self.native_window, width as f64, height as f64);

                let mut rect = ViewRect {
                    left: 0,
                    top: 0,
                    right: width,
                    bottom: height,
                };
                let vtbl = &*(*self.view).vtbl;
                (vtbl.on_size)(self.view as *mut c_void, &mut rect);

                debug!(
                    plugin = %self.plugin_name,
                    width, height,
                    "Editor window resized"
                );
            }
        }
    }

    /// Check if the native window is still open.
    #[cfg(target_os = "macos")]
    pub fn is_open(&self) -> bool {
        self.attached && macos::is_window_visible(&self.native_window)
    }

    #[cfg(not(target_os = "macos"))]
    pub fn is_open(&self) -> bool {
        false
    }

    /// Close the editor window and clean up resources.
    pub fn close(&mut self) {
        if !self.attached {
            return;
        }

        unsafe {
            // Detach the plugin view
            let vtbl = &*(*self.view).vtbl;
            (vtbl.removed)(self.view as *mut c_void);
            self.attached = false;

            // Clear the frame reference
            (vtbl.set_frame)(self.view as *mut c_void, std::ptr::null_mut());

            // Release the view
            (vtbl.release)(self.view as *mut c_void);

            // Close the native window
            #[cfg(target_os = "macos")]
            macos::close_window(&self.native_window);

            // Destroy the plug frame
            HostPlugFrame::destroy(self.plug_frame);
        }

        info!(plugin = %self.plugin_name, "Plugin editor window closed");
    }
}

impl Drop for EditorWindow {
    fn drop(&mut self) {
        if self.attached {
            self.close();
        }
    }
}

// ── macOS implementation ────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;

    // Objective-C runtime types
    type Class = *mut c_void;
    type Sel = *mut c_void;
    type Id = *mut c_void;

    #[allow(non_upper_case_globals)]
    const _nil: Id = std::ptr::null_mut();

    // NSWindow style mask flags
    const NS_WINDOW_STYLE_MASK_TITLED: u64 = 1;
    const NS_WINDOW_STYLE_MASK_CLOSABLE: u64 = 1 << 1;
    const NS_WINDOW_STYLE_MASK_MINIATURIZABLE: u64 = 1 << 2;
    const NS_WINDOW_STYLE_MASK_RESIZABLE: u64 = 1 << 3;

    // NSBackingStoreType
    const NS_BACKING_STORE_BUFFERED: u64 = 2;

    // CGRect / NSRect (same layout on 64-bit)
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGSize {
        width: f64,
        height: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGRect {
        origin: CGPoint,
        size: CGSize,
    }

    #[link(name = "objc", kind = "dylib")]
    unsafe extern "C" {
        fn objc_getClass(name: *const i8) -> Class;
        fn sel_registerName(name: *const i8) -> Sel;
        fn objc_msgSend() -> *mut c_void;
    }

    // Helper types for message sending with different signatures
    type MsgSendId = unsafe extern "C" fn(Id, Sel) -> Id;
    type MsgSendIdId = unsafe extern "C" fn(Id, Sel, Id) -> Id;
    type MsgSendVoid = unsafe extern "C" fn(Id, Sel);
    type MsgSendVoidBool = unsafe extern "C" fn(Id, Sel, i8);
    type MsgSendBool = unsafe extern "C" fn(Id, Sel) -> i8;
    type MsgSendInitWindow = unsafe extern "C" fn(Id, Sel, CGRect, u64, u64, i8) -> Id;
    type MsgSendSetFrame = unsafe extern "C" fn(Id, Sel, CGRect, i8);
    type MsgSendSetTitle = unsafe extern "C" fn(Id, Sel, Id) -> ();

    unsafe fn class(name: &str) -> Class {
        unsafe {
            let c_name = std::ffi::CString::new(name).unwrap();
            objc_getClass(c_name.as_ptr())
        }
    }

    unsafe fn sel(name: &str) -> Sel {
        unsafe {
            let c_name = std::ffi::CString::new(name).unwrap();
            sel_registerName(c_name.as_ptr())
        }
    }

    /// Create a native NSWindow with an NSView suitable for plugin editor hosting.
    pub(super) unsafe fn create_window(
        title: &str,
        width: f64,
        height: f64,
    ) -> Option<super::NativeWindow> {
        unsafe {
            // NSWindow alloc
            let ns_window_class = class("NSWindow");
            if ns_window_class.is_null() {
                return None;
            }

            let alloc: MsgSendId = std::mem::transmute(objc_msgSend as *const c_void);
            let window = alloc(ns_window_class, sel("alloc"));
            if window.is_null() {
                return None;
            }

            // initWithContentRect:styleMask:backing:defer:
            let rect = CGRect {
                origin: CGPoint { x: 200.0, y: 200.0 },
                size: CGSize { width, height },
            };
            let style = NS_WINDOW_STYLE_MASK_TITLED
                | NS_WINDOW_STYLE_MASK_CLOSABLE
                | NS_WINDOW_STYLE_MASK_MINIATURIZABLE
                | NS_WINDOW_STYLE_MASK_RESIZABLE;

            let init: MsgSendInitWindow = std::mem::transmute(objc_msgSend as *const c_void);
            let window = init(
                window,
                sel("initWithContentRect:styleMask:backing:defer:"),
                rect,
                style,
                NS_BACKING_STORE_BUFFERED,
                0, // defer = NO
            );
            if window.is_null() {
                return None;
            }

            // Set title
            let ns_string_class = class("NSString");
            let title_c = std::ffi::CString::new(title).unwrap();
            let alloc_string: MsgSendId = std::mem::transmute(objc_msgSend as *const c_void);
            let ns_title = alloc_string(ns_string_class, sel("alloc"));
            let _init_string: MsgSendIdId = std::mem::transmute(objc_msgSend as *const c_void);
            let encoding_utf8: u64 = 4; // NSUTF8StringEncoding
            let init_with_cstring: unsafe extern "C" fn(Id, Sel, *const i8, u64) -> Id =
                std::mem::transmute(objc_msgSend as *const c_void);
            let ns_title = init_with_cstring(
                ns_title,
                sel("initWithCString:encoding:"),
                title_c.as_ptr(),
                encoding_utf8,
            );

            let set_title: MsgSendSetTitle = std::mem::transmute(objc_msgSend as *const c_void);
            set_title(window, sel("setTitle:"), ns_title);

            // Release the title string (window retains it)
            let release: MsgSendVoid = std::mem::transmute(objc_msgSend as *const c_void);
            release(ns_title, sel("release"));

            // Get contentView
            let content_view: MsgSendId = std::mem::transmute(objc_msgSend as *const c_void);
            let view = content_view(window, sel("contentView"));
            if view.is_null() {
                release(window, sel("release"));
                return None;
            }

            // Center the window on screen
            let center: MsgSendVoid = std::mem::transmute(objc_msgSend as *const c_void);
            center(window, sel("center"));

            Some(super::NativeWindow { window, view })
        }
    }

    /// Show the native window.
    pub(super) unsafe fn show_window(native: &super::NativeWindow) {
        unsafe {
            let make_key: MsgSendVoidBool = std::mem::transmute(objc_msgSend as *const c_void);
            make_key(native.window, sel("makeKeyAndOrderFront:"), 0);
        }
    }

    /// Check if the window is still visible.
    pub(super) fn is_window_visible(native: &super::NativeWindow) -> bool {
        unsafe {
            let is_visible: MsgSendBool = std::mem::transmute(objc_msgSend as *const c_void);
            is_visible(native.window, sel("isVisible")) != 0
        }
    }

    /// Resize the native window content area.
    pub(super) unsafe fn resize_window(native: &super::NativeWindow, width: f64, height: f64) {
        unsafe {
            // Get the current window frame
            type MsgSendGetFrame = unsafe extern "C" fn(Id, Sel) -> CGRect;
            let get_frame: MsgSendGetFrame = std::mem::transmute(objc_msgSend as *const c_void);
            let current = get_frame(native.window, sel("frame"));

            // Calculate new frame (keep top-left position)
            // frameRectForContentRect: tells us how big the window frame should be for the content size
            type MsgSendFrameForContent = unsafe extern "C" fn(Id, Sel, CGRect) -> CGRect;
            let frame_for_content: MsgSendFrameForContent =
                std::mem::transmute(objc_msgSend as *const c_void);
            let content_rect = CGRect {
                origin: CGPoint { x: 0.0, y: 0.0 },
                size: CGSize { width, height },
            };
            let needed =
                frame_for_content(native.window, sel("frameRectForContentRect:"), content_rect);

            let new_frame = CGRect {
                origin: CGPoint {
                    x: current.origin.x,
                    y: current.origin.y + current.size.height - needed.size.height,
                },
                size: needed.size,
            };

            let set_frame: MsgSendSetFrame = std::mem::transmute(objc_msgSend as *const c_void);
            set_frame(native.window, sel("setFrame:display:"), new_frame, 1);
        }
    }

    /// Close and release the native window.
    pub(super) unsafe fn close_window(native: &super::NativeWindow) {
        unsafe {
            let close: MsgSendVoid = std::mem::transmute(objc_msgSend as *const c_void);
            close(native.window, sel("close"));
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_view_rect_platform_constant() {
        assert_eq!(K_PLATFORM_TYPE_NSVIEW, b"NSView\0");
    }

    #[test]
    fn test_editor_window_struct_size() {
        // Just verify the type compiles and has a reasonable size
        assert!(std::mem::size_of::<EditorWindow>() > 0);
    }

    #[test]
    fn test_k_result_ok_value() {
        assert_eq!(K_RESULT_OK, 0);
    }
}
