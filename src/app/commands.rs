use crate::app::config;
use crate::app::interactive::{self, InteractiveState};
use crate::audio::device::{AudioConfig, AudioDevice};
use crate::audio::engine::AudioEngine;
use crate::midi::device::MidiDevice;
use crate::vst3::com::K_SPEAKER_STEREO;
use crate::vst3::{cache, module::Vst3Module, scanner, types::PluginModuleInfo};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tracing::{info, warn};

/// Scan VST3 plugin directories, load modules, and cache metadata.
pub fn scan(extra_paths: Vec<PathBuf>) -> anyhow::Result<()> {
    println!("Scanning for VST3 plugins...\n");

    // Build search paths: defaults + persistent config + CLI one-time paths
    let mut search_paths = scanner::default_vst3_paths();

    // Load persistent extra paths from config
    match config::load() {
        Ok(cfg) => {
            if !cfg.extra_scan_paths.is_empty() {
                println!("Persistent scan paths (from config):");
                for p in &cfg.extra_scan_paths {
                    println!("  + {}", p.display());
                }
                println!();
                search_paths.extend(cfg.extra_scan_paths);
            }
        }
        Err(e) => {
            warn!(error = %e, "Failed to load config, skipping persistent paths");
        }
    }

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

/// List available audio output devices.
pub fn devices() -> anyhow::Result<()> {
    let audio = AudioDevice::new();
    let devices = audio.list_output_devices();

    if devices.is_empty() {
        println!("No audio output devices found.");
        return Ok(());
    }

    println!("Audio output devices:\n");
    for (i, dev) in devices.iter().enumerate() {
        let default_marker = if dev.is_default { " (default)" } else { "" };
        println!("  {:>3}. {}{}", i + 1, dev.name, default_marker);
    }
    println!();

    Ok(())
}

/// List available MIDI input ports.
pub fn midi_ports() -> anyhow::Result<()> {
    let device = MidiDevice::new().map_err(|e| anyhow::anyhow!(e))?;
    let ports = device.list_input_ports();

    if ports.is_empty() {
        println!("No MIDI input ports found.");
        return Ok(());
    }

    println!("MIDI input ports:\n");
    for port in &ports {
        println!("  {:>3}. {}", port.index + 1, port.name);
    }
    println!();

    Ok(())
}

/// Load and run a plugin with real-time audio processing.
pub fn run(
    plugin: &str,
    device_name: Option<&str>,
    midi_port: Option<&str>,
    sample_rate: Option<u32>,
    buffer_size: Option<u32>,
    no_tone: bool,
    list_params: bool,
) -> anyhow::Result<()> {
    // 1. Look up plugin in cache or load from path
    let (module, class_name, cid) = resolve_plugin(plugin)?;

    println!("Loading plugin: {}", class_name);
    info!(plugin = %class_name, path = %module.bundle_path().display(), "Loading plugin");

    // 2. Create VST3 component instance
    let mut instance = module.create_instance(&cid, &class_name)?;

    // Verify 32-bit float support
    if !instance.can_process_f32() {
        anyhow::bail!(
            "Plugin '{}' does not support 32-bit float processing",
            class_name
        );
    }

    // 3. Set up audio device
    let audio = AudioDevice::new();
    let device = audio
        .get_output_device(device_name)
        .ok_or_else(|| anyhow::anyhow!("No audio output device available"))?;

    let device_name_str = device.name().unwrap_or_else(|_| "unknown".into());
    println!("Audio device: {}", device_name_str);

    // Get device config
    let default_config = AudioDevice::default_config(&device).map_err(|e| anyhow::anyhow!(e))?;

    let config = AudioConfig {
        sample_rate: sample_rate.unwrap_or(default_config.sample_rate),
        channels: default_config.channels.min(2), // Limit to stereo for now
        buffer_size: buffer_size.unwrap_or(0),
    };

    println!(
        "Audio config: {} Hz, {} ch, buffer: {}",
        config.sample_rate,
        config.channels,
        if config.buffer_size > 0 {
            format!("{} frames", config.buffer_size)
        } else {
            "default".into()
        }
    );

    // 4. Configure plugin processing
    let max_block_size = if config.buffer_size > 0 {
        config.buffer_size as i32
    } else {
        4096 // Reasonable default max
    };

    // Set bus arrangements (stereo)
    instance.set_bus_arrangements(K_SPEAKER_STEREO, K_SPEAKER_STEREO)?;

    // Setup processing
    instance.setup_processing(config.sample_rate as f64, max_block_size)?;

    // Activate
    instance.activate()?;
    instance.start_processing()?;

    let latency = instance.latency_samples();
    if latency > 0 {
        println!("Plugin latency: {} samples", latency);
    }

    // 4b. Install component handler for plugin parameter notifications
    instance.install_component_handler();

    // 4c. Query and display parameters (if requested)
    let params = if list_params {
        if let Some(params) = instance.query_parameters() {
            println!("\nPlugin parameters ({}):\n", params.count());
            params.print_table();
            println!();
            Some(params)
        } else {
            println!("\nPlugin does not expose parameters via IEditController.\n");
            None
        }
    } else {
        instance.query_parameters()
    };

    // 5. Capture component handler pointer before instance is consumed
    let instance_component_handler = instance.component_handler();

    // 5b. Build audio engine
    let mut engine = AudioEngine::new(
        instance,
        config.sample_rate as f64,
        max_block_size as usize,
        config.channels as usize,
    );

    if no_tone {
        engine.tone().enabled = false;
        println!("Test tone: disabled");
    } else {
        println!("Test tone: 440 Hz sine wave");
    }

    // 6. Set up MIDI input (if requested)
    let _midi_connection = if let Some(midi_name) = midi_port {
        match crate::midi::device::open_midi_input(Some(midi_name)) {
            Ok((connection, port_name, receiver)) => {
                engine.set_midi_receiver(receiver);
                println!("MIDI input: {}", port_name);
                Some(connection)
            }
            Err(e) => {
                warn!(error = %e, "Failed to open MIDI input");
                println!("MIDI input: failed ({})", e);
                None
            }
        }
    } else {
        // Try to open default MIDI port if any are available
        None
    };

    // Capture state for interactive mode before wrapping engine
    let param_queue = engine.pending_param_queue();
    let shutdown_flag = engine.shutdown_flag();
    let component_handler = instance_component_handler;

    let engine = Arc::new(Mutex::new(engine));

    // 8. Set up Ctrl+C handler
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::Relaxed);
    })
    .map_err(|e| anyhow::anyhow!("Failed to set Ctrl+C handler: {}", e))?;

    // 9. Start audio stream
    let engine_cb = engine.clone();
    let stream = AudioDevice::build_output_stream(
        &device,
        &config,
        move |data: &mut [f32], _info: &cpal::OutputCallbackInfo| {
            // Check atomic shutdown flag before acquiring the Mutex
            if shutdown_flag.load(Ordering::Acquire) {
                data.fill(0.0);
                return;
            }
            if let Ok(mut eng) = engine_cb.try_lock() {
                eng.process(data);
            } else {
                // Fill silence if we can't obtain the lock
                data.fill(0.0);
            }
        },
        |err| {
            tracing::error!(error = %err, "Audio stream error");
        },
    )
    .map_err(|e| anyhow::anyhow!(e))?;

    AudioDevice::play(&stream).map_err(|e| anyhow::anyhow!(e))?;

    println!("\nProcessing audio. Type 'help' for commands, 'quit' to stop.\n");

    // 10. Run interactive command loop (blocks until quit or Ctrl+C)
    let mut interactive_state = InteractiveState {
        params,
        component_handler,
        param_queue,
        running: running.clone(),
    };
    interactive::run_interactive(&mut interactive_state);

    println!("\nStopping...");

    // 11. Clean shutdown
    // Drop stream first to stop the audio callback
    drop(stream);
    // Brief pause to let any in-flight callbacks complete
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Now safely shut down the engine
    if let Ok(mut eng) = engine.lock() {
        eng.shutdown();
    }

    println!("Done.");
    Ok(())
}

