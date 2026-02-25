//! Plugin parameter introspection and management.
//!
//! Queries the VST3 IEditController interface to enumerate plugin parameters,
//! build a host-side parameter registry, and convert between normalized/plain values.

use crate::vst3::com::*;
use std::ffi::c_void;
use tracing::{debug, info, warn};

/// A single plugin parameter with metadata.
#[derive(Debug, Clone)]
pub struct ParameterEntry {
    /// Parameter ID (used in all API calls).
    pub id: u32,
    /// Display title.
    pub title: String,
    /// Short title for compact display.
    #[allow(dead_code)]
    pub short_title: String,
    /// Units label (e.g. "dB", "Hz", "%").
    pub units: String,
    /// Number of discrete steps (0 = continuous).
    #[allow(dead_code)]
    pub step_count: i32,
    /// Default normalized value [0..1].
    pub default_normalized: f64,
    /// Current normalized value [0..1].
    pub current_normalized: f64,
    /// Whether the parameter can be automated.
    pub can_automate: bool,
    /// Whether the parameter is read-only.
    pub is_read_only: bool,
    /// Whether this is a bypass parameter.
    pub is_bypass: bool,
}

/// Parameter registry — holds all discoverable parameters from a plugin's IEditController.
pub struct ParameterRegistry {
    /// The IEditController COM pointer (if available).
    controller: *mut ComPtr<IEditControllerVtbl>,
    /// Whether we own (and should release) the controller.
    owns_controller: bool,
    /// All enumerated parameters.
    pub parameters: Vec<ParameterEntry>,
}

// Safety: Like Vst3Instance, access is guarded by Mutex.
unsafe impl Send for ParameterRegistry {}

impl ParameterRegistry {
    /// Create a parameter registry from an IEditController COM pointer.
    ///
    /// # Safety
    /// The `controller` must be a valid IEditController COM pointer.
    /// If `owns` is true, the registry will release it on drop.
    pub unsafe fn from_controller(
        controller: *mut ComPtr<IEditControllerVtbl>,
        owns: bool,
    ) -> Self {
        let mut registry = Self {
            controller,
            owns_controller: owns,
            parameters: Vec::new(),
        };
        registry.enumerate_parameters();
        registry
    }

    /// Enumerate all parameters from the controller.
    fn enumerate_parameters(&mut self) {
        if self.controller.is_null() {
            return;
        }

        unsafe {
            let vtbl = &*(*self.controller).vtbl;
            let count = (vtbl.get_parameter_count)(self.controller as *mut c_void);

            if count <= 0 {
                info!("Plugin has no parameters");
                return;
            }

            debug!(count, "Enumerating plugin parameters");

            for i in 0..count {
                let mut info: ParameterInfo = std::mem::zeroed();
                let result = (vtbl.get_parameter_info)(
                    self.controller as *mut c_void,
                    i,
                    &mut info,
                );

                if result != K_RESULT_OK {
                    warn!(index = i, result, "getParameterInfo failed");
                    continue;
                }

                let title = utf16_to_string(&info.title);
                let short_title = utf16_to_string(&info.short_title);
                let units = utf16_to_string(&info.units);

                // Get current normalized value
                let current = (vtbl.get_param_normalized)(
                    self.controller as *mut c_void,
                    info.id,
                );

                let entry = ParameterEntry {
                    id: info.id,
                    title,
                    short_title,
                    units,
                    step_count: info.step_count,
                    default_normalized: info.default_normalized_value,
                    current_normalized: current,
                    can_automate: (info.flags & K_CAN_AUTOMATE) != 0,
                    is_read_only: (info.flags & K_IS_READ_ONLY) != 0,
                    is_bypass: (info.flags & K_IS_BYPASS) != 0,
                };

                debug!(
                    id = entry.id,
                    title = %entry.title,
                    units = %entry.units,
                    default = %format!("{:.3}", entry.default_normalized),
                    current = %format!("{:.3}", entry.current_normalized),
                    "Parameter discovered"
                );

                self.parameters.push(entry);
            }

            info!(count = self.parameters.len(), "Parameters enumerated");
        }
    }

    /// Get a parameter by ID.
    #[allow(dead_code)]
    pub fn get(&self, id: u32) -> Option<&ParameterEntry> {
        self.parameters.iter().find(|p| p.id == id)
    }

