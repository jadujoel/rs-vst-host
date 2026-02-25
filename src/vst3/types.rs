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
