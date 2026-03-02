//! Preset file management for VST3 plugins.
//!
//! Supports saving and loading user presets as JSON files containing
//! the plugin's component and controller state blobs along with metadata.
//! Presets are stored in `~/.rs-vst-host/presets/<plugin-name>/`.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ── Preset file format ──────────────────────────────────────────────────

/// A user preset file containing plugin state and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    /// Display name of the preset.
    pub name: String,
    /// Class ID of the plugin this preset belongs to.
    pub plugin_cid: [u8; 16],
    /// Component state blob (binary), from `IComponent::getState()`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_optional_base64",
        deserialize_with = "deserialize_optional_base64"
    )]
    pub component_state: Option<Vec<u8>>,
    /// Controller state blob (binary), from `IEditController::getState()`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_optional_base64",
        deserialize_with = "deserialize_optional_base64"
    )]
    pub controller_state: Option<Vec<u8>>,
}

fn serialize_optional_base64<S: serde::Serializer>(
    data: &Option<Vec<u8>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match data {
        Some(bytes) => serializer.serialize_some(&BASE64.encode(bytes)),
        None => serializer.serialize_none(),
    }
}

fn deserialize_optional_base64<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec<u8>>, D::Error> {
    let opt: Option<String> = Option::deserialize(deserializer)?;
    match opt {
        Some(s) => BASE64
            .decode(&s)
            .map(Some)
            .map_err(serde::de::Error::custom),
        None => Ok(None),
    }
}

impl Preset {
    /// Save this preset to a JSON file.
    pub fn save_to_file(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, &json)?;
        tracing::info!(path = %path.display(), name = %self.name, "Preset saved");
        Ok(())
    }

    /// Load a preset from a JSON file.
    pub fn load_from_file(path: &Path) -> anyhow::Result<Self> {
        let json = std::fs::read_to_string(path)?;
        let preset: Preset = serde_json::from_str(&json)?;
        tracing::info!(path = %path.display(), name = %preset.name, "Preset loaded");
        Ok(preset)
    }
}

// ── Preset directory management ─────────────────────────────────────────

/// Get the user presets directory for a specific plugin.
///
/// Returns `~/.rs-vst-host/presets/<sanitized-plugin-name>/`
pub fn presets_dir(plugin_name: &str) -> Option<PathBuf> {
    dirs::data_dir().map(|d| {
        d.join("rs-vst-host")
            .join("presets")
            .join(sanitize_filename(plugin_name))
    })
}

/// Get the root presets directory.
///
/// Returns `~/.rs-vst-host/presets/`
pub fn presets_root_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("rs-vst-host").join("presets"))
}

/// List all user presets for a given plugin.
///
/// Returns a list of `(preset_name, preset_path)` pairs, sorted by name.
pub fn list_user_presets(plugin_name: &str) -> Vec<(String, PathBuf)> {
    let dir = match presets_dir(plugin_name) {
        Some(d) => d,
        None => return Vec::new(),
    };

    if !dir.exists() {
        return Vec::new();
    }

    let mut presets = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                // Try to read just the name field without parsing the full state
                if let Ok(data) = std::fs::read_to_string(&path)
                    && let Ok(preset) = serde_json::from_str::<Preset>(&data)
                {
                    presets.push((preset.name, path));
                }
            }
        }
    }

    presets.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    presets
}

/// Sanitize a preset name for use as a filename.
///
/// Replaces filesystem-unsafe characters with underscores.
/// This is the public API for generating preset file names.
pub fn sanitize_preset_name(name: &str) -> String {
    sanitize_filename(name)
}