/// Resolve a plugin name or path to a loaded module, class name, and class ID.
fn resolve_plugin(plugin: &str) -> anyhow::Result<(Vst3Module, String, [u8; 16])> {
    // Check if it's a path to a .vst3 bundle
    let path = Path::new(plugin);
    if path.extension().is_some_and(|ext| ext == "vst3") && path.exists() {
        return load_plugin_from_path(path);
    }

    // Look up in cache by name
    let scan_cache = cache::load()?.ok_or_else(|| {
        anyhow::anyhow!("No plugin cache found. Run 'scan' first, or provide a .vst3 path.")
    })?;

    // Search for matching class name (case-insensitive)
    let plugin_lower = plugin.to_lowercase();
    for module_info in &scan_cache.modules {
        for class in &module_info.classes {
            if class.name.to_lowercase() == plugin_lower
                || class.name.to_lowercase().contains(&plugin_lower)
            {
                let module = Vst3Module::load(&module_info.path)?;
                return Ok((module, class.name.clone(), class.cid));
            }
        }
    }

    // Try matching by module path stem
    for module_info in &scan_cache.modules {
        let stem = module_info
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        if stem == plugin_lower || stem.contains(&plugin_lower) {
            if let Some(class) = module_info.classes.first() {
                let module = Vst3Module::load(&module_info.path)?;
                return Ok((module, class.name.clone(), class.cid));
            }
        }
    }

    anyhow::bail!(
        "Plugin '{}' not found. Run 'list' to see available plugins.",
        plugin
    )
}

