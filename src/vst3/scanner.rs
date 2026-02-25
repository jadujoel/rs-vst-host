use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// Returns standard VST3 search directories for the current platform.
pub fn default_vst3_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    #[cfg(target_os = "macos")]
    {
        paths.push(PathBuf::from("/Library/Audio/Plug-Ins/VST3"));
        if let Some(home) = dirs::home_dir() {
            paths.push(home.join("Library/Audio/Plug-Ins/VST3"));
        }
    }

    #[cfg(target_os = "linux")]
    {
        paths.push(PathBuf::from("/usr/lib/vst3"));
        paths.push(PathBuf::from("/usr/local/lib/vst3"));
        if let Some(home) = dirs::home_dir() {
            paths.push(home.join(".vst3"));
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(pf) = std::env::var_os("ProgramFiles") {
            paths.push(PathBuf::from(pf).join("Common Files").join("VST3"));
        }
    }

    paths
}

/// Discover all .vst3 bundles in the given directories.
pub fn discover_bundles(search_paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut bundles = Vec::new();

    for dir in search_paths {
        if !dir.exists() {
            debug!(path = %dir.display(), "VST3 search path does not exist");
            continue;
        }

        info!(path = %dir.display(), "Scanning directory");
        match discover_in_directory(dir) {
            Ok(found) => {
                info!(count = found.len(), path = %dir.display(), "Found bundles");
                bundles.extend(found);
            }
            Err(e) => {
                warn!(path = %dir.display(), error = %e, "Error scanning directory");
            }
        }
    }

    bundles.sort();
    bundles.dedup();
    bundles
}

/// Recursively discover .vst3 bundles in a directory.
fn discover_in_directory(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut bundles = Vec::new();

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().is_some_and(|ext| ext == "vst3") {
            // .vst3 bundle (directory on macOS/Linux) or file (Windows)
            bundles.push(path);
        } else if path.is_dir() {
            // Recurse into subdirectories (vendor folders)
            if let Ok(sub) = discover_in_directory(&path) {
                bundles.extend(sub);
            }
        }
    }

    Ok(bundles)
}

/// Resolve the path to the loadable binary within a .vst3 bundle.
pub fn resolve_bundle_binary(bundle_path: &Path) -> Option<PathBuf> {
    if bundle_path.is_file() {
        // Single-file format (primarily Windows)
        return Some(bundle_path.to_path_buf());
    }

    #[cfg(target_os = "macos")]
    return resolve_macos_binary(bundle_path);

    #[cfg(target_os = "linux")]
    return resolve_linux_binary(bundle_path);

    #[cfg(target_os = "windows")]
    return resolve_windows_binary(bundle_path);

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    None
}

#[cfg(target_os = "macos")]
fn resolve_macos_binary(bundle_path: &Path) -> Option<PathBuf> {
    let macos_dir = bundle_path.join("Contents").join("MacOS");
    if !macos_dir.exists() {
        return None;
    }

    // Try the expected name (bundle stem)
    if let Some(stem) = bundle_path.file_stem() {
        let binary = macos_dir.join(stem);
        if binary.exists() {
            return Some(binary);
        }
    }

    // Fallback: first file in MacOS directory
    std::fs::read_dir(&macos_dir)
        .ok()
        .and_then(|mut entries| {
            entries.find_map(|e| {
                let path = e.ok()?.path();
                path.is_file().then_some(path)
            })
        })
}

#[cfg(target_os = "linux")]
fn resolve_linux_binary(bundle_path: &Path) -> Option<PathBuf> {
    let contents = bundle_path.join("Contents");
    let arch_dir = if cfg!(target_arch = "x86_64") {
        "x86_64-linux"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64-linux"
    } else {
        return None;
    };

    let dir = contents.join(arch_dir);
    if !dir.exists() {
        return None;
    }

    std::fs::read_dir(&dir).ok().and_then(|mut entries| {
        entries.find_map(|e| {
            let path = e.ok()?.path();
            path.extension()
                .is_some_and(|ext| ext == "so")
                .then_some(path)
        })
    })
}

