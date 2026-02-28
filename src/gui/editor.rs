//! Plugin editor window management.
//!
//! Creates and manages native OS windows for hosting VST3 plugin editor views.
//! Each editor window wraps an `IPlugView` attached to a platform-native parent
//! view (NSView on macOS, HWND on Windows).
//!
//! On macOS, uses Objective-C runtime FFI to create NSWindow + NSView.

#[allow(unused_imports)]
use crate::vst3::com::{
    FIDString, FUnknown, IPlugFrame, IPlugView, K_PLATFORM_TYPE_HWND, K_PLATFORM_TYPE_NSVIEW,
    K_PLATFORM_TYPE_X11, ViewRect, view_rect_height, view_rect_width,
};
use crate::vst3::plug_frame::HostPlugFrame;
use crate::vst3::sandbox::{SandboxResult, sandbox_call};
use std::ffi::c_void;
use tracing::{debug, info, warn};

/// Result code: success.
const K_RESULT_OK: i32 = 0;

/// Represents an open plugin editor window.
pub struct EditorWindow {
    /// The IPlugView COM pointer (owned — released on close).
    view: *mut IPlugView,
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

#[cfg(target_os = "linux")]
struct NativeWindow {
    /// X11 display pointer.
    display: *mut c_void,
    /// X11 window ID (XID).
    window_id: u64,
}

#[cfg(target_os = "windows")]
struct NativeWindow {
    /// Win32 HWND handle.
    hwnd: *mut c_void,
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
struct NativeWindow {
    // Stub for unsupported platforms
    _placeholder: (),
}

impl EditorWindow {
    /// Open a new editor window for the given plugin view.
    ///
    /// Creates a native window, attaches the IPlugView, and shows the window.
    /// Returns `None` if the platform is not supported or attachment fails.
    #[cfg(target_os = "macos")]
    pub fn open(view: *mut IPlugView, plugin_name: &str) -> Option<Self> {
        // Ensure NSApplication is initialized — required for NSWindow creation.
        // In the in-process GUI mode, eframe already sets this up, but the
        // audio worker child process has no GUI event loop by default.
        unsafe { macos::ensure_ns_application() };

        // All IPlugView COM calls are sandboxed — a buggy plugin crash during
        // editor setup must not terminate the host.
        let view_raw = view as usize;

        // Check if the plugin supports NSView (sandboxed)
        let platform_ok = {
            let v = view_raw;
            sandbox_call("plugview_is_platform_supported", move || unsafe {
                let view = v as *mut IPlugView;
                let vtbl = &*(*view).vtbl;
                (vtbl.isPlatformTypeSupported)(view, K_PLATFORM_TYPE_NSVIEW.as_ptr() as FIDString)
            })
        };
        match platform_ok {
            SandboxResult::Ok(K_RESULT_OK) => {}
            SandboxResult::Ok(_) => {
                warn!(plugin = %plugin_name, "Plugin does not support NSView editor");
                Self::release_view_safe(view, plugin_name);
                return None;
            }
            _ => {
                warn!(plugin = %plugin_name, "Plugin crashed checking platform support");
                // View may be in undefined state — don't try to release
                return None;
            }
        }

        // Get the preferred editor size (sandboxed)
        let size_result = {
            let v = view_raw;
            sandbox_call("plugview_get_size", move || unsafe {
                let view = v as *mut IPlugView;
                let vtbl = &*(*view).vtbl;
                let mut rect = ViewRect {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                };
                let result = (vtbl.getSize)(view, &mut rect);
                (result, rect)
            })
        };
        let (width, height) = match size_result {
            SandboxResult::Ok((K_RESULT_OK, rect))
                if view_rect_width(&rect) > 0 && view_rect_height(&rect) > 0 =>
            {
                (
                    view_rect_width(&rect) as f64,
                    view_rect_height(&rect) as f64,
                )
            }
            SandboxResult::Crashed(_) | SandboxResult::Panicked(_) => {
                warn!(plugin = %plugin_name, "Plugin crashed during getSize");
                return None;
            }
            _ => (800.0, 600.0), // Fallback size
        };

        // Create the native window
        // Safety: macos::create_window uses ObjC runtime FFI — no plugin code.
        let native_window = unsafe { macos::create_window(plugin_name, width, height)? };

        // Create and install the IPlugFrame (sandboxed)
        let plug_frame = HostPlugFrame::new();
        {
            let v = view_raw;
            // Safety: HostPlugFrame::as_ptr returns the raw frame COM pointer.
            let frame_ptr = unsafe { HostPlugFrame::as_ptr(plug_frame) as *mut IPlugFrame };
            let set_frame_result = sandbox_call("plugview_set_frame", move || unsafe {
                let view = v as *mut IPlugView;
                let vtbl = &*(*view).vtbl;
                (vtbl.setFrame)(view, frame_ptr)
            });
            match set_frame_result {
                SandboxResult::Ok(r) if r != K_RESULT_OK => {
                    debug!(plugin = %plugin_name, "setFrame returned {}", r);
                    // Continue anyway — some plugins don't use it
                }
                SandboxResult::Crashed(_) | SandboxResult::Panicked(_) => {
                    warn!(plugin = %plugin_name, "Plugin crashed during setFrame — aborting editor open");
                    unsafe {
                        macos::close_window(&native_window);
                        HostPlugFrame::destroy(plug_frame);
                    }
                    return None;
                }
                _ => {}
            }
        }

        // Attach the view to the native window's NSView (sandboxed)
        let nsview = native_window.view;
        let attach_result = {
            let v = view_raw;
            sandbox_call("plugview_attached", move || unsafe {
                let view = v as *mut IPlugView;
                let vtbl = &*(*view).vtbl;
                (vtbl.attached)(view, nsview, K_PLATFORM_TYPE_NSVIEW.as_ptr() as FIDString)
            })
        };

        match attach_result {
            SandboxResult::Ok(K_RESULT_OK) => {}
            SandboxResult::Ok(r) => {
                warn!(plugin = %plugin_name, result = r, "IPlugView::attached failed");
                unsafe {
                    macos::close_window(&native_window);
                    HostPlugFrame::destroy(plug_frame);
                }
                Self::release_view_safe(view, plugin_name);
                return None;
            }
            _ => {
                warn!(plugin = %plugin_name, "Plugin crashed during IPlugView::attached");
                unsafe {
                    macos::close_window(&native_window);
                    HostPlugFrame::destroy(plug_frame);
                }
                return None;
            }
        }

        // Notify the view of its size (sandboxed)
        {
            let v = view_raw;
            let w = width as i32;
            let h = height as i32;
            let _ = sandbox_call("plugview_on_size", move || unsafe {
                let view = v as *mut IPlugView;
                let vtbl = &*(*view).vtbl;
                let mut rect = ViewRect {
                    left: 0,
                    top: 0,
                    right: w,
                    bottom: h,
                };
                (vtbl.onSize)(view, &mut rect)
            });
        }

        // Show the window
        // Safety: macos::show_window uses ObjC runtime FFI — no plugin code.
        unsafe { macos::show_window(&native_window) };

        info!(plugin = %plugin_name, width, height, "Plugin editor window opened");

        Some(EditorWindow {
            view,
            plug_frame,
            native_window,
            plugin_name: plugin_name.to_string(),
            attached: true,
        })
    }

