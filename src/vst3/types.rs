use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Metadata for a single plugin class within a VST3 module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginClassInfo {
    /// Display name of the plugin class.
    pub name: String,
    /// Primary category (e.g., "Audio Module Class").
    pub category: String,
    /// Subcategories (e.g., "Instrument|Synth", "Fx|EQ").
    pub subcategories: Option<String>,
    /// Vendor/manufacturer name.
    pub vendor: Option<String>,
    /// Plugin version string.
    pub version: Option<String>,
    /// SDK version the plugin was built with.
    pub sdk_version: Option<String>,
    /// Unique class identifier (128-bit).
    pub cid: [u8; 16],
}

/// Information about a VST3 module (bundle).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginModuleInfo {
    /// Path to the .vst3 bundle.
    pub path: PathBuf,
    /// Vendor from factory info.
    pub factory_vendor: Option<String>,
    /// URL from factory info.
    pub factory_url: Option<String>,
    /// Email from factory info.
    pub factory_email: Option<String>,
    /// Plugin classes provided by this module.
    pub classes: Vec<PluginClassInfo>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_class_info() -> PluginClassInfo {
        PluginClassInfo {
            name: "TestSynth".into(),
            category: "Audio Module Class".into(),
            subcategories: Some("Instrument|Synth".into()),
            vendor: Some("TestVendor".into()),
            version: Some("1.0.0".into()),
            sdk_version: Some("VST 3.7".into()),
            cid: [
                0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E,
                0x0F, 0x10,
            ],
        }
    }

    fn sample_module_info() -> PluginModuleInfo {
        PluginModuleInfo {
            path: PathBuf::from("/Library/Audio/Plug-Ins/VST3/Test.vst3"),
            factory_vendor: Some("TestVendor".into()),
            factory_url: Some("https://example.com".into()),
            factory_email: Some("test@example.com".into()),
            classes: vec![sample_class_info()],
        }
    }

    #[test]
    fn test_plugin_class_info_serde_roundtrip() {
        let info = sample_class_info();
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: PluginClassInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "TestSynth");
        assert_eq!(deserialized.category, "Audio Module Class");
        assert_eq!(
            deserialized.subcategories.as_deref(),
            Some("Instrument|Synth")
        );
        assert_eq!(deserialized.vendor.as_deref(), Some("TestVendor"));
        assert_eq!(deserialized.version.as_deref(), Some("1.0.0"));
        assert_eq!(deserialized.sdk_version.as_deref(), Some("VST 3.7"));
        assert_eq!(deserialized.cid, info.cid);
    }

    #[test]
    fn test_plugin_module_info_serde_roundtrip() {
        let info = sample_module_info();
        let json = serde_json::to_string_pretty(&info).unwrap();
        let deserialized: PluginModuleInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.path, info.path);
        assert_eq!(deserialized.factory_vendor.as_deref(), Some("TestVendor"));
        assert_eq!(
            deserialized.factory_url.as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            deserialized.factory_email.as_deref(),
            Some("test@example.com")
        );
        assert_eq!(deserialized.classes.len(), 1);
        assert_eq!(deserialized.classes[0].name, "TestSynth");
    }

    #[test]
    fn test_plugin_class_info_optional_fields_none() {
        let info = PluginClassInfo {
            name: "Minimal".into(),
            category: "Audio Module Class".into(),
            subcategories: None,
            vendor: None,
            version: None,
            sdk_version: None,
            cid: [0u8; 16],
        };
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: PluginClassInfo = serde_json::from_str(&json).unwrap();
        assert!(deserialized.subcategories.is_none());
        assert!(deserialized.vendor.is_none());
        assert!(deserialized.version.is_none());
        assert!(deserialized.sdk_version.is_none());
    }

    #[test]
    fn test_cid_serialization() {
        let info = sample_class_info();
        let json = serde_json::to_string(&info).unwrap();
        // CID is [u8; 16] which serde_json serializes as an array of numbers
        assert!(json.contains("[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16]"));
    }

    #[test]
    fn test_plugin_class_info_debug() {
        let info = sample_class_info();
        let debug = format!("{:?}", info);
        assert!(debug.contains("TestSynth"));
        assert!(debug.contains("Audio Module Class"));
    }

    #[test]
    fn test_plugin_module_info_debug() {
        let info = sample_module_info();
        let debug = format!("{:?}", info);
        assert!(debug.contains("Test.vst3"));
        assert!(debug.contains("TestVendor"));
    }

    #[test]
    fn test_plugin_class_info_clone() {
        let info = sample_class_info();
        let cloned = info.clone();
        assert_eq!(cloned.name, info.name);
        assert_eq!(cloned.cid, info.cid);
    }

    #[test]
    fn test_plugin_module_info_clone() {
        let info = sample_module_info();
        let cloned = info.clone();
        assert_eq!(cloned.path, info.path);
        assert_eq!(cloned.classes.len(), info.classes.len());
    }

    #[test]
    fn test_empty_module_info() {
        let info = PluginModuleInfo {
            path: PathBuf::from("/empty.vst3"),
            factory_vendor: None,
            factory_url: None,
            factory_email: None,
            classes: vec![],
        };
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: PluginModuleInfo = serde_json::from_str(&json).unwrap();
        assert!(deserialized.classes.is_empty());
    }

    #[test]
    fn test_multiple_classes_in_module() {
        let info = PluginModuleInfo {
            path: PathBuf::from("/multi.vst3"),
            factory_vendor: Some("Multi".into()),
            factory_url: None,
            factory_email: None,
            classes: vec![
                PluginClassInfo {
                    name: "Synth".into(),
                    category: "Audio Module Class".into(),
                    subcategories: Some("Instrument".into()),
                    vendor: None,
                    version: None,
                    sdk_version: None,
                    cid: [1u8; 16],
                },
                PluginClassInfo {
                    name: "Effect".into(),
                    category: "Audio Module Class".into(),
                    subcategories: Some("Fx".into()),
                    vendor: None,
                    version: None,
                    sdk_version: None,
                    cid: [2u8; 16],
                },
            ],
        };
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: PluginModuleInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.classes.len(), 2);
        assert_eq!(deserialized.classes[0].name, "Synth");
        assert_eq!(deserialized.classes[1].name, "Effect");
    }
}