/// Sanitize a plugin name for use as a directory name.
///
/// Replaces filesystem-unsafe characters with underscores.
pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect()
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preset_serde_roundtrip() {
        let preset = Preset {
            name: "My Preset".to_string(),
            plugin_cid: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
            component_state: Some(vec![0xDE, 0xAD, 0xBE, 0xEF]),
            controller_state: Some(vec![0xCA, 0xFE]),
        };

        let json = serde_json::to_string_pretty(&preset).unwrap();
        let restored: Preset = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.name, "My Preset");
        assert_eq!(restored.plugin_cid, preset.plugin_cid);
        assert_eq!(restored.component_state, Some(vec![0xDE, 0xAD, 0xBE, 0xEF]));
        assert_eq!(restored.controller_state, Some(vec![0xCA, 0xFE]));
    }

    #[test]
    fn test_preset_serde_no_state() {
        let preset = Preset {
            name: "Empty".to_string(),
            plugin_cid: [0u8; 16],
            component_state: None,
            controller_state: None,
        };

        let json = serde_json::to_string(&preset).unwrap();
        assert!(!json.contains("component_state"));
        assert!(!json.contains("controller_state"));

        let restored: Preset = serde_json::from_str(&json).unwrap();
        assert!(restored.component_state.is_none());
        assert!(restored.controller_state.is_none());
    }

    #[test]
    fn test_preset_base64_encoding() {
        let preset = Preset {
            name: "B64Test".to_string(),
            plugin_cid: [0u8; 16],
            component_state: Some(vec![1, 2, 3, 4, 5]),
            controller_state: None,
        };

        let json = serde_json::to_string_pretty(&preset).unwrap();
        // Base64 of [1,2,3,4,5] is "AQIDBAU="
        assert!(
            json.contains("AQIDBAU="),
            "JSON should contain base64: {}",
            json
        );

        let restored: Preset = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.component_state, Some(vec![1, 2, 3, 4, 5]));
    }

    #[test]
    fn test_preset_file_roundtrip() {
        let dir = std::env::temp_dir().join("rs-vst-host-test-presets");
        let path = dir.join("test_preset.json");

        let preset = Preset {
            name: "File Test".to_string(),
            plugin_cid: [42u8; 16],
            component_state: Some(vec![10, 20, 30]),
            controller_state: Some(vec![40, 50]),
        };

        preset.save_to_file(&path).unwrap();
        assert!(path.exists());

        let loaded = Preset::load_from_file(&path).unwrap();
        assert_eq!(loaded.name, "File Test");
        assert_eq!(loaded.plugin_cid, [42u8; 16]);
        assert_eq!(loaded.component_state, Some(vec![10, 20, 30]));
        assert_eq!(loaded.controller_state, Some(vec![40, 50]));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_preset_load_invalid_json() {
        let dir = std::env::temp_dir().join("rs-vst-host-test-preset-bad");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.json");
        std::fs::write(&path, "not valid json").unwrap();

        let result = Preset::load_from_file(&path);
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_preset_load_missing_file() {
        let path = PathBuf::from("/nonexistent/preset.json");
        let result = Preset::load_from_file(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("Hello World"), "Hello World");
        assert_eq!(sanitize_filename("Plugin/Name"), "Plugin_Name");
        assert_eq!(sanitize_filename("A:B*C?D"), "A_B_C_D");
        assert_eq!(sanitize_filename("Normal-Name_v2"), "Normal-Name_v2");
    }

    #[test]
    fn test_presets_dir() {
        let dir = presets_dir("TestPlugin");
        if let Some(d) = &dir {
            assert!(d.to_string_lossy().contains("presets"));
            assert!(d.to_string_lossy().contains("TestPlugin"));
        }
    }

    #[test]
    fn test_list_user_presets_empty() {
        let presets = list_user_presets("NonExistentPlugin12345");
        assert!(presets.is_empty());
    }

    #[test]
    fn test_list_user_presets_with_files() {
        let plugin_name = "TestListPresets";
        let dir = presets_dir(plugin_name).unwrap();
        std::fs::create_dir_all(&dir).unwrap();

        // Create two preset files
        let p1 = Preset {
            name: "Bright".to_string(),
            plugin_cid: [0u8; 16],
            component_state: Some(vec![1]),
            controller_state: None,
        };
        let p2 = Preset {
            name: "Ambient".to_string(),
            plugin_cid: [0u8; 16],
            component_state: Some(vec![2]),
            controller_state: None,
        };

        p1.save_to_file(&dir.join("bright.json")).unwrap();
        p2.save_to_file(&dir.join("ambient.json")).unwrap();

        let presets = list_user_presets(plugin_name);
        assert_eq!(presets.len(), 2);
        // Should be sorted alphabetically (case-insensitive)
        assert_eq!(presets[0].0, "Ambient");
        assert_eq!(presets[1].0, "Bright");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_preset_backward_compat_no_state_fields() {
        // Simulate a preset file from before state fields were added
        let json = r#"{
            "name": "Old Preset",
            "plugin_cid": [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]
        }"#;
        let preset: Preset = serde_json::from_str(json).unwrap();
        assert_eq!(preset.name, "Old Preset");
        assert!(preset.component_state.is_none());
        assert!(preset.controller_state.is_none());
    }

    #[test]
    fn test_preset_large_state() {
        let large_state = vec![0xABu8; 1_000_000]; // 1 MB state blob
        let preset = Preset {
            name: "Large State".to_string(),
            plugin_cid: [0u8; 16],
            component_state: Some(large_state.clone()),
            controller_state: None,
        };

        let json = serde_json::to_string(&preset).unwrap();
        let restored: Preset = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.component_state.unwrap().len(), 1_000_000);
    }
}