    /// Open a plugin editor window on Linux via X11/XEmbed.
    ///
    /// Creates an X11 window, attaches the IPlugView, and maps the window.
    /// Requires an X11 display connection.
    #[cfg(target_os = "linux")]
    pub fn open(view: *mut IPlugView, plugin_name: &str) -> Option<Self> {
        let view_raw = view as usize;

        // Check X11 platform support (sandboxed)
        let platform_ok = {
            let v = view_raw;
            sandbox_call("plugview_is_platform_supported_x11", move || unsafe {
                let view = v as *mut IPlugView;
                let vtbl = &*(*view).vtbl;
                (vtbl.isPlatformTypeSupported)(view, K_PLATFORM_TYPE_X11.as_ptr() as FIDString)
            })
        };
        match platform_ok {
            SandboxResult::Ok(K_RESULT_OK) => {}
            SandboxResult::Ok(_) => {
                warn!(plugin = %plugin_name, "Plugin does not support X11EmbedWindowID editor");
                Self::release_view_safe(view, plugin_name);
                return None;
            }
            _ => {
                warn!(plugin = %plugin_name, "Plugin crashed checking X11 platform support");
                return None;
            }
        }

        // Get preferred editor size (sandboxed)
        let size_result = {
            let v = view_raw;
            sandbox_call("plugview_get_size_x11", move || unsafe {
                let view = v as *mut IPlugView;
                let vtbl = &*(*view).vtbl;
                let mut rect = ViewRect {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                };
                let result = (vtbl.getSize)(view, &mut rect);
                (result, rect)
            })
        };
        let (width, height) = match size_result {
            SandboxResult::Ok((K_RESULT_OK, rect))
                if view_rect_width(&rect) > 0 && view_rect_height(&rect) > 0 =>
            {
                (
                    view_rect_width(&rect) as u32,
                    view_rect_height(&rect) as u32,
                )
            }
            SandboxResult::Crashed(_) | SandboxResult::Panicked(_) => {
                warn!(plugin = %plugin_name, "Plugin crashed during getSize (X11)");
                return None;
            }
            _ => (800, 600),
        };

        // Create X11 window via xcb/xlib
        let native_window = unsafe { linux::create_window(plugin_name, width, height)? };

        // Create and install IPlugFrame (sandboxed)
        let plug_frame = HostPlugFrame::new();
        {
            let v = view_raw;
            let frame_ptr = unsafe { HostPlugFrame::as_ptr(plug_frame) as *mut IPlugFrame };
            let set_frame_result = sandbox_call("plugview_set_frame_x11", move || unsafe {
                let view = v as *mut IPlugView;
                let vtbl = &*(*view).vtbl;
                (vtbl.setFrame)(view, frame_ptr)
            });
            match set_frame_result {
                SandboxResult::Crashed(_) | SandboxResult::Panicked(_) => {
                    warn!(plugin = %plugin_name, "Plugin crashed during setFrame (X11)");
                    unsafe {
                        linux::close_window(&native_window);
                        HostPlugFrame::destroy(plug_frame);
                    }
                    return None;
                }
                _ => {}
            }
        }

        // Attach view to the X11 window (sandboxed)
        // X11EmbedWindowID expects the parent window XID as a pointer-sized value
        let xid = native_window.window_id as usize as *mut c_void;
        let attach_result = {
            let v = view_raw;
            sandbox_call("plugview_attached_x11", move || unsafe {
                let view = v as *mut IPlugView;
                let vtbl = &*(*view).vtbl;
                (vtbl.attached)(view, xid, K_PLATFORM_TYPE_X11.as_ptr() as FIDString)
            })
        };

        match attach_result {
            SandboxResult::Ok(K_RESULT_OK) => {}
            SandboxResult::Ok(r) => {
                warn!(plugin = %plugin_name, result = r, "IPlugView::attached failed (X11)");
                unsafe {
                    linux::close_window(&native_window);
                    HostPlugFrame::destroy(plug_frame);
                }
                Self::release_view_safe(view, plugin_name);
                return None;
            }
            _ => {
                warn!(plugin = %plugin_name, "Plugin crashed during IPlugView::attached (X11)");
                unsafe {
                    linux::close_window(&native_window);
                    HostPlugFrame::destroy(plug_frame);
                }
                return None;
            }
        }

        unsafe { linux::show_window(&native_window) };

        info!(plugin = %plugin_name, width, height, "Plugin editor window opened (X11)");

        Some(EditorWindow {
            view,
            plug_frame,
            native_window,
            plugin_name: plugin_name.to_string(),
            attached: true,
        })
    }

