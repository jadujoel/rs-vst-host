//! Dynamic loading of VST3 plugin modules and COM factory access.
//!
//! Uses manual FFI to the VST3 COM interfaces rather than depending on
//! auto-generated bindings. This gives us full control over the ABI and
//! avoids version coupling with binding crates.

use crate::error::Vst3Error;
use crate::vst3::types::{PluginClassInfo, PluginModuleInfo};
use libloading::{Library, Symbol};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

#[cfg(target_os = "macos")]
use crate::vst3::cf_bundle;

use super::scanner;

// ─── COM vtable definitions for VST3 IPluginFactory ───────────────────────

pub(crate) const K_RESULT_OK: i32 = 0;

/// PFactoryInfo — matches the C struct layout from the VST3 SDK.
#[repr(C)]
pub(crate) struct RawFactoryInfo {
    vendor: [u8; 64],
    url: [u8; 256],
    email: [u8; 128],
    flags: i32,
}

/// PClassInfo — matches the C struct layout from the VST3 SDK.
#[repr(C)]
pub(crate) struct RawClassInfo {
    cid: [u8; 16],
    cardinality: i32,
    category: [u8; 32],
    name: [u8; 64],
}

/// PClassInfo2 — extended class info from IPluginFactory2.
#[repr(C)]
struct RawClassInfo2 {
    cid: [u8; 16],
    cardinality: i32,
    category: [u8; 32],
    name: [u8; 64],
    class_flags: u32,
    subcategories: [u8; 128],
    vendor: [u8; 64],
    version: [u8; 64],
    sdk_version: [u8; 64],
}

/// IUnknown vtable (COM base interface).
#[repr(C)]
pub struct IUnknownVtbl {
    pub query_interface:
        unsafe extern "system" fn(this: *mut c_void, iid: *const u8, obj: *mut *mut c_void) -> i32,
    pub add_ref: unsafe extern "system" fn(this: *mut c_void) -> u32,
    pub release: unsafe extern "system" fn(this: *mut c_void) -> u32,
}

/// IPluginFactory vtable (extends IUnknown).
#[repr(C)]
pub struct IPluginFactoryVtbl {
    pub base: IUnknownVtbl,
    pub get_factory_info:
        unsafe extern "system" fn(this: *mut c_void, info: *mut RawFactoryInfo) -> i32,
    pub count_classes: unsafe extern "system" fn(this: *mut c_void) -> i32,
    pub get_class_info:
        unsafe extern "system" fn(this: *mut c_void, index: i32, info: *mut RawClassInfo) -> i32,
    pub create_instance: unsafe extern "system" fn(
        this: *mut c_void,
        cid: *const u8,
        iid: *const u8,
        obj: *mut *mut c_void,
    ) -> i32,
}

/// IPluginFactory2 vtable (extends IPluginFactory).
#[repr(C)]
struct IPluginFactory2Vtbl {
    base: IPluginFactoryVtbl,
    get_class_info2:
        unsafe extern "system" fn(this: *mut c_void, index: i32, info: *mut RawClassInfo2) -> i32,
}

/// COM interface pointer: pointer to vtable pointer.
#[repr(C)]
pub struct ComObj<V> {
    pub vtbl: *const V,
}

/// IPluginFactory2 IID: {0007B650-F24B-4C0B-A464-EDB9F00B2ABB}
/// Byte encoding uses the non-COM (big-endian per u32) format used on macOS/Linux.
#[cfg(not(target_os = "windows"))]
const IPLUGIN_FACTORY2_IID: [u8; 16] = [
    0x00, 0x07, 0xB6, 0x50, 0xF2, 0x4B, 0x4C, 0x0B, 0xA4, 0x64, 0xED, 0xB9, 0xF0, 0x0B, 0x2A, 0xBB,
];

/// IPluginFactory2 IID in COM-compatible byte order (Windows).
#[cfg(target_os = "windows")]
const IPLUGIN_FACTORY2_IID: [u8; 16] = [
    0x50, 0xB6, 0x07, 0x00, 0x4B, 0xF2, 0x0B, 0x4C, 0xA4, 0x64, 0xED, 0xB9, 0xF0, 0x0B, 0x2A, 0xBB,
];

/// IPluginFactory3 IID: {4555A2AB-C123-4E57-9B12-291036878931}
#[cfg(not(target_os = "windows"))]
const IPLUGIN_FACTORY3_IID: [u8; 16] = [
    0x45, 0x55, 0xA2, 0xAB, 0xC1, 0x23, 0x4E, 0x57, 0x9B, 0x12, 0x29, 0x10, 0x36, 0x87, 0x89, 0x31,
];

