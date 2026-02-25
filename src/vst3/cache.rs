use crate::vst3::types::PluginModuleInfo;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::debug;

/// Complete scan result, persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanCache {
    /// Timestamp of the scan.
    pub scan_timestamp: String,
    /// All discovered modules with their class metadata.
    pub modules: Vec<PluginModuleInfo>,
}

impl ScanCache {
    /// Create a new scan cache with the current timestamp.
    pub fn new(modules: Vec<PluginModuleInfo>) -> Self {
        Self {
            scan_timestamp: timestamp_now(),
            modules,
        }
    }
}

/// Get the cache file path for the current platform.
fn cache_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("rs-vst-host").join("plugin-cache.json"))
}

/// Save scan results to cache file.
pub fn save(cache: &ScanCache) -> anyhow::Result<()> {
    let path =
        cache_path().ok_or_else(|| anyhow::anyhow!("Could not determine cache directory"))?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let json = serde_json::to_string_pretty(cache)?;
    std::fs::write(&path, &json)?;
    debug!(path = %path.display(), "Saved plugin cache");
    Ok(())
}

/// Load cached scan results if available.
pub fn load() -> anyhow::Result<Option<ScanCache>> {
    let path = match cache_path() {
        Some(p) if p.exists() => p,
        _ => return Ok(None),
    };

    let json = std::fs::read_to_string(&path)?;
    let cache: ScanCache = serde_json::from_str(&json)?;
    debug!(path = %path.display(), modules = cache.modules.len(), "Loaded plugin cache");
    Ok(Some(cache))
}

/// Simple UTC timestamp without external crate.
fn timestamp_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let (year, month, day) = epoch_days_to_date(days);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

/// Convert days since Unix epoch (1970-01-01) to (year, month, day).
fn epoch_days_to_date(mut days: u64) -> (u64, u64, u64) {
    let mut year = 1970;

    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let months: [u64; 12] = if is_leap(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 1;
    for &d in &months {
        if days < d {
            break;
        }
        days -= d;
        month += 1;
    }

    (year, month, days + 1)
}

fn is_leap(y: u64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_epoch_days_to_date_epoch() {
        assert_eq!(epoch_days_to_date(0), (1970, 1, 1));
    }

    #[test]
    fn test_epoch_days_to_date_known() {
        // 2024-01-01 is 19723 days after epoch
        assert_eq!(epoch_days_to_date(19723), (2024, 1, 1));
    }

    #[test]
    fn test_is_leap() {
        assert!(is_leap(2000));
        assert!(is_leap(2024));
        assert!(!is_leap(1900));
        assert!(!is_leap(2023));
    }

    #[test]
    fn test_scan_cache_new() {
        let cache = ScanCache::new(vec![]);
        assert!(cache.modules.is_empty());
        assert!(!cache.scan_timestamp.is_empty());
    }
}