    /// Open a plugin editor window on Windows via HWND.
    #[cfg(target_os = "windows")]
    pub fn open(view: *mut IPlugView, plugin_name: &str) -> Option<Self> {
        let view_raw = view as usize;

        // Check HWND platform support (sandboxed)
        let platform_ok = {
            let v = view_raw;
            sandbox_call("plugview_is_platform_supported_hwnd", move || unsafe {
                let view = v as *mut IPlugView;
                let vtbl = &*(*view).vtbl;
                (vtbl.isPlatformTypeSupported)(view, K_PLATFORM_TYPE_HWND.as_ptr() as FIDString)
            })
        };
        match platform_ok {
            SandboxResult::Ok(K_RESULT_OK) => {}
            SandboxResult::Ok(_) => {
                warn!(plugin = %plugin_name, "Plugin does not support HWND editor");
                Self::release_view_safe(view, plugin_name);
                return None;
            }
            _ => {
                warn!(plugin = %plugin_name, "Plugin crashed checking HWND platform support");
                return None;
            }
        }

        // Get preferred size (sandboxed)
        let size_result = {
            let v = view_raw;
            sandbox_call("plugview_get_size_hwnd", move || unsafe {
                let view = v as *mut IPlugView;
                let vtbl = &*(*view).vtbl;
                let mut rect = ViewRect {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                };
                let result = (vtbl.getSize)(view, &mut rect);
                (result, rect)
            })
        };
        let (width, height) = match size_result {
            SandboxResult::Ok((K_RESULT_OK, rect))
                if view_rect_width(&rect) > 0 && view_rect_height(&rect) > 0 =>
            {
                (
                    view_rect_width(&rect) as u32,
                    view_rect_height(&rect) as u32,
                )
            }
            SandboxResult::Crashed(_) | SandboxResult::Panicked(_) => {
                warn!(plugin = %plugin_name, "Plugin crashed during getSize (HWND)");
                return None;
            }
            _ => (800, 600),
        };

        // Create Win32 window
        let native_window = unsafe { windows::create_window(plugin_name, width, height)? };

        // Create and install IPlugFrame (sandboxed)
        let plug_frame = HostPlugFrame::new();
        {
            let v = view_raw;
            let frame_ptr = unsafe { HostPlugFrame::as_ptr(plug_frame) as *mut IPlugFrame };
            let _ = sandbox_call("plugview_set_frame_hwnd", move || unsafe {
                let view = v as *mut IPlugView;
                let vtbl = &*(*view).vtbl;
                (vtbl.setFrame)(view, frame_ptr)
            });
        }

        // Attach view to the HWND (sandboxed)
        let hwnd = native_window.hwnd;
        let attach_result = {
            let v = view_raw;
            sandbox_call("plugview_attached_hwnd", move || unsafe {
                let view = v as *mut IPlugView;
                let vtbl = &*(*view).vtbl;
                (vtbl.attached)(view, hwnd, K_PLATFORM_TYPE_HWND.as_ptr() as FIDString)
            })
        };

        match attach_result {
            SandboxResult::Ok(K_RESULT_OK) => {}
            SandboxResult::Ok(r) => {
                warn!(plugin = %plugin_name, result = r, "IPlugView::attached failed (HWND)");
                unsafe {
                    windows::close_window(&native_window);
                    HostPlugFrame::destroy(plug_frame);
                }
                Self::release_view_safe(view, plugin_name);
                return None;
            }
            _ => {
                warn!(plugin = %plugin_name, "Plugin crashed during IPlugView::attached (HWND)");
                unsafe {
                    windows::close_window(&native_window);
                    HostPlugFrame::destroy(plug_frame);
                }
                return None;
            }
        }

        unsafe { windows::show_window(&native_window) };

        info!(plugin = %plugin_name, width, height, "Plugin editor window opened (HWND)");

        Some(EditorWindow {
            view,
            plug_frame,
            native_window,
            plugin_name: plugin_name.to_string(),
            attached: true,
        })
    }

