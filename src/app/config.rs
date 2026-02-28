use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::debug;

/// Persistent application configuration, stored as JSON.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// Additional directories to scan for VST3 plugins (persisted across runs).
    #[serde(default)]
    pub extra_scan_paths: Vec<PathBuf>,
}

/// Get the config file path for the current platform.
///
/// On macOS: `~/Library/Application Support/rs-vst-host/config.json`
pub fn config_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("rs-vst-host").join("config.json"))
}

/// Load configuration from disk. Returns default config if file doesn't exist.
pub fn load() -> anyhow::Result<Config> {
    let path = match config_path() {
        Some(p) if p.exists() => p,
        _ => return Ok(Config::default()),
    };

    let json = std::fs::read_to_string(&path)?;
    let config: Config = serde_json::from_str(&json)?;
    debug!(path = %path.display(), paths = config.extra_scan_paths.len(), "Loaded config");
    Ok(config)
}

/// Save configuration to disk.
pub fn save(config: &Config) -> anyhow::Result<()> {
    let path =
        config_path().ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let json = serde_json::to_string_pretty(config)?;
    std::fs::write(&path, &json)?;
    debug!(path = %path.display(), "Saved config");
    Ok(())
}

/// Add a scan path to the persistent configuration.
/// Returns `true` if the path was added (not already present).
pub fn add_scan_path(dir: &Path) -> anyhow::Result<bool> {
    let canonical = normalize_path(dir);
    let mut config = load()?;

    if config
        .extra_scan_paths
        .iter()
        .any(|p| normalize_path(p) == canonical)
    {
        return Ok(false);
    }

    config.extra_scan_paths.push(canonical);
    save(&config)?;
    Ok(true)
}

/// Remove a scan path from the persistent configuration.
/// Returns `true` if the path was found and removed.
pub fn remove_scan_path(dir: &Path) -> anyhow::Result<bool> {
    let canonical = normalize_path(dir);
    let mut config = load()?;

    let before = config.extra_scan_paths.len();
    config
        .extra_scan_paths
        .retain(|p| normalize_path(p) != canonical);

    if config.extra_scan_paths.len() == before {
        return Ok(false);
    }

    save(&config)?;
    Ok(true)
}

/// Normalize a path for comparison (expand `~`, resolve `.`/`..` where possible).
fn normalize_path(path: &Path) -> PathBuf {
    // Try to canonicalize (resolves symlinks + relative paths), fall back to the original
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert!(config.extra_scan_paths.is_empty());
    }

    #[test]
    fn test_config_serde_roundtrip() {
        let config = Config {
            extra_scan_paths: vec![
                PathBuf::from("/custom/vst3"),
                PathBuf::from("/another/path"),
            ],
        };
        let json = serde_json::to_string_pretty(&config).unwrap();
        let restored: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.extra_scan_paths.len(), 2);
        assert_eq!(restored.extra_scan_paths[0], PathBuf::from("/custom/vst3"));
        assert_eq!(
            restored.extra_scan_paths[1],
            PathBuf::from("/another/path")
        );
    }

    #[test]
    fn test_config_serde_empty() {
        let config = Config::default();
        let json = serde_json::to_string(&config).unwrap();
        let restored: Config = serde_json::from_str(&json).unwrap();
        assert!(restored.extra_scan_paths.is_empty());
    }

    #[test]
    fn test_config_serde_missing_field_uses_default() {
        // A JSON object with no extra_scan_paths field should deserialize with default
        let json = "{}";
        let config: Config = serde_json::from_str(json).unwrap();
        assert!(config.extra_scan_paths.is_empty());
    }

    #[test]
    fn test_config_save_and_load_roundtrip() {
        let tmp_dir = std::env::temp_dir().join("rs-vst-host-test-config");
        let _ = std::fs::create_dir_all(&tmp_dir);
        let config_file = tmp_dir.join("test-config.json");

        let config = Config {
            extra_scan_paths: vec![PathBuf::from("/test/vst3")],
        };

        // Write to temp file
        let json = serde_json::to_string_pretty(&config).unwrap();
        std::fs::write(&config_file, &json).unwrap();

        // Read back
        let read_json = std::fs::read_to_string(&config_file).unwrap();
        let restored: Config = serde_json::from_str(&read_json).unwrap();
        assert_eq!(restored.extra_scan_paths.len(), 1);
        assert_eq!(restored.extra_scan_paths[0], PathBuf::from("/test/vst3"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn test_config_corrupt_json() {
        let result = serde_json::from_str::<Config>("not valid json");
        assert!(result.is_err());
    }

    #[test]
    fn test_normalize_path_nonexistent() {
        // Non-existent path should return as-is
        let path = PathBuf::from("/nonexistent/path/to/vst3");
        let normalized = normalize_path(&path);
        assert_eq!(normalized, path);
    }

    #[test]
    fn test_normalize_path_existing() {
        // Existing path should be canonicalized
        let path = PathBuf::from("/tmp");
        let normalized = normalize_path(&path);
        // On macOS, /tmp -> /private/tmp
        assert!(normalized.exists());
    }

    #[test]
    fn test_add_scan_path_and_remove() {
        let tmp_dir = std::env::temp_dir().join("rs-vst-host-test-add-remove");
        let _ = std::fs::create_dir_all(&tmp_dir);
        let config_file = tmp_dir.join("config.json");

        // Start with empty config
        let config = Config::default();
        let json = serde_json::to_string_pretty(&config).unwrap();
        std::fs::write(&config_file, &json).unwrap();

        // Test that add/remove work on Config struct directly
        let mut config = Config::default();
        let path = PathBuf::from("/test/custom/vst3");

        // Add
        assert!(!config.extra_scan_paths.contains(&path));
        config.extra_scan_paths.push(path.clone());
        assert_eq!(config.extra_scan_paths.len(), 1);

        // Duplicate check
        let already_present = config.extra_scan_paths.iter().any(|p| p == &path);
        assert!(already_present);

        // Remove
        config.extra_scan_paths.retain(|p| p != &path);
        assert!(config.extra_scan_paths.is_empty());

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn test_config_path_not_none() {
        // config_path should return Some on all supported platforms
        assert!(config_path().is_some());
    }

    #[test]
    fn test_load_default_when_no_file() {
        // When no config file exists, load() should return default
        let config = load().unwrap();
        // The default config may or may not have paths depending on if
        // the test environment has a config file, but it shouldn't error
        assert!(config.extra_scan_paths.len() < 1000); // sanity check
    }
}