/// IPluginFactory3 IID in COM-compatible byte order (Windows).
#[cfg(target_os = "windows")]
const IPLUGIN_FACTORY3_IID: [u8; 16] = [
    0xAB, 0xA2, 0x55, 0x45, 0x23, 0xC1, 0x57, 0x4E, 0x9B, 0x12, 0x29, 0x10, 0x36, 0x87, 0x89, 0x31,
];

/// IPluginFactory3 vtable (extends IPluginFactory2).
#[repr(C)]
struct IPluginFactory3Vtbl {
    base: IPluginFactory2Vtbl,
    /// setHostContext(context: FUnknown*) -> tresult
    set_host_context: unsafe extern "system" fn(this: *mut c_void, context: *mut c_void) -> i32,
}

// ─── Vst3Module ───────────────────────────────────────────────────────────

/// A loaded VST3 module with access to its plugin factory.
pub struct Vst3Module {
    /// Keep the dynamic library alive as long as the module is in use.
    /// Must be dropped AFTER factory release and bundleExit.
    _library: Library,
    /// Raw COM pointer to IPluginFactory.
    factory: *mut ComObj<IPluginFactoryVtbl>,
    /// Path to the .vst3 bundle.
    bundle_path: PathBuf,
    /// Host context for factory (IPluginFactory3::setHostContext).
    host_context: *mut crate::vst3::host_context::HostApplication,
    /// CFBundleRef handle (macOS only). Passed to bundleEntry, released on drop.
    #[cfg(target_os = "macos")]
    cf_bundle_ref: *mut c_void,
}

// Safety: COM pointers are only accessed from the loading thread.
unsafe impl Send for Vst3Module {}

impl Vst3Module {
    /// Load a VST3 module from a bundle path.
    pub fn load(bundle_path: &Path) -> Result<Self, Vst3Error> {
        let binary_path =
            scanner::resolve_bundle_binary(bundle_path).ok_or_else(|| Vst3Error::Bundle {
                path: bundle_path.display().to_string(),
                message: "could not resolve binary path within bundle".into(),
            })?;

        debug!(binary = %binary_path.display(), "Loading VST3 binary");

        let library = unsafe { Library::new(&binary_path) }
            .map_err(|e| Vst3Error::ModuleLoad(format!("{}: {}", binary_path.display(), e)))?;

        // Platform-specific module entry
        #[cfg(target_os = "macos")]
        let cf_bundle_ref = call_bundle_entry(&library, bundle_path);

        #[cfg(target_os = "linux")]
        call_module_entry(&library);

        // Get the plugin factory
        let factory = unsafe {
            let get_factory: Symbol<unsafe extern "C" fn() -> *mut c_void> = library
                .get(b"GetPluginFactory\0")
                .map_err(|e| Vst3Error::EntryPoint(format!("GetPluginFactory: {}", e)))?;

            let raw = get_factory();
            if raw.is_null() {
                return Err(Vst3Error::Factory("GetPluginFactory returned null".into()));
            }
            raw as *mut ComObj<IPluginFactoryVtbl>
        };

        // If the factory supports IPluginFactory3, call setHostContext
        // so the plugin can access host services during createInstance.
        let host_context = crate::vst3::host_context::HostApplication::new();
        let factory3 = Self::query_factory3_raw(factory);
        if let Some(f3) = factory3 {
            unsafe {
                let f3_vtbl = &*(*f3).vtbl;
                let result = (f3_vtbl.set_host_context)(
                    f3 as *mut c_void,
                    crate::vst3::host_context::HostApplication::as_unknown(host_context),
                );
                debug!(result, "IPluginFactory3::setHostContext");
                (f3_vtbl.base.base.base.release)(f3 as *mut c_void);
            }
        }

        Ok(Self {
            _library: library,
            factory,
            bundle_path: bundle_path.to_path_buf(),
            #[cfg(target_os = "macos")]
            cf_bundle_ref,
            host_context,
        })
    }