    /// Get a parameter by name (case-insensitive search).
    #[allow(dead_code)]
    pub fn find_by_name(&self, name: &str) -> Option<&ParameterEntry> {
        let name_lower = name.to_lowercase();
        self.parameters
            .iter()
            .find(|p| p.title.to_lowercase().contains(&name_lower))
    }

    /// Set a parameter's normalized value.
    ///
    /// Returns the value that was actually set (read back from the controller).
    #[allow(dead_code)]
    pub fn set_normalized(&mut self, id: u32, value: f64) -> Result<f64, String> {
        if self.controller.is_null() {
            return Err("No controller available".into());
        }

        unsafe {
            let vtbl = &*(*self.controller).vtbl;
            let clamped = value.clamp(0.0, 1.0);

            let result = (vtbl.set_param_normalized)(
                self.controller as *mut c_void,
                id,
                clamped,
            );

            if result != K_RESULT_OK {
                return Err(format!("setParamNormalized failed (result: {})", result));
            }

            // Read back the actual value
            let actual = (vtbl.get_param_normalized)(
                self.controller as *mut c_void,
                id,
            );

            // Update our local copy
            if let Some(param) = self.parameters.iter_mut().find(|p| p.id == id) {
                param.current_normalized = actual;
            }

            Ok(actual)
        }
    }

    /// Convert a normalized value to a display string via the controller.
    pub fn value_to_string(&self, id: u32, value: f64) -> Option<String> {
        if self.controller.is_null() {
            return None;
        }

        unsafe {
            let vtbl = &*(*self.controller).vtbl;
            let mut buf = [0u16; 128];

            let result = (vtbl.get_param_string_by_value)(
                self.controller as *mut c_void,
                id,
                value,
                buf.as_mut_ptr(),
            );

            if result == K_RESULT_OK {
                Some(utf16_to_string(&buf))
            } else {
                None
            }
        }
    }

    /// Convert a normalized value to a plain (physical) value.
    #[allow(dead_code)]
    pub fn normalized_to_plain(&self, id: u32, normalized: f64) -> f64 {
        if self.controller.is_null() {
            return normalized;
        }

        unsafe {
            let vtbl = &*(*self.controller).vtbl;
            (vtbl.normalized_param_to_plain)(
                self.controller as *mut c_void,
                id,
                normalized,
            )
        }
    }

    /// Convert a plain (physical) value to normalized.
    #[allow(dead_code)]
    pub fn plain_to_normalized(&self, id: u32, plain: f64) -> f64 {
        if self.controller.is_null() {
            return plain;
        }

        unsafe {
            let vtbl = &*(*self.controller).vtbl;
            (vtbl.plain_param_to_normalized)(
                self.controller as *mut c_void,
                id,
                plain,
            )
        }
    }

    /// Number of parameters.
    pub fn count(&self) -> usize {
        self.parameters.len()
    }

    /// Print a formatted table of all parameters.
    pub fn print_table(&self) {
        if self.parameters.is_empty() {
            println!("  (no parameters)");
            return;
        }

        println!(
            "  {:>6}  {:<30}  {:>8}  {:>8}  {:<8}  {}",
            "ID", "Title", "Default", "Current", "Units", "Flags"
        );
        println!("  {:-<6}  {:-<30}  {:->8}  {:->8}  {:-<8}  {:-<10}", "", "", "", "", "", "");

        for param in &self.parameters {
            let flags = format!(
                "{}{}{}",
                if param.can_automate { "A" } else { "" },
                if param.is_read_only { "R" } else { "" },
                if param.is_bypass { "B" } else { "" },
            );

            let default_str = self
                .value_to_string(param.id, param.default_normalized)
                .unwrap_or_else(|| format!("{:.3}", param.default_normalized));

            let current_str = self
                .value_to_string(param.id, param.current_normalized)
                .unwrap_or_else(|| format!("{:.3}", param.current_normalized));

            println!(
                "  {:>6}  {:<30}  {:>8}  {:>8}  {:<8}  {}",
                param.id,
                truncate(&param.title, 30),
                truncate(&default_str, 8),
                truncate(&current_str, 8),
                truncate(&param.units, 8),
                flags,
            );
        }
    }
}

impl Drop for ParameterRegistry {
    fn drop(&mut self) {
        if self.owns_controller && !self.controller.is_null() {
            unsafe {
                let vtbl = &*(*self.controller).vtbl;
                (vtbl.terminate)(self.controller as *mut c_void);
                (vtbl.release)(self.controller as *mut c_void);
            }
            debug!("IEditController released");
        }
    }
}