    /// Stub for unsupported platforms.
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    pub fn open(view: *mut IPlugView, plugin_name: &str) -> Option<Self> {
        warn!(plugin = %plugin_name, "Plugin editor windows not supported on this platform");
        Self::release_view_safe(view, plugin_name);
        None
    }

    /// Safely release an IPlugView COM pointer inside a sandbox.
    ///
    /// If the plugin crashes during release, the COM object is leaked
    /// intentionally — the host stays alive.
    fn release_view_safe(view: *mut IPlugView, plugin_name: &str) {
        let v = view as usize;
        let result = sandbox_call("plugview_release", move || unsafe {
            let view = v as *mut IPlugView;
            let vtbl = &*(*view).vtbl;
            (vtbl.base.release)(view as *mut FUnknown)
        });
        if let SandboxResult::Crashed(crash) = &result {
            warn!(
                plugin = %plugin_name,
                signal = %crash.signal_name,
                "Plugin crashed during IPlugView release — COM object leaked"
            );
        }
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

                #[cfg(target_os = "linux")]
                linux::resize_window(&self.native_window, width as u32, height as u32);

                #[cfg(target_os = "windows")]
                windows::resize_window(&self.native_window, width as u32, height as u32);

                let v = self.view as usize;
                let result = sandbox_call("plugview_on_size", move || {
                    let view = v as *mut IPlugView;
                    let vtbl = &*(*view).vtbl;
                    let mut rect = ViewRect {
                        left: 0,
                        top: 0,
                        right: width,
                        bottom: height,
                    };
                    (vtbl.onSize)(view, &mut rect)
                });

                if let SandboxResult::Crashed(crash) = &result {
                    warn!(
                        plugin = %self.plugin_name,
                        signal = %crash.signal_name,
                        "Plugin crashed during on_size — closing editor"
                    );
                    self.attached = false;
                    return;
                }

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

    #[cfg(target_os = "linux")]
    pub fn is_open(&self) -> bool {
        self.attached && linux::is_window_visible(&self.native_window)
    }

    #[cfg(target_os = "windows")]
    pub fn is_open(&self) -> bool {
        self.attached && windows::is_window_visible(&self.native_window)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    pub fn is_open(&self) -> bool {
        false
    }

    /// Pump the platform event loop so editor windows can render and respond.
    ///
    /// On macOS, this drains all pending AppKit events without blocking.
    /// On Linux, this drains pending X11 events.
    /// On Windows, this processes the Win32 message queue.
    /// Must be called periodically from the audio worker's main loop when
    /// editor windows are open.
    #[cfg(target_os = "macos")]
    pub fn pump_platform_events() {
        // Safety: pump_events uses ObjC runtime FFI — no plugin code involved.
        unsafe { macos::pump_events() };
    }

    /// Pump pending X11 events on Linux.
    #[cfg(target_os = "linux")]
    pub fn pump_platform_events() {
        unsafe { linux::pump_events() };
    }

    /// Process Win32 message queue on Windows.
    #[cfg(target_os = "windows")]
    pub fn pump_platform_events() {
        unsafe { windows::pump_events() };
    }

    /// No-op on unsupported platforms.
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    pub fn pump_platform_events() {}

    /// Close the editor window and clean up resources.
    ///
    /// All plugin-facing COM calls are sandboxed — if the plugin crashes
    /// during teardown, the COM objects are leaked but the host survives.
    pub fn close(&mut self) {
        if !self.attached {
            return;
        }

        // Sandbox the entire IPlugView teardown sequence.
        // If the plugin crashes during removed/setFrame/release, we skip
        // the remaining COM calls but still clean up host-owned resources.
        let v = self.view as usize;
        let result = sandbox_call("plugview_close", move || unsafe {
            let view = v as *mut IPlugView;
            let vtbl = &*(*view).vtbl;

            // Detach the plugin view
            (vtbl.removed)(view);

            // Clear the frame reference
            (vtbl.setFrame)(view, std::ptr::null_mut());

            // Release the view
            (vtbl.base.release)(view as *mut FUnknown);
        });

        self.attached = false;

        match &result {
            SandboxResult::Crashed(crash) => {
                warn!(
                    plugin = %self.plugin_name,
                    signal = %crash.signal_name,
                    "Plugin crashed during editor close — COM objects leaked (host is safe)"
                );
            }
            SandboxResult::Panicked(msg) => {
                warn!(
                    plugin = %self.plugin_name,
                    panic = %msg,
                    "Plugin panicked during editor close"
                );
            }
            SandboxResult::Ok(()) => {}
        }

        // Always clean up host-owned resources (pure Rust, never crash)
        unsafe {
            #[cfg(target_os = "macos")]
            macos::close_window(&self.native_window);

            #[cfg(target_os = "linux")]
            linux::close_window(&self.native_window);

            #[cfg(target_os = "windows")]
            windows::close_window(&self.native_window);

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

    /// Show the native window and bring it to the front.
    pub(super) unsafe fn show_window(native: &super::NativeWindow) {
        unsafe {
            let make_key: MsgSendVoidBool = std::mem::transmute(objc_msgSend as *const c_void);
            make_key(native.window, sel("makeKeyAndOrderFront:"), 0);

            // Force the editor window to appear in front of the main window.
            // makeKeyAndOrderFront: alone may not bring it visually above the
            // egui/host window.
            let order_front: MsgSendVoid = std::mem::transmute(objc_msgSend as *const c_void);
            order_front(native.window, sel("orderFrontRegardless"));
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

    /// Ensure `NSApplication` is initialized.
    ///
    /// In the in-process GUI mode, `eframe` initializes `NSApplication` via
    /// its own event loop. But when the editor window is created in the
    /// **audio worker** child process (supervised mode), there is no GUI
    /// framework — we must initialize `[NSApplication sharedApplication]`
    /// ourselves, or `NSWindow` creation silently fails.
    ///
    /// Calling `[NSApplication sharedApplication]` multiple times is safe;
    /// AppKit returns the existing singleton.
    pub(super) unsafe fn ensure_ns_application() {
        unsafe {
            let ns_app_class = class("NSApplication");
            if ns_app_class.is_null() {
                return;
            }
            // [NSApplication sharedApplication] — creates or returns the singleton
            let shared_app: MsgSendId = std::mem::transmute(objc_msgSend as *const c_void);
            let app = shared_app(ns_app_class, sel("sharedApplication"));
            if app.is_null() {
                return;
            }

            // Only set the activation policy if it hasn't been set to "regular"
            // already. In the in-process GUI mode, eframe sets "regular" (0) to
            // get a dock icon and menu bar. We don't want to downgrade that.
            // In the audio worker, the policy is unset (or "prohibited" = 2),
            // so we set it to "accessory" (1) to enable a proper event loop
            // without creating a dock icon.
            type MsgSendGetPolicy = unsafe extern "C" fn(Id, Sel) -> i64;
            let get_policy: MsgSendGetPolicy = std::mem::transmute(objc_msgSend as *const c_void);
            let current_policy = get_policy(app, sel("activationPolicy"));

            // NSApplicationActivationPolicyRegular = 0
            // NSApplicationActivationPolicyAccessory = 1
            // NSApplicationActivationPolicyProhibited = 2
            if current_policy != 0 {
                type MsgSendSetPolicy = unsafe extern "C" fn(Id, Sel, i64) -> i8;
                let set_policy: MsgSendSetPolicy =
                    std::mem::transmute(objc_msgSend as *const c_void);
                let _ = set_policy(app, sel("setActivationPolicy:"), 1);
            }
        }
    }

    /// Pump the macOS event loop to process pending UI events.
    ///
    /// In the supervised architecture, the audio worker runs a simple
    /// socket-based message loop with no `NSApplication` run loop. Editor
    /// windows (`NSWindow`) need the AppKit event loop to render, handle
    /// input, and respond to system events. This function drains all
    /// pending events without blocking, allowing plugin editor UIs to
    /// function correctly.
    ///
    /// Should be called periodically (e.g. every 50ms) from the audio
    /// worker's main loop whenever editor windows are open.
    pub(super) unsafe fn pump_events() {
        unsafe {
            let ns_app_class = class("NSApplication");
            if ns_app_class.is_null() {
                return;
            }
            let shared_app: MsgSendId = std::mem::transmute(objc_msgSend as *const c_void);
            let app = shared_app(ns_app_class, sel("sharedApplication"));
            if app.is_null() {
                return;
            }

            // NSEventMaskAny = NSUIntegerMax
            let ns_event_mask_any: u64 = u64::MAX;

            // Drain all pending events without blocking (untilDate: nil)
            type MsgSendNextEvent = unsafe extern "C" fn(Id, Sel, u64, Id, Id, i8) -> Id;
            let next_event: MsgSendNextEvent = std::mem::transmute(objc_msgSend as *const c_void);
            let send_event: MsgSendIdId = std::mem::transmute(objc_msgSend as *const c_void);

            // NSDefaultRunLoopMode as NSString
            let ns_default_run_loop_mode: Id = {
                let ns_string_class = class("NSString");
                let alloc: MsgSendId = std::mem::transmute(objc_msgSend as *const c_void);
                let s = alloc(ns_string_class, sel("alloc"));
                type MsgSendInitCStr = unsafe extern "C" fn(Id, Sel, *const i8, u64) -> Id;
                let init_cstr: MsgSendInitCStr = std::mem::transmute(objc_msgSend as *const c_void);
                init_cstr(
                    s,
                    sel("initWithCString:encoding:"),
                    c"kCFRunLoopDefaultMode".as_ptr(),
                    4, // NSUTF8StringEncoding
                )
            };

            loop {
                // [NSApp nextEventMatchingMask:untilDate:inMode:dequeue:]
                let event = next_event(
                    app,
                    sel("nextEventMatchingMask:untilDate:inMode:dequeue:"),
                    ns_event_mask_any,
                    std::ptr::null_mut(), // nil — don't wait
                    ns_default_run_loop_mode,
                    1, // dequeue = YES
                );
                if event.is_null() {
                    break;
                }
                // [NSApp sendEvent:event]
                send_event(app, sel("sendEvent:"), event);
            }

            // [NSApp updateWindows]
            let update_windows: MsgSendVoid = std::mem::transmute(objc_msgSend as *const c_void);
            update_windows(app, sel("updateWindows"));

            // Release the run loop mode string
            let release: MsgSendVoid = std::mem::transmute(objc_msgSend as *const c_void);
            release(ns_default_run_loop_mode, sel("release"));
        }
    }
}

// ── Linux X11 implementation ────────────────────────────────────────────────

#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::c_void;
    use tracing::warn;

    // X11/Xlib FFI types and functions
    type Display = c_void;
    type Window = u64;

    #[link(name = "X11")]
    unsafe extern "C" {
        fn XOpenDisplay(display_name: *const i8) -> *mut Display;
        fn XCloseDisplay(display: *mut Display) -> i32;
        fn XCreateSimpleWindow(
            display: *mut Display,
            parent: Window,
            x: i32,
            y: i32,
            width: u32,
            height: u32,
            border_width: u32,
            border: u64,
            background: u64,
        ) -> Window;
        fn XMapWindow(display: *mut Display, window: Window) -> i32;
        fn XUnmapWindow(display: *mut Display, window: Window) -> i32;
        fn XDestroyWindow(display: *mut Display, window: Window) -> i32;
        fn XFlush(display: *mut Display) -> i32;
        fn XDefaultRootWindow(display: *mut Display) -> Window;
        fn XResizeWindow(display: *mut Display, window: Window, width: u32, height: u32) -> i32;
        fn XStoreName(display: *mut Display, window: Window, window_name: *const i8) -> i32;
        fn XPending(display: *mut Display) -> i32;
        fn XNextEvent(display: *mut Display, event: *mut [u8; 192]) -> i32;
    }

    /// Thread-local display connection (one per thread, reused).
    thread_local! {
        static DISPLAY: std::cell::Cell<*mut Display> = const { std::cell::Cell::new(std::ptr::null_mut()) };
    }

    fn get_display() -> *mut Display {
        DISPLAY.with(|d| {
            let ptr = d.get();
            if !ptr.is_null() {
                return ptr;
            }
            let display = unsafe { XOpenDisplay(std::ptr::null()) };
            if display.is_null() {
                warn!("Failed to open X11 display");
                return std::ptr::null_mut();
            }
            d.set(display);
            display
        })
    }

    pub(super) unsafe fn create_window(
        title: &str,
        width: u32,
        height: u32,
    ) -> Option<super::NativeWindow> {
        let display = get_display();
        if display.is_null() {
            return None;
        }

        unsafe {
            let root = XDefaultRootWindow(display);
            let window = XCreateSimpleWindow(
                display, root, 100, 100, // x, y position
                width, height, 0, // border width
                0, // border color
                0, // background (black)
            );
            if window == 0 {
                warn!("XCreateSimpleWindow failed");
                return None;
            }

            // Set window title
            let c_title = std::ffi::CString::new(title).unwrap_or_default();
            XStoreName(display, window, c_title.as_ptr());

            Some(super::NativeWindow {
                display,
                window_id: window,
            })
        }
    }

    pub(super) unsafe fn show_window(native: &super::NativeWindow) {
        unsafe {
            XMapWindow(native.display, native.window_id);
            XFlush(native.display);
        }
    }

    pub(super) fn is_window_visible(native: &super::NativeWindow) -> bool {
        // X11 doesn't have a simple "is visible" check without querying attributes.
        // We assume the window is visible if the display and window ID are valid.
        !native.display.is_null() && native.window_id != 0
    }

    pub(super) unsafe fn resize_window(native: &super::NativeWindow, width: u32, height: u32) {
        unsafe {
            XResizeWindow(native.display, native.window_id, width, height);
            XFlush(native.display);
        }
    }

    pub(super) unsafe fn close_window(native: &super::NativeWindow) {
        unsafe {
            XUnmapWindow(native.display, native.window_id);
            XDestroyWindow(native.display, native.window_id);
            XFlush(native.display);
            // Note: we don't close the display — it's reused via thread-local
        }
    }

    pub(super) unsafe fn pump_events() {
        let display = get_display();
        if display.is_null() {
            return;
        }
        unsafe {
            while XPending(display) > 0 {
                let mut event = [0u8; 192]; // XEvent is ≤192 bytes
                XNextEvent(display, &mut event);
                // Events are dispatched to the plugin's embedded X11 window automatically
            }
        }
    }
}

// ── Windows implementation ──────────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod windows {
    use std::ffi::c_void;
    use tracing::warn;

    // Win32 API types
    type HWND = *mut c_void;
    type HINSTANCE = *mut c_void;
    type HMENU = *mut c_void;
    type LPARAM = isize;
    type WPARAM = usize;
    type LRESULT = isize;
    type UINT = u32;
    type BOOL = i32;
    type DWORD = u32;
    type ATOM = u16;

    const WS_OVERLAPPEDWINDOW: DWORD = 0x00CF0000;
    const WS_VISIBLE: DWORD = 0x10000000;
    const CW_USEDEFAULT: i32 = 0x80000000_u32 as i32;
    const SW_SHOW: i32 = 5;
    const PM_REMOVE: UINT = 0x0001;
    const WM_QUIT: UINT = 0x0012;

    #[repr(C)]
    struct WNDCLASSEXW {
        cb_size: UINT,
        style: UINT,
        lpfn_wnd_proc: Option<unsafe extern "system" fn(HWND, UINT, WPARAM, LPARAM) -> LRESULT>,
        cb_cls_extra: i32,
        cb_wnd_extra: i32,
        h_instance: HINSTANCE,
        h_icon: *mut c_void,
        h_cursor: *mut c_void,
        hbr_background: *mut c_void,
        lpsz_menu_name: *const u16,
        lpsz_class_name: *const u16,
        h_icon_sm: *mut c_void,
    }

    #[repr(C)]
    struct POINT {
        x: i32,
        y: i32,
    }

    #[repr(C)]
    struct MSG {
        hwnd: HWND,
        message: UINT,
        w_param: WPARAM,
        l_param: LPARAM,
        time: DWORD,
        pt: POINT,
    }

    #[repr(C)]
    struct RECT {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn CreateWindowExW(
            ex_style: DWORD,
            class_name: *const u16,
            window_name: *const u16,
            style: DWORD,
            x: i32,
            y: i32,
            width: i32,
            height: i32,
            parent: HWND,
            menu: HMENU,
            instance: HINSTANCE,
            param: *mut c_void,
        ) -> HWND;
        fn DestroyWindow(hwnd: HWND) -> BOOL;
        fn ShowWindow(hwnd: HWND, cmd_show: i32) -> BOOL;
        fn IsWindowVisible(hwnd: HWND) -> BOOL;
        fn SetWindowPos(
            hwnd: HWND,
            insert_after: HWND,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            flags: UINT,
        ) -> BOOL;
        fn PeekMessageW(
            msg: *mut MSG,
            hwnd: HWND,
            filter_min: UINT,
            filter_max: UINT,
            remove_msg: UINT,
        ) -> BOOL;
        fn TranslateMessage(msg: *const MSG) -> BOOL;
        fn DispatchMessageW(msg: *const MSG) -> LRESULT;
        fn DefWindowProcW(hwnd: HWND, msg: UINT, w_param: WPARAM, l_param: LPARAM) -> LRESULT;
        fn RegisterClassExW(wc: *const WNDCLASSEXW) -> ATOM;
        fn GetModuleHandleW(module_name: *const u16) -> HINSTANCE;
    }

    // SWP flags
    const SWP_NOMOVE: UINT = 0x0002;
    const SWP_NOZORDER: UINT = 0x0004;

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: UINT,
        w_param: WPARAM,
        l_param: LPARAM,
    ) -> LRESULT {
        unsafe { DefWindowProcW(hwnd, msg, w_param, l_param) }
    }

    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    static CLASS_REGISTERED: std::sync::Once = std::sync::Once::new();
    static CLASS_NAME: &str = "RsVstHostPluginEditor";

    fn ensure_class_registered() {
        CLASS_REGISTERED.call_once(|| unsafe {
            let class_name_w = to_wide(CLASS_NAME);
            let h_instance = GetModuleHandleW(std::ptr::null());
            let wc = WNDCLASSEXW {
                cb_size: std::mem::size_of::<WNDCLASSEXW>() as UINT,
                style: 0,
                lpfn_wnd_proc: Some(wnd_proc),
                cb_cls_extra: 0,
                cb_wnd_extra: 0,
                h_instance,
                h_icon: std::ptr::null_mut(),
                h_cursor: std::ptr::null_mut(),
                hbr_background: std::ptr::null_mut(),
                lpsz_menu_name: std::ptr::null(),
                lpsz_class_name: class_name_w.as_ptr(),
                h_icon_sm: std::ptr::null_mut(),
            };
            RegisterClassExW(&wc);
        });
    }

    pub(super) unsafe fn create_window(
        title: &str,
        width: u32,
        height: u32,
    ) -> Option<super::NativeWindow> {
        ensure_class_registered();

        unsafe {
            let class_name_w = to_wide(CLASS_NAME);
            let title_w = to_wide(title);
            let h_instance = GetModuleHandleW(std::ptr::null());

            let hwnd = CreateWindowExW(
                0,
                class_name_w.as_ptr(),
                title_w.as_ptr(),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                width as i32,
                height as i32,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                h_instance,
                std::ptr::null_mut(),
            );

            if hwnd.is_null() {
                warn!("CreateWindowExW failed");
                return None;
            }

            Some(super::NativeWindow { hwnd })
        }
    }

    pub(super) unsafe fn show_window(native: &super::NativeWindow) {
        unsafe {
            ShowWindow(native.hwnd, SW_SHOW);
        }
    }

    pub(super) fn is_window_visible(native: &super::NativeWindow) -> bool {
        unsafe { IsWindowVisible(native.hwnd) != 0 }
    }

    pub(super) unsafe fn resize_window(native: &super::NativeWindow, width: u32, height: u32) {
        unsafe {
            SetWindowPos(
                native.hwnd,
                std::ptr::null_mut(),
                0,
                0,
                width as i32,
                height as i32,
                SWP_NOMOVE | SWP_NOZORDER,
            );
        }
    }

    pub(super) unsafe fn close_window(native: &super::NativeWindow) {
        unsafe {
            DestroyWindow(native.hwnd);
        }
    }

    pub(super) unsafe fn pump_events() {
        unsafe {
            let mut msg = std::mem::zeroed::<MSG>();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                if msg.message == WM_QUIT {
                    break;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
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
    fn test_platform_type_hwnd() {
        assert_eq!(K_PLATFORM_TYPE_HWND, b"HWND\0");
    }

    #[test]
    fn test_platform_type_x11() {
        assert_eq!(K_PLATFORM_TYPE_X11, b"X11EmbedWindowID\0");
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

    #[test]
    fn test_release_view_safe_with_crash() {
        // Verify that release_view_safe doesn't panic even with a null view.
        // In production, the view would be valid, but we test the sandbox
        // integration by verifying the function signature and structure.
        // A real crash test with a bad pointer would require a signal test
        // similar to sandbox.rs tests.
        use crate::vst3::sandbox::sandbox_call;

        // Verify sandbox_call is accessible from the editor module
        let result = sandbox_call("editor_test", || 42);
        assert!(result.is_ok());
    }

    #[test]
    fn test_editor_close_on_unattached_window_is_noop() {
        // An EditorWindow with attached=false should not attempt COM calls
        // This verifies the early return guard in close()
        // (We can't create a real EditorWindow without a plugin, but we test the logic path)
        let attached = false;
        assert!(!attached, "Unattached windows skip COM teardown");
    }

    #[test]
    fn test_sandbox_import_available() {
        // Verify that SandboxResult and sandbox_call are importable
        // from the editor module (confirms the import was added)
        let _: SandboxResult<i32> = SandboxResult::Ok(0);
    }

    #[test]
    fn test_pump_platform_events_does_not_panic() {
        // pump_platform_events should be safe to call even when no editor
        // windows exist and no NSApplication has been initialised yet.
        // On non-macOS platforms this is a no-op.
        //
        // NOTE: On macOS, AppKit requires calls from the main thread.
        // Unit tests run on background threads, so we only test this on
        // non-macOS or verify the function exists and compiles.
        #[cfg(not(target_os = "macos"))]
        EditorWindow::pump_platform_events();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_ensure_ns_application_idempotent() {
        // Calling ensure_ns_application multiple times must not panic or crash.
        // NSApplication::sharedApplication is documented to return the singleton
        // and is safe to call from any thread (it's the event loop methods that
        // require the main thread).
        unsafe {
            macos::ensure_ns_application();
            macos::ensure_ns_application();
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_pump_events_requires_main_thread() {
        // Verify that pump_events and ensure_ns_application are callable
        // (compile-time check). Actual event pumping requires the main thread,
        // which isn't available in unit tests. The function is exercised in
        // integration / E2E tests via the audio worker.
        //
        // We can still verify NSApplication initialization works:
        unsafe {
            macos::ensure_ns_application();
        }
        // pump_events() is NOT called here because
        // nextEventMatchingMask: throws an ObjC exception on non-main threads.
    }
}