    /// Query factory for module and class metadata.
    pub fn get_info(&self) -> Result<PluginModuleInfo, Vst3Error> {
        let this = self.factory as *mut c_void;
        let vtbl = unsafe { &*(*self.factory).vtbl };

        // Factory info
        let mut fi: RawFactoryInfo = unsafe { std::mem::zeroed() };
        let (vendor, url, email) = unsafe {
            if (vtbl.get_factory_info)(this, &mut fi) == K_RESULT_OK {
                (
                    Some(bytes_to_string(&fi.vendor)),
                    Some(bytes_to_string(&fi.url)),
                    Some(bytes_to_string(&fi.email)),
                )
            } else {
                (None, None, None)
            }
        };

        // Try to get IPluginFactory2 for extended class info
        let factory2 = self.query_factory2();

        // Enumerate classes
        let count = unsafe { (vtbl.count_classes)(this) };
        let mut classes = Vec::with_capacity(count.max(0) as usize);

        for i in 0..count {
            // Try extended info first (IPluginFactory2)
            if let Some(f2) = factory2 {
                let f2_this = f2 as *mut c_void;
                let f2_vtbl = unsafe { &*(*f2).vtbl };
                let mut ci2: RawClassInfo2 = unsafe { std::mem::zeroed() };
                if unsafe { (f2_vtbl.get_class_info2)(f2_this, i, &mut ci2) } == K_RESULT_OK {
                    classes.push(PluginClassInfo {
                        name: bytes_to_string(&ci2.name),
                        category: bytes_to_string(&ci2.category),
                        subcategories: non_empty(bytes_to_string(&ci2.subcategories)),
                        vendor: non_empty(bytes_to_string(&ci2.vendor)),
                        version: non_empty(bytes_to_string(&ci2.version)),
                        sdk_version: non_empty(bytes_to_string(&ci2.sdk_version)),
                        cid: ci2.cid,
                    });
                    continue;
                }
            }

            // Fallback to basic class info (IPluginFactory)
            let mut ci: RawClassInfo = unsafe { std::mem::zeroed() };
            if unsafe { (vtbl.get_class_info)(this, i, &mut ci) } == K_RESULT_OK {
                classes.push(PluginClassInfo {
                    name: bytes_to_string(&ci.name),
                    category: bytes_to_string(&ci.category),
                    subcategories: None,
                    vendor: vendor.clone(),
                    version: None,
                    sdk_version: None,
                    cid: ci.cid,
                });
            }
        }

        // Release factory2 if we obtained it
        if let Some(f2) = factory2 {
            let f2_vtbl = unsafe { &*(*f2).vtbl };
            unsafe {
                (f2_vtbl.base.base.release)(f2 as *mut c_void);
            }
        }

        Ok(PluginModuleInfo {
            path: self.bundle_path.clone(),
            factory_vendor: vendor,
            factory_url: url,
            factory_email: email,
            classes,
        })
    }

    /// Attempt to QueryInterface for IPluginFactory2 (extended class info).
    fn query_factory2(&self) -> Option<*mut ComObj<IPluginFactory2Vtbl>> {
        let this = self.factory as *mut c_void;
        let vtbl = unsafe { &*(*self.factory).vtbl };
        let mut obj: *mut c_void = std::ptr::null_mut();

        let result =
            unsafe { (vtbl.base.query_interface)(this, IPLUGIN_FACTORY2_IID.as_ptr(), &mut obj) };

        if result == K_RESULT_OK && !obj.is_null() {
            Some(obj as *mut ComObj<IPluginFactory2Vtbl>)
        } else {
            None
        }
    }

    /// Attempt to QueryInterface for IPluginFactory3 (host context support).
    fn query_factory3_raw(
        factory: *mut ComObj<IPluginFactoryVtbl>,
    ) -> Option<*mut ComObj<IPluginFactory3Vtbl>> {
        let this = factory as *mut c_void;
        let vtbl = unsafe { &*(*factory).vtbl };
        let mut obj: *mut c_void = std::ptr::null_mut();

        let result =
            unsafe { (vtbl.base.query_interface)(this, IPLUGIN_FACTORY3_IID.as_ptr(), &mut obj) };

        if result == K_RESULT_OK && !obj.is_null() {
            debug!("Factory supports IPluginFactory3");
            Some(obj as *mut ComObj<IPluginFactory3Vtbl>)
        } else {
            debug!("Factory does not support IPluginFactory3");
            None
        }
    }

    /// Create a VST3 plugin instance from a class ID.
    ///
    /// Instantiates IComponent from the factory and sets up IAudioProcessor.
    /// The returned `Vst3Instance` is ready for `setup_processing` and activation.
    pub fn create_instance(
        &self,
        cid: &[u8; 16],
        name: &str,
    ) -> Result<crate::vst3::instance::Vst3Instance, Vst3Error> {
        let factory = self.factory as *mut c_void;
        let vtbl = unsafe { &*(*self.factory).vtbl };

        unsafe { crate::vst3::instance::Vst3Instance::create(factory, vtbl, cid, name) }
    }