#[cfg(target_os = "windows")]
fn resolve_windows_binary(bundle_path: &Path) -> Option<PathBuf> {
    let contents = bundle_path.join("Contents");
    let arch_dir = if cfg!(target_arch = "x86_64") {
        "x86_64-win"
    } else if cfg!(target_arch = "x86") {
        "x86-win"
    } else if cfg!(target_arch = "aarch64") {
        "arm64-win"
    } else {
        return None;
    };

    let dir = contents.join(arch_dir);
    if !dir.exists() {
        return None;
    }

    std::fs::read_dir(&dir).ok().and_then(|mut entries| {
        entries.find_map(|e| {
            let path = e.ok()?.path();
            path.extension()
                .is_some_and(|ext| ext == "vst3")
                .then_some(path)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_vst3_paths_not_empty() {
        let paths = default_vst3_paths();
        assert!(!paths.is_empty(), "Should return at least one search path");
    }

    #[test]
    fn test_discover_bundles_with_nonexistent_path() {
        let bundles = discover_bundles(&[PathBuf::from("/nonexistent/path/vst3")]);
        assert!(bundles.is_empty());
    }

    #[test]
    fn test_discover_in_empty_dir() {
        let tmp = std::env::temp_dir().join("rs-vst-host-test-scan");
        let _ = std::fs::create_dir_all(&tmp);
        let result = discover_in_directory(&tmp).unwrap();
        assert!(result.is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_resolve_bundle_binary_file() {
        // A plain file should resolve to itself
        let tmp = std::env::temp_dir().join("rs-vst-host-test-resolve.vst3");
        std::fs::write(&tmp, b"fake").unwrap();
        assert_eq!(resolve_bundle_binary(&tmp), Some(tmp.clone()));
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_discover_bundles_dedup() {
        let tmp = std::env::temp_dir().join("rs-vst-host-test-dedup");
        let _ = std::fs::create_dir_all(&tmp);

        // Create a fake .vst3 bundle
        let bundle = tmp.join("Test.vst3");
        let _ = std::fs::create_dir_all(&bundle);

        // Pass same directory twice — result should be deduped
        let bundles = discover_bundles(&[tmp.clone(), tmp.clone()]);
        let matching: Vec<_> = bundles
            .iter()
            .filter(|b| b.file_name().is_some_and(|n| n == "Test.vst3"))
            .collect();
        assert_eq!(matching.len(), 1, "Duplicate bundles should be deduped");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_discover_bundles_sorted() {
        let tmp = std::env::temp_dir().join("rs-vst-host-test-sorted");
        let _ = std::fs::create_dir_all(&tmp);

        let _ = std::fs::create_dir_all(tmp.join("Zebra.vst3"));
        let _ = std::fs::create_dir_all(tmp.join("Alpha.vst3"));
        let _ = std::fs::create_dir_all(tmp.join("Middle.vst3"));

        let bundles = discover_bundles(&[tmp.clone()]);
        let names: Vec<_> = bundles
            .iter()
            .filter_map(|b| b.file_name().map(|n| n.to_string_lossy().to_string()))
            .collect();

        // Verify they're sorted
        let mut sorted_names = names.clone();
        sorted_names.sort();
        assert_eq!(names, sorted_names, "Bundles should be sorted");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_discover_in_directory_recursive() {
        let tmp = std::env::temp_dir().join("rs-vst-host-test-recursive");
        let _ = std::fs::create_dir_all(&tmp);

        // Create a nested vendor folder structure
        let vendor_dir = tmp.join("VendorA");
        let _ = std::fs::create_dir_all(&vendor_dir);
        let _ = std::fs::create_dir_all(vendor_dir.join("Deep.vst3"));

        let bundles = discover_in_directory(&tmp).unwrap();
        let found = bundles
            .iter()
            .any(|b| b.file_name().is_some_and(|n| n == "Deep.vst3"));
        assert!(found, "Should find .vst3 in nested vendor directory");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_discover_ignores_non_vst3() {
        let tmp = std::env::temp_dir().join("rs-vst-host-test-nonvst");
        let _ = std::fs::create_dir_all(&tmp);

        // Create non-vst3 items
        let _ = std::fs::create_dir_all(tmp.join("NotPlugin.component"));
        let _ = std::fs::write(tmp.join("readme.txt"), b"hello");
        let _ = std::fs::create_dir_all(tmp.join("Real.vst3"));

        let bundles = discover_in_directory(&tmp).unwrap();
        assert_eq!(bundles.len(), 1);
        assert!(bundles[0].file_name().unwrap() == "Real.vst3");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_resolve_bundle_binary_nonexistent_dir() {
        // A non-existent directory-style bundle should return None
        let path = PathBuf::from("/nonexistent/FakePlugin.vst3");
        assert_eq!(resolve_bundle_binary(&path), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_resolve_macos_bundle_structure() {
        let tmp = std::env::temp_dir().join("rs-vst-host-test-macos-bundle");
        let macos_dir = tmp.join("Contents").join("MacOS");
        let _ = std::fs::create_dir_all(&macos_dir);

        // Create a binary with the "stem" name: the bundle is "rs-vst-host-test-macos-bundle"
        let stem = tmp
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let binary_path = macos_dir.join(&stem);
        std::fs::write(&binary_path, b"fake binary").unwrap();

        let result = resolve_bundle_binary(&tmp);
        assert_eq!(result, Some(binary_path));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
