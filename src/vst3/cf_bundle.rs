//! CoreFoundation FFI for creating CFBundleRef from a .vst3 bundle path (macOS only).
//!
//! VST3 plugins on macOS require a valid `CFBundleRef` passed to `bundleEntry`
//! so they can locate their resources within the bundle. Without this, many
//! plugins (e.g., FabFilter) fail to provide IAudioProcessor.

use std::ffi::{CString, c_void};
use std::path::Path;
use tracing::debug;

// ─── CoreFoundation type aliases ──────────────────────────────────────────

type CFAllocatorRef = *const c_void;
type CFStringRef = *const c_void;
type CFURLRef = *const c_void;
type CFBundleRef = *mut c_void;
type CFIndex = isize;
type CFStringEncoding = u32;

/// kCFStringEncodingUTF8
const K_CF_STRING_ENCODING_UTF8: CFStringEncoding = 0x0800_0100;

/// kCFURLPOSIXPathStyle
const K_CF_URL_POSIX_PATH_STYLE: CFIndex = 0;

// ─── CoreFoundation extern declarations ───────────────────────────────────

unsafe extern "C" {
    fn CFStringCreateWithCString(
        alloc: CFAllocatorRef,
        c_str: *const i8,
        encoding: CFStringEncoding,
    ) -> CFStringRef;

    fn CFURLCreateWithFileSystemPath(
        alloc: CFAllocatorRef,
        file_path: CFStringRef,
        path_style: CFIndex,
        is_directory: u8,
    ) -> CFURLRef;

    fn CFBundleCreate(alloc: CFAllocatorRef, bundle_url: CFURLRef) -> CFBundleRef;

    fn CFRelease(cf: *const c_void);
}

// ─── Public API ───────────────────────────────────────────────────────────

/// Create a `CFBundleRef` from a filesystem path to a .vst3 bundle.
///
/// Returns a non-null `CFBundleRef` on success, or null on failure.
/// The caller must call [`release`] when done with the bundle ref.
pub fn create(path: &Path) -> *mut c_void {
    let path_str = match path.to_str() {
        Some(s) => s,
        None => {
            debug!(path = %path.display(), "Non-UTF8 bundle path, cannot create CFBundleRef");
            return std::ptr::null_mut();
        }
    };

    let c_str = match CString::new(path_str) {
        Ok(s) => s,
        Err(_) => {
            debug!(path = %path.display(), "Bundle path contains null byte");
            return std::ptr::null_mut();
        }
    };

    unsafe {
        // Create CFString from the path
        let cf_string =
            CFStringCreateWithCString(std::ptr::null(), c_str.as_ptr(), K_CF_STRING_ENCODING_UTF8);
        if cf_string.is_null() {
            debug!("CFStringCreateWithCString returned null");
            return std::ptr::null_mut();
        }

        // Create CFURL from the string path
        let cf_url = CFURLCreateWithFileSystemPath(
            std::ptr::null(),
            cf_string,
            K_CF_URL_POSIX_PATH_STYLE,
            1, // isDirectory = true (VST3 bundles are directories)
        );
        CFRelease(cf_string);

        if cf_url.is_null() {
            debug!("CFURLCreateWithFileSystemPath returned null");
            return std::ptr::null_mut();
        }

        // Create the CFBundle
        let bundle = CFBundleCreate(std::ptr::null(), cf_url);
        CFRelease(cf_url);

        if bundle.is_null() {
            debug!(path = %path.display(), "CFBundleCreate returned null");
        } else {
            debug!(path = %path.display(), "Created CFBundleRef");
        }

        bundle
    }
}

/// Release a `CFBundleRef` previously created by [`create`].
///
/// Safe to call with a null pointer (no-op).
pub fn release(bundle: *mut c_void) {
    if !bundle.is_null() {
        unsafe {
            CFRelease(bundle as *const c_void);
        }
        debug!("Released CFBundleRef");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_nonexistent_path_returns_null() {
        let path = Path::new("/nonexistent/path/Test.vst3");
        let bundle = create(path);
        // Nonexistent path should return null (or a bundle that can't load)
        // On macOS, CFBundleCreate may still succeed for nonexistent paths
        // but we shouldn't crash. Clean up if non-null.
        if !bundle.is_null() {
            release(bundle);
        }
    }

    #[test]
    fn test_release_null_is_noop() {
        // Should not panic or crash
        release(std::ptr::null_mut());
    }

    #[test]
    fn test_create_with_valid_system_framework() {
        // Use a known system framework path to verify the FFI works
        let path = Path::new("/System/Library/Frameworks/CoreFoundation.framework");
        let bundle = create(path);
        // System framework should produce a valid bundle
        assert!(
            !bundle.is_null(),
            "CFBundleCreate should succeed for CoreFoundation.framework"
        );
        release(bundle);
    }
}