    /// Get the path to the .vst3 bundle.
    pub fn bundle_path(&self) -> &Path {
        &self.bundle_path
    }
}

impl Drop for Vst3Module {
    fn drop(&mut self) {
        // Release the factory COM reference
        let vtbl = unsafe { &*(*self.factory).vtbl };
        unsafe {
            (vtbl.base.release)(self.factory as *mut c_void);
        }

        // Destroy host context
        unsafe {
            crate::vst3::host_context::HostApplication::destroy(self.host_context);
        }

        // Platform-specific module exit (must happen before library unload)
        #[cfg(target_os = "macos")]
        {
            call_bundle_exit(&self._library, self.cf_bundle_ref);
            cf_bundle::release(self.cf_bundle_ref);
        }
    }
}

#[cfg(target_os = "macos")]
fn call_bundle_entry(library: &Library, bundle_path: &Path) -> *mut c_void {
    let bundle_ref = cf_bundle::create(bundle_path);
    if bundle_ref.is_null() {
        warn!(path = %bundle_path.display(), "Failed to create CFBundleRef, falling back to null");
    }

    unsafe {
        if let Ok(entry) =
            library.get::<unsafe extern "C" fn(*mut c_void) -> bool>(b"bundleEntry\0")
        {
            let ok = entry(bundle_ref);
            if !ok {
                warn!("bundleEntry returned false");
            }
        }
    }

    bundle_ref
}

#[cfg(target_os = "macos")]
fn call_bundle_exit(library: &Library, bundle_ref: *mut c_void) {
    unsafe {
        if let Ok(exit) = library.get::<unsafe extern "C" fn(*mut c_void) -> bool>(b"bundleExit\0")
        {
            let ok = exit(bundle_ref);
            if !ok {
                warn!("bundleExit returned false");
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn call_module_entry(library: &Library) {
    unsafe {
        if let Ok(entry) =
            library.get::<unsafe extern "C" fn(*mut c_void) -> bool>(b"ModuleEntry\0")
        {
            let ok = entry(std::ptr::null_mut());
            if !ok {
                warn!("ModuleEntry returned false");
            }
        }
    }
}

/// Convert a null-terminated byte buffer to a Rust `String`.
fn bytes_to_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// Return `None` for empty strings, `Some(s)` otherwise.
fn non_empty(s: String) -> Option<String> {
    if s.is_empty() { None } else { Some(s) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bytes_to_string_basic() {
        let buf = b"Hello\0World";
        assert_eq!(bytes_to_string(buf), "Hello");
    }

    #[test]
    fn test_bytes_to_string_no_null() {
        let buf = b"NoNull";
        assert_eq!(bytes_to_string(buf), "NoNull");
    }

    #[test]
    fn test_bytes_to_string_empty() {
        let buf = b"\0rest";
        assert_eq!(bytes_to_string(buf), "");
    }

    #[test]
    fn test_non_empty() {
        assert_eq!(non_empty(String::new()), None);
        assert_eq!(non_empty("hello".into()), Some("hello".into()));
    }

    /// Convert a UUID string to big-endian byte array.
    fn uuid_to_big_endian(uuid: &str) -> [u8; 16] {
        let hex: String = uuid.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        assert_eq!(hex.len(), 32);
        let mut bytes = [0u8; 16];
        for i in 0..16 {
            bytes[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
        }
        bytes
    }

    #[test]
    fn test_iplugin_factory2_iid_matches_uuid() {
        let expected = uuid_to_big_endian("0007B650-F24B-4C0B-A464-EDB9F00B2ABB");
        #[cfg(not(target_os = "windows"))]
        assert_eq!(
            IPLUGIN_FACTORY2_IID, expected,
            "IPluginFactory2 IID mismatch"
        );
        let _ = expected;
    }

    #[test]
    fn test_iplugin_factory3_iid_matches_uuid() {
        let expected = uuid_to_big_endian("4555A2AB-C123-4E57-9B12-291036878931");
        #[cfg(not(target_os = "windows"))]
        assert_eq!(
            IPLUGIN_FACTORY3_IID, expected,
            "IPluginFactory3 IID mismatch"
        );
        let _ = expected;
    }
}