/// Convert a null-terminated UTF-16 buffer to a Rust String.
fn utf16_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// Truncate a string to a maximum display width.
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max - 1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_utf16_to_string() {
        let buf: [u16; 128] = {
            let mut b = [0u16; 128];
            let text: Vec<u16> = "Gain".encode_utf16().collect();
            b[..text.len()].copy_from_slice(&text);
            b
        };
        assert_eq!(utf16_to_string(&buf), "Gain");
    }

    #[test]
    fn test_utf16_to_string_empty() {
        let buf = [0u16; 128];
        assert_eq!(utf16_to_string(&buf), "");
    }

    #[test]
    fn test_truncate_short() {
        assert_eq!(truncate("Hello", 10), "Hello");
    }

    #[test]
    fn test_truncate_long() {
        assert_eq!(truncate("Hello World!", 8), "Hello W…");
    }

    #[test]
    fn test_parameter_entry_defaults() {
        let entry = ParameterEntry {
            id: 0,
            title: "Test".into(),
            short_title: "T".into(),
            units: "dB".into(),
            step_count: 0,
            default_normalized: 0.5,
            current_normalized: 0.5,
            can_automate: true,
            is_read_only: false,
            is_bypass: false,
        };
        assert_eq!(entry.id, 0);
        assert!(entry.can_automate);
        assert!(!entry.is_bypass);
    }

    #[test]
    fn test_parameter_flags() {
        let entry = ParameterEntry {
            id: 1,
            title: "Bypass".into(),
            short_title: "Byp".into(),
            units: "".into(),
            step_count: 1,
            default_normalized: 0.0,
            current_normalized: 0.0,
            can_automate: true,
            is_read_only: false,
            is_bypass: true,
        };
        assert!(entry.is_bypass);
        assert!(entry.can_automate);
    }

    #[test]
    fn test_truncate_exact_length() {
        assert_eq!(truncate("12345", 5), "12345");
    }

    #[test]
    fn test_truncate_empty() {
        assert_eq!(truncate("", 10), "");
    }

    #[test]
    fn test_truncate_single_char_max() {
        // max=1 edge case: can't fit anything meaningful
        assert_eq!(truncate("AB", 1), "…");
    }

    #[test]
    fn test_utf16_to_string_no_null() {
        // Buffer with no null terminator — should use full length
        let buf: [u16; 4] = [0x48, 0x65, 0x6C, 0x6C]; // "Hell"
        assert_eq!(utf16_to_string(&buf), "Hell");
    }

    #[test]
    fn test_utf16_to_string_unicode() {
        // "Café" in UTF-16
        let buf: [u16; 8] = [0x43, 0x61, 0x66, 0xE9, 0, 0, 0, 0];
        assert_eq!(utf16_to_string(&buf), "Café");
    }

    #[test]
    fn test_parameter_entry_read_only() {
        let entry = ParameterEntry {
            id: 10,
            title: "ReadOnly".into(),
            short_title: "RO".into(),
            units: "".into(),
            step_count: 0,
            default_normalized: 0.5,
            current_normalized: 0.5,
            can_automate: false,
            is_read_only: true,
            is_bypass: false,
        };
        assert!(entry.is_read_only);
        assert!(!entry.can_automate);
        assert!(!entry.is_bypass);
    }

    #[test]
    fn test_parameter_entry_all_flags_set() {
        let entry = ParameterEntry {
            id: 99,
            title: "All".into(),
            short_title: "A".into(),
            units: "dB".into(),
            step_count: 10,
            default_normalized: 0.0,
            current_normalized: 1.0,
            can_automate: true,
            is_read_only: true,
            is_bypass: true,
        };
        assert!(entry.can_automate);
        assert!(entry.is_read_only);
        assert!(entry.is_bypass);
    }

    #[test]
    fn test_parameter_entry_debug_format() {
        let entry = ParameterEntry {
            id: 5,
            title: "Volume".into(),
            short_title: "Vol".into(),
            units: "dB".into(),
            step_count: 0,
            default_normalized: 0.7,
            current_normalized: 0.3,
            can_automate: true,
            is_read_only: false,
            is_bypass: false,
        };
        let debug = format!("{:?}", entry);
        assert!(debug.contains("Volume"));
        assert!(debug.contains("dB"));
    }
}