/// Load a plugin directly from a .vst3 bundle path.
fn load_plugin_from_path(path: &Path) -> anyhow::Result<(Vst3Module, String, [u8; 16])> {
    let module = Vst3Module::load(path)?;
    let info = module.get_info()?;

    // Find the first Audio Module Class or first class
    let class = info
        .classes
        .iter()
        .find(|c| c.category == "Audio Module Class")
        .or(info.classes.first())
        .ok_or_else(|| anyhow::anyhow!("No plugin classes found in {}", path.display()))?;

    let name = class.name.clone();
    let cid = class.cid;

    // Reload the module (we consumed info from the first load)
    let module = Vst3Module::load(path)?;
    Ok((module, name, cid))
}

/// Add a directory to the persistent scan paths.
pub fn scan_paths_add(dir: PathBuf) -> anyhow::Result<()> {
    let display = dir.display().to_string();
    match config::add_scan_path(&dir)? {
        true => {
            println!("Added '{}' to persistent scan paths.", display);
            println!("Run 'scan' to discover plugins in this directory.");
        }
        false => {
            println!("'{}' is already in the persistent scan paths.", display);
        }
    }
    Ok(())
}

/// Remove a directory from the persistent scan paths.
pub fn scan_paths_remove(dir: PathBuf) -> anyhow::Result<()> {
    let display = dir.display().to_string();
    match config::remove_scan_path(&dir)? {
        true => {
            println!("Removed '{}' from persistent scan paths.", display);
            println!("Run 'scan' to update the plugin cache.");
        }
        false => {
            println!("'{}' was not in the persistent scan paths.", display);
        }
    }
    Ok(())
}

/// List all persistent scan paths.
pub fn scan_paths_list() -> anyhow::Result<()> {
    let cfg = config::load()?;

    if cfg.extra_scan_paths.is_empty() {
        println!("No persistent scan paths configured.");
        println!();
        println!("Add one with: rs-vst-host scan-paths add <DIR>");
        return Ok(());
    }

    println!("Persistent scan paths:\n");
    for (i, path) in cfg.extra_scan_paths.iter().enumerate() {
        let exists = if path.exists() { "" } else { " (not found)" };
        println!("  {:>3}. {}{}", i + 1, path.display(), exists);
    }
    println!();

    if let Some(config_path) = config::config_path() {
        println!("Config file: {}", config_path.display());
    }

    Ok(())
}

use cpal::traits::DeviceTrait;
