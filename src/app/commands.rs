use crate::vst3::{cache, module::Vst3Module, scanner, types::PluginModuleInfo};
use std::path::PathBuf;
use tracing::{info, warn};

/// Scan VST3 plugin directories, load modules, and cache metadata.
pub fn scan(extra_paths: Vec<PathBuf>) -> anyhow::Result<()> {
    println!("Scanning for VST3 plugins...\n");

    // Build search paths
    let mut search_paths = scanner::default_vst3_paths();
    search_paths.extend(extra_paths);

    println!("Search paths:");
    for p in &search_paths {
        let exists = if p.exists() { "" } else { " (not found)" };
        println!("  {}{}", p.display(), exists);
    }
    println!();

    // Discover bundles on filesystem
    let bundles = scanner::discover_bundles(&search_paths);
    println!("Found {} VST3 bundle(s).\n", bundles.len());

    if bundles.is_empty() {
        println!("No VST3 plugins found.");
        return Ok(());
    }

    // Load each bundle and extract metadata
    let mut modules: Vec<PluginModuleInfo> = Vec::new();

    for bundle_path in &bundles {
        let name = bundle_path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| bundle_path.display().to_string());

        print!("  Loading {}... ", name);
        match Vst3Module::load(bundle_path) {
            Ok(module) => match module.get_info() {
                Ok(info) => {
                    let class_count = info.classes.len();
                    println!("OK ({} class(es))", class_count);
                    for class in &info.classes {
                        let subcats = class
                            .subcategories
                            .as_deref()
                            .map(|s| format!(" | {}", s))
                            .unwrap_or_default();
                        println!("    - {} [{}{}]", class.name, class.category, subcats);
                    }
                    modules.push(info);
                }
                Err(e) => {
                    println!("metadata error: {}", e);
                    warn!(plugin = %name, error = %e, "Failed to get metadata");
                }
            },
            Err(e) => {
                println!("load error: {}", e);
                warn!(plugin = %name, error = %e, "Failed to load module");
            }
        }
    }

    // Save cache
    let scan_cache = cache::ScanCache::new(modules);
    cache::save(&scan_cache)?;

    let total_classes: usize = scan_cache.modules.iter().map(|m| m.classes.len()).sum();
    println!(
        "\nScan complete: {} module(s), {} plugin class(es) cached.",
        scan_cache.modules.len(),
        total_classes
    );

    Ok(())
}

/// List discovered plugins from the cache.
pub fn list() -> anyhow::Result<()> {
    let scan_cache = match cache::load()? {
        Some(c) => c,
        None => {
            println!("No plugin cache found. Run 'scan' first.");
            return Ok(());
        }
    };

    println!("Cached plugins (scanned {}):\n", scan_cache.scan_timestamp);

    let mut index = 1;
    for module in &scan_cache.modules {
        for class in &module.classes {
            let vendor = class
                .vendor
                .as_deref()
                .or(module.factory_vendor.as_deref())
                .unwrap_or("Unknown");
            let subcats = class.subcategories.as_deref().unwrap_or("");

            println!("  {:>3}. {} ({})", index, class.name, vendor);
            if !subcats.is_empty() {
                println!("       Category: {} | {}", class.category, subcats);
            } else {
                println!("       Category: {}", class.category);
            }
            println!("       Path: {}", module.path.display());
            println!();
            index += 1;
        }
    }

    if index == 1 {
        println!("  (no plugins found in cache)");
    }

    Ok(())
}

/// Load and run a plugin (placeholder for Phase 3+).
pub fn run(plugin: &str) -> anyhow::Result<()> {
    println!("Plugin loading and audio processing not yet implemented.");
    println!("Requested plugin: {}", plugin);
    info!(plugin = %plugin, "Run command invoked");

    // TODO: Phase 3+ implementation
    // 1. Look up plugin in cache or load from path
    // 2. Initialize audio device (cpal)
    // 3. Set up VST3 processing (bus arrangement, sample rate, etc.)
    // 4. Run real-time audio loop
    // 5. Clean shutdown

    Ok(())
}
