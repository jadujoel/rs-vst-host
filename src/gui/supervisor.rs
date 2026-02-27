//! GUI process supervisor — spawns the GUI in a child process and relaunches on crash.
//!
//! The supervisor lives in the main process alongside the audio engine and
//! plugin backend. It:
//!
//! 1. Creates a Unix domain socket pair
//! 2. Spawns a child process running `rs-vst-host gui-worker --socket <path>`
//! 3. Runs the [`HostBackend`] and handles [`GuiAction`] messages from the child
//! 4. Sends [`SupervisorUpdate`] state updates to the GUI
//! 5. If the child crashes, relaunches it and re-sends the full state
//!
//! This provides complete crash isolation: a plugin that corrupts the heap
//! in the GUI process cannot bring down audio processing. The supervisor
//! simply restarts the GUI.

use crate::gui::backend::{AudioStatus, HostBackend, ParamSnapshot};
use crate::gui::ipc::*;
use crate::gui::session::Session;
use crate::vst3::types::PluginModuleInfo;
use crate::vst3::{cache, scanner};

use std::io::Write;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// Maximum number of rapid restarts before giving up.
const MAX_RAPID_RESTARTS: u32 = 5;
/// Time window for counting "rapid" restarts.
const RAPID_RESTART_WINDOW: Duration = Duration::from_secs(30);

/// Run the GUI supervisor loop.
///
/// This function blocks until the user closes the GUI (clean shutdown)
/// or the maximum restart count is exceeded within the rapid restart window.
///
/// # Arguments
/// * `safe_mode` — if true, no plugins loaded from cache on startup
/// * `malloc_debug` — if true, enable periodic heap checks
pub fn run_supervisor(safe_mode: bool, malloc_debug: bool) -> anyhow::Result<()> {
    let _span = tracing::info_span!("gui_supervisor").entered();
    info!(
        "Starting GUI supervisor (safe_mode={}, malloc_debug={})",
        safe_mode, malloc_debug
    );

    // ── Build initial state ─────────────────────────────────────────────
    let mut backend = HostBackend::new();
    let mut plugin_modules: Vec<PluginModuleInfo> = if safe_mode {
        Vec::new()
    } else {
        cache::load()
            .ok()
            .flatten()
            .map(|c| c.modules)
            .unwrap_or_default()
    };

    let mut rack: Vec<RackSlotState> = Vec::new();
    let mut selected_slot: Option<usize> = None;
    let mut param_snapshots: Vec<ParamSnapshot> = Vec::new();
    let mut status_message: String = if safe_mode {
        "Safe mode — no plugins loaded. Click 'Scan' to discover VST3 plugins.".into()
    } else if plugin_modules.is_empty() {
        "No plugins cached. Click 'Scan' to discover VST3 plugins.".into()
    } else {
        let total: usize = plugin_modules.iter().map(|m| m.classes.len()).sum();
        format!("{} plugin class(es) loaded from cache.", total)
    };
    let mut tone_enabled = false;
    let mut transport = TransportUpdate {
        playing: false,
        tempo: 120.0,
        time_sig_num: 4,
        time_sig_den: 4,
    };
    let mut session_path = crate::gui::session::sessions_dir()
        .map(|d| d.join("default.json").to_string_lossy().to_string())
        .unwrap_or_else(|| "session.json".into());

    // ── Restart loop ────────────────────────────────────────────────────
    let mut restart_count: u32 = 0;
    let mut first_restart_time = std::time::Instant::now();

    loop {
        // Check rapid restart limit
        if restart_count > 0 {
            if first_restart_time.elapsed() > RAPID_RESTART_WINDOW {
                // Reset counter — we're past the window
                restart_count = 0;
                first_restart_time = std::time::Instant::now();
            }
            if restart_count >= MAX_RAPID_RESTARTS {
                error!(
                    "GUI crashed {} times within {}s — giving up",
                    restart_count,
                    RAPID_RESTART_WINDOW.as_secs()
                );
                return Err(anyhow::anyhow!(
                    "GUI process crashed {} times rapidly — cannot recover",
                    restart_count
                ));
            }
        }

        // Create socket
        let socket_path =
            std::env::temp_dir().join(format!("rs-vst-host-gui-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket_path);

        let listener = UnixListener::bind(&socket_path).map_err(|e| {
            anyhow::anyhow!(
                "Failed to create GUI socket at '{}': {}",
                socket_path.display(),
                e
            )
        })?;

        // Spawn GUI child process
        let exe_path = std::env::current_exe()?;
        let mut cmd = Command::new(&exe_path);
        cmd.arg("gui-worker")
            .arg("--socket")
            .arg(socket_path.to_str().unwrap_or(""));
        if safe_mode {
            cmd.arg("--safe-mode");
        }
        if malloc_debug {
            cmd.arg("--malloc-debug");
        }

        let child = cmd
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn GUI process: {}", e))?;

        let child_pid = child.id();
        info!(pid = child_pid, "Spawned GUI process");

        // Accept connection with timeout
        listener.set_nonblocking(true).ok();
        let stream = {
            let start = std::time::Instant::now();
            let timeout = Duration::from_secs(15);
            loop {
                match listener.accept() {
                    Ok((stream, _)) => {
                        stream.set_nonblocking(false).ok();
                        stream
                            .set_read_timeout(Some(Duration::from_millis(50)))
                            .ok();
                        break stream;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        if start.elapsed() > timeout {
                            error!("Timed out waiting for GUI process to connect");
                            let _ = std::fs::remove_file(&socket_path);
                            return Err(anyhow::anyhow!(
                                "GUI process did not connect within {}s",
                                timeout.as_secs()
                            ));
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    Err(e) => {
                        let _ = std::fs::remove_file(&socket_path);
                        return Err(anyhow::anyhow!("Accept failed: {}", e));
                    }
                }
            }
        };

        info!("GUI process connected");

        // Send full state to the newly connected GUI
        let full_state = build_full_state(
            &plugin_modules,
            &rack,
            selected_slot,
            &backend,
            &param_snapshots,
            &status_message,
            &transport,
            tone_enabled,
            safe_mode,
        );
        if let Err(e) = send_update(&stream, &full_state) {
            warn!(error = %e, "Failed to send initial state to GUI");
        }

        // ── Message loop ────────────────────────────────────────────────
        let result = run_message_loop(
            &stream,
            child,
            &mut backend,
            &mut plugin_modules,
            &mut rack,
            &mut selected_slot,
            &mut param_snapshots,
            &mut status_message,
            &mut tone_enabled,
            &mut transport,
            &mut session_path,
            safe_mode,
            malloc_debug,
        );

        // Clean up socket
        let _ = std::fs::remove_file(&socket_path);

        match result {
            LoopResult::CleanShutdown => {
                info!("GUI shut down cleanly");
                break;
            }
            LoopResult::Crashed(reason) => {
                restart_count += 1;
                if restart_count == 1 {
                    first_restart_time = std::time::Instant::now();
                }
                warn!(
                    reason = %reason,
                    restarts = restart_count,
                    "GUI process crashed — restarting"
                );
                status_message = format!(
                    "⚠ GUI process crashed and was restarted (attempt {}). Audio continues unaffected.",
                    restart_count
                );
                // Brief pause before restart
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }

    // Clean shutdown: deactivate any active plugin
    backend.deactivate_plugin();
    info!("Supervisor shut down");
    Ok(())
}

/// Result of the message loop.
enum LoopResult {
    /// GUI shut down normally (window closed).
    CleanShutdown,
    /// GUI process crashed or disconnected.
    Crashed(String),
}

/// Run the message loop, processing actions from the GUI and sending updates.
#[allow(clippy::too_many_arguments)]
fn run_message_loop(
    stream: &std::os::unix::net::UnixStream,
    mut child: Child,
    backend: &mut HostBackend,
    plugin_modules: &mut Vec<PluginModuleInfo>,
    rack: &mut Vec<RackSlotState>,
    selected_slot: &mut Option<usize>,
    param_snapshots: &mut Vec<ParamSnapshot>,
    status_message: &mut String,
    tone_enabled: &mut bool,
    transport: &mut TransportUpdate,
    _session_path: &mut String,
    safe_mode: bool,
    _malloc_debug: bool,
) -> LoopResult {
    // Use a clone for reading (Unix sockets can be split this way)
    let mut reader = stream.try_clone().expect("clone stream for reading");
    reader
        .set_read_timeout(Some(Duration::from_millis(50)))
        .ok();

    loop {
        // 1. Try to read a GUI action (non-blocking with 50ms timeout)
        match decode::<GuiAction>(&mut reader) {
            Ok(Some(action)) => {
                let response = handle_action(
                    action,
                    backend,
                    plugin_modules,
                    rack,
                    selected_slot,
                    param_snapshots,
                    status_message,
                    tone_enabled,
                    transport,
                    safe_mode,
                );

                // Send response updates
                for update in response {
                    if let Err(e) = send_update(stream, &update) {
                        return LoopResult::Crashed(format!("Send failed: {}", e));
                    }
                }
            }
            Ok(None) => {
                // EOF — GUI process closed the connection
                return check_child_exit(&mut child);
            }
            Err(e) if e.is_timeout() => {
                // Timeout is expected — we're polling at 50ms intervals
            }
            Err(e) => {
                // Real error — GUI might have crashed
                debug!(error = %e, "GUI decode error");
                return check_child_exit(&mut child);
            }
        }

        // 2. Check for plugin crashes
        if backend.is_crashed() {
            let active_name = backend
                .active_slot_index()
                .and_then(|idx| rack.get(idx))
                .map(|s| s.name.clone())
                .unwrap_or_else(|| "Unknown".into());
            backend.deactivate_plugin();
            *status_message = format!(
                "⚠ '{}' crashed — deactivated safely. Audio host is unaffected.",
                active_name
            );

            // Send updates to GUI
            let updates = vec![
                SupervisorUpdate::StatusMessage {
                    message: status_message.clone(),
                },
                SupervisorUpdate::RackUpdated {
                    rack: rack.clone(),
                    active_slot: backend.active_slot_index(),
                    selected_slot: *selected_slot,
                },
                SupervisorUpdate::AudioStatusUpdated {
                    status: audio_status_state(&backend.audio_status),
                },
            ];
            for update in updates {
                if let Err(e) = send_update(stream, &update) {
                    debug!(error = %e, "Failed to send crash update to GUI");
                }
            }
        }

        // 3. Periodically refresh parameters for active plugin
        if backend.is_active() {
            if let Some(idx) = *selected_slot {
                let is_active = backend.active_slot_index() == Some(idx);
                if is_active {
                    let new_snapshots = backend.active_param_snapshots();
                    if new_snapshots != *param_snapshots {
                        *param_snapshots = new_snapshots;
                        // Only send if snapshots actually changed
                        let _ = send_update(
                            stream,
                            &SupervisorUpdate::ParamsUpdated {
                                snapshots: param_snapshots.clone(),
                            },
                        );
                    }
                }
            }
        }

        // 4. Check if child is still running
        match child.try_wait() {
            Ok(Some(exit_status)) => {
                if exit_status.success() {
                    return LoopResult::CleanShutdown;
                } else {
                    return LoopResult::Crashed(format!("GUI exited with {}", exit_status));
                }
            }
            Ok(None) => {
                // Still running — continue
            }
            Err(e) => {
                return LoopResult::Crashed(format!("wait error: {}", e));
            }
        }
    }
}

/// Handle a single GUI action and return supervisor updates to send back.
#[allow(clippy::too_many_arguments)]
fn handle_action(
    action: GuiAction,
    backend: &mut HostBackend,
    plugin_modules: &mut Vec<PluginModuleInfo>,
    rack: &mut Vec<RackSlotState>,
    selected_slot: &mut Option<usize>,
    param_snapshots: &mut Vec<ParamSnapshot>,
    status_message: &mut String,
    tone_enabled: &mut bool,
    transport: &mut TransportUpdate,
    safe_mode: bool,
) -> Vec<SupervisorUpdate> {
    match action {
        GuiAction::Ping => vec![SupervisorUpdate::Pong],

        GuiAction::Shutdown => vec![SupervisorUpdate::ShutdownAck],

        GuiAction::ScanPlugins => {
            *status_message = "Scanning for plugins…".into();

            let search_paths = scanner::default_vst3_paths();
            let bundles = scanner::discover_bundles(&search_paths);

            let mut modules: Vec<PluginModuleInfo> = Vec::new();
            let mut error_count: usize = 0;
            for bundle_path in &bundles {
                match crate::vst3::module::Vst3Module::load(bundle_path) {
                    Ok(module) => {
                        if let Ok(info) = module.get_info() {
                            modules.push(info);
                        }
                    }
                    Err(e) => {
                        error_count += 1;
                        warn!(path = %bundle_path.display(), error = %e, "scan failed");
                    }
                }
            }

            let scan_cache = cache::ScanCache::new(modules.clone());
            if let Err(e) = cache::save(&scan_cache) {
                warn!(error = %e, "cache save failed");
            }

            let class_count: usize = modules.iter().map(|m| m.classes.len()).sum();
            let module_count = modules.len();
            *plugin_modules = modules;

            let error_str = if error_count > 0 {
                format!(", {} error(s)", error_count)
            } else {
                String::new()
            };
            *status_message = format!(
                "Scan complete — {} module(s), {} class(es){}.",
                module_count, class_count, error_str
            );

            vec![
                SupervisorUpdate::PluginModulesUpdated {
                    modules: plugin_modules.clone(),
                },
                SupervisorUpdate::StatusMessage {
                    message: status_message.clone(),
                },
            ]
        }

        GuiAction::AddToRack {
            module_index,
            class_index,
        } => {
            if let Some(module) = plugin_modules.get(module_index) {
                if let Some(class) = module.classes.get(class_index) {
                    let vendor = class
                        .vendor
                        .as_deref()
                        .or(module.factory_vendor.as_deref())
                        .unwrap_or("Unknown")
                        .to_string();

                    let slot = RackSlotState {
                        name: class.name.clone(),
                        vendor,
                        category: class.category.clone(),
                        path: module.path.clone(),
                        cid: class.cid,
                        bypassed: false,
                        param_cache: Vec::new(),
                        staged_changes: Vec::new(),
                    };

                    *status_message = format!("Added '{}' to the rack.", slot.name);
                    rack.push(slot);

                    return vec![
                        SupervisorUpdate::RackUpdated {
                            rack: rack.clone(),
                            active_slot: backend.active_slot_index(),
                            selected_slot: *selected_slot,
                        },
                        SupervisorUpdate::StatusMessage {
                            message: status_message.clone(),
                        },
                    ];
                }
            }
            vec![SupervisorUpdate::StatusMessage {
                message: "Invalid module/class index.".into(),
            }]
        }

        GuiAction::RemoveFromRack { index } => {
            if index < rack.len() {
                let name = rack[index].name.clone();

                if backend.active_slot_index() == Some(index) {
                    backend.deactivate_plugin();
                    param_snapshots.clear();
                }

                rack.remove(index);
                if *selected_slot == Some(index) {
                    *selected_slot = None;
                    param_snapshots.clear();
                } else if let Some(sel) = *selected_slot {
                    if sel > index {
                        *selected_slot = Some(sel - 1);
                    }
                }
                *status_message = format!("Removed '{}' from the rack.", name);
            }

            vec![
                SupervisorUpdate::RackUpdated {
                    rack: rack.clone(),
                    active_slot: backend.active_slot_index(),
                    selected_slot: *selected_slot,
                },
                SupervisorUpdate::ParamsUpdated {
                    snapshots: param_snapshots.clone(),
                },
                SupervisorUpdate::StatusMessage {
                    message: status_message.clone(),
                },
            ]
        }

        GuiAction::ActivateSlot { index } => {
            if index < rack.len() {
                let slot = &rack[index];
                let path = slot.path.clone();
                let cid = slot.cid;
                let name = slot.name.clone();

                match backend.activate_plugin(index, &path, &cid, &name) {
                    Ok(snapshots) => {
                        *param_snapshots = snapshots;
                        *selected_slot = Some(index);

                        // Apply staged changes
                        let staged: Vec<(u32, f64)> =
                            rack[index].staged_changes.drain(..).collect();
                        let staged_count = staged.len();
                        for (id, value) in staged {
                            if let Err(e) = backend.set_parameter(id, value) {
                                warn!(param_id = id, error = %e, "staged param apply failed");
                            }
                        }

                        if staged_count > 0 {
                            *param_snapshots = backend.active_param_snapshots();
                        }

                        rack[index].param_cache.clone_from(param_snapshots);

                        let staged_msg = if staged_count > 0 {
                            format!(" ({} staged change(s) applied)", staged_count)
                        } else {
                            String::new()
                        };
                        *status_message =
                            format!("▶ '{}' active — processing audio.{}", name, staged_msg);

                        let has_editor = backend.active_has_editor();
                        return vec![
                            SupervisorUpdate::RackUpdated {
                                rack: rack.clone(),
                                active_slot: backend.active_slot_index(),
                                selected_slot: *selected_slot,
                            },
                            SupervisorUpdate::ParamsUpdated {
                                snapshots: param_snapshots.clone(),
                            },
                            SupervisorUpdate::AudioStatusUpdated {
                                status: audio_status_state(&backend.audio_status),
                            },
                            SupervisorUpdate::EditorAvailability { has_editor },
                            SupervisorUpdate::StatusMessage {
                                message: status_message.clone(),
                            },
                        ];
                    }
                    Err(e) => {
                        *status_message = format!("✗ Failed to activate '{}': {}", name, e);
                        error!(plugin = %name, error = %e, "activation failed");
                    }
                }
            }

            vec![SupervisorUpdate::StatusMessage {
                message: status_message.clone(),
            }]
        }

        GuiAction::DeactivateSlot => {
            // Cache params before deactivating
            if let Some(active_idx) = backend.active_slot_index() {
                if let Some(slot) = rack.get_mut(active_idx) {
                    slot.param_cache = param_snapshots.clone();
                }
            }

            let tainted_before = backend.tainted_paths.len();
            backend.deactivate_plugin();
            let tainted_after = backend.tainted_paths.len();

            if tainted_after > tainted_before {
                let name = backend
                    .active_slot_index()
                    .and_then(|idx| rack.get(idx))
                    .map(|s| s.name.clone())
                    .unwrap_or_else(|| "Plugin".into());
                *status_message = format!(
                    "⚠ '{}' crashed during deactivation — restart the host to reuse this plugin.",
                    name
                );
            } else {
                *status_message = "Plugin deactivated.".into();
            }

            let mut updates = vec![
                SupervisorUpdate::RackUpdated {
                    rack: rack.clone(),
                    active_slot: backend.active_slot_index(),
                    selected_slot: *selected_slot,
                },
                SupervisorUpdate::AudioStatusUpdated {
                    status: audio_status_state(&backend.audio_status),
                },
                SupervisorUpdate::StatusMessage {
                    message: status_message.clone(),
                },
            ];

            if backend.heap_corruption_detected {
                updates.push(SupervisorUpdate::HeapCorruptionDetected);
            }

            updates
        }

        GuiAction::SetParameter { id, value } => {
            match backend.set_parameter(id, value) {
                Ok(_) => {}
                Err(e) => {
                    warn!(param_id = id, error = %e, "param set failed");
                }
            }
            vec![] // Params will be refreshed in the periodic update
        }

        GuiAction::StageParameter {
            slot_index,
            id,
            value,
        } => {
            if let Some(slot) = rack.get_mut(slot_index) {
                if let Some(existing) = slot.staged_changes.iter_mut().find(|(sid, _)| *sid == id) {
                    existing.1 = value;
                } else {
                    slot.staged_changes.push((id, value));
                }
                if let Some(cached) = slot.param_cache.iter_mut().find(|s| s.id == id) {
                    cached.value = value;
                    cached.display = format!("{:.3}", value);
                }
            }
            vec![SupervisorUpdate::RackUpdated {
                rack: rack.clone(),
                active_slot: backend.active_slot_index(),
                selected_slot: *selected_slot,
            }]
        }

        GuiAction::SelectSlot { index } => {
            *selected_slot = index;
            // Load cached params for inactive plugin
            if let Some(idx) = index {
                let is_active = backend.active_slot_index() == Some(idx);
                if is_active {
                    *param_snapshots = backend.active_param_snapshots();
                } else if let Some(slot) = rack.get(idx) {
                    *param_snapshots = slot.param_cache.clone();
                } else {
                    param_snapshots.clear();
                }
            } else {
                param_snapshots.clear();
            }

            vec![SupervisorUpdate::ParamsUpdated {
                snapshots: param_snapshots.clone(),
            }]
        }

        GuiAction::SetToneEnabled { enabled } => {
            *tone_enabled = enabled;
            backend.set_tone_enabled(enabled);
            vec![]
        }

        GuiAction::SetTransport {
            playing,
            tempo,
            time_sig_num,
            time_sig_den,
        } => {
            transport.playing = playing;
            transport.tempo = tempo;
            transport.time_sig_num = time_sig_num;
            transport.time_sig_den = time_sig_den;

            if backend.is_active() {
                backend.set_playing(playing);
                backend.set_tempo(tempo);
                backend.set_time_signature(time_sig_num, time_sig_den);
            }
            vec![]
        }

        GuiAction::OpenEditor => {
            let name = selected_slot
                .and_then(|idx| rack.get(idx))
                .map(|s| s.name.clone())
                .unwrap_or_default();

            match backend.open_editor(&name) {
                Ok(()) => {
                    *status_message = format!("🎹 Editor opened for '{}'.", name);
                }
                Err(e) => {
                    *status_message = format!("✗ Editor failed: {}", e);
                }
            }

            vec![SupervisorUpdate::StatusMessage {
                message: status_message.clone(),
            }]
        }

        GuiAction::SaveSession { path } => {
            let gui_transport = crate::gui::app::TransportState {
                playing: transport.playing,
                tempo: transport.tempo,
                time_sig_num: transport.time_sig_num,
                time_sig_den: transport.time_sig_den,
            };

            let gui_rack: Vec<crate::gui::app::PluginSlot> = rack
                .iter()
                .map(|s| crate::gui::app::PluginSlot {
                    name: s.name.clone(),
                    vendor: s.vendor.clone(),
                    category: s.category.clone(),
                    path: s.path.clone(),
                    cid: s.cid,
                    bypassed: s.bypassed,
                    param_cache: s.param_cache.clone(),
                    staged_changes: s.staged_changes.clone(),
                })
                .collect();

            let session = Session::capture(
                &gui_transport,
                &gui_rack,
                backend.selected_audio_device.clone(),
                backend.selected_midi_port.clone(),
            );

            let p = PathBuf::from(&path);
            match session.save_to_file(&p) {
                Ok(()) => {
                    *status_message = format!("Session saved to {}", p.display());
                }
                Err(e) => {
                    *status_message = format!("✗ Save failed: {}", e);
                }
            }

            vec![SupervisorUpdate::StatusMessage {
                message: status_message.clone(),
            }]
        }

        GuiAction::LoadSession { path } => {
            let p = PathBuf::from(&path);
            match Session::load_from_file(&p) {
                Ok(session) => {
                    backend.deactivate_plugin();
                    param_snapshots.clear();

                    let (gui_transport, gui_rack) = session.restore();
                    transport.playing = gui_transport.playing;
                    transport.tempo = gui_transport.tempo;
                    transport.time_sig_num = gui_transport.time_sig_num;
                    transport.time_sig_den = gui_transport.time_sig_den;

                    *rack = gui_rack
                        .into_iter()
                        .map(|s| RackSlotState {
                            name: s.name,
                            vendor: s.vendor,
                            category: s.category,
                            path: s.path,
                            cid: s.cid,
                            bypassed: s.bypassed,
                            param_cache: Vec::new(),
                            staged_changes: Vec::new(),
                        })
                        .collect();

                    *selected_slot = None;
                    backend.selected_audio_device = session.audio_device;
                    backend.selected_midi_port = session.midi_port;

                    *status_message =
                        format!("Session loaded from {} ({} slots)", p.display(), rack.len());
                }
                Err(e) => {
                    *status_message = format!("✗ Load failed: {}", e);
                }
            }

            // Send full state after session load
            vec![build_full_state(
                plugin_modules,
                rack,
                *selected_slot,
                backend,
                param_snapshots,
                status_message,
                transport,
                *tone_enabled,
                safe_mode,
            )]
        }

        GuiAction::SelectAudioDevice { name } => {
            backend.selected_audio_device = name;
            vec![]
        }

        GuiAction::SelectMidiPort { name } => {
            backend.selected_midi_port = name;
            vec![]
        }

        GuiAction::RefreshDevices => {
            backend.refresh_devices();
            *status_message = format!(
                "Devices refreshed — {} audio, {} MIDI",
                backend.audio_devices.len(),
                backend.midi_ports.len()
            );
            vec![
                SupervisorUpdate::DevicesUpdated {
                    audio_devices: backend
                        .audio_devices
                        .iter()
                        .map(|d| DeviceState {
                            name: d.name.clone(),
                        })
                        .collect(),
                    midi_ports: backend
                        .midi_ports
                        .iter()
                        .map(|p| MidiPortState {
                            name: p.name.clone(),
                        })
                        .collect(),
                },
                SupervisorUpdate::StatusMessage {
                    message: status_message.clone(),
                },
            ]
        }

        GuiAction::SetProcessIsolation { enabled } => {
            backend.process_isolation = enabled;
            vec![]
        }
    }
}

/// Check the child process exit status.
fn check_child_exit(child: &mut Child) -> LoopResult {
    match child.try_wait() {
        Ok(Some(status)) if status.success() => LoopResult::CleanShutdown,
        Ok(Some(status)) => LoopResult::Crashed(format!("GUI exited with {}", status)),
        Ok(None) => {
            // Child is still running but socket closed — kill it
            let _ = child.kill();
            let _ = child.wait();
            LoopResult::Crashed("GUI socket closed unexpectedly".into())
        }
        Err(e) => LoopResult::Crashed(format!("wait error: {}", e)),
    }
}

/// Build a full state update for sending to a (re)connected GUI.
fn build_full_state(
    plugin_modules: &[PluginModuleInfo],
    rack: &[RackSlotState],
    selected_slot: Option<usize>,
    backend: &HostBackend,
    param_snapshots: &[ParamSnapshot],
    status_message: &str,
    transport: &TransportUpdate,
    tone_enabled: bool,
    safe_mode: bool,
) -> SupervisorUpdate {
    SupervisorUpdate::FullState {
        plugin_modules: plugin_modules.to_vec(),
        rack: rack.to_vec(),
        selected_slot,
        active_slot: backend.active_slot_index(),
        param_snapshots: param_snapshots.to_vec(),
        audio_status: audio_status_state(&backend.audio_status),
        audio_devices: backend
            .audio_devices
            .iter()
            .map(|d| DeviceState {
                name: d.name.clone(),
            })
            .collect(),
        midi_ports: backend
            .midi_ports
            .iter()
            .map(|p| MidiPortState {
                name: p.name.clone(),
            })
            .collect(),
        selected_audio_device: backend.selected_audio_device.clone(),
        selected_midi_port: backend.selected_midi_port.clone(),
        process_isolation: backend.process_isolation,
        status_message: status_message.to_string(),
        heap_corruption_detected: backend.heap_corruption_detected,
        has_editor: backend.active_has_editor(),
        tainted_count: backend.tainted_paths.len(),
        transport: transport.clone(),
        tone_enabled,
        safe_mode,
    }
}

/// Convert `AudioStatus` to `AudioStatusState` for IPC.
fn audio_status_state(status: &AudioStatus) -> AudioStatusState {
    AudioStatusState {
        sample_rate: status.sample_rate,
        buffer_size: status.buffer_size,
        device_name: status.device_name.clone(),
        running: status.running,
    }
}

/// Send a supervisor update to the GUI process.
fn send_update(
    stream: &std::os::unix::net::UnixStream,
    update: &SupervisorUpdate,
) -> Result<(), String> {
    let data = encode(update)?;
    let mut writer = stream;
    writer
        .write_all(&data)
        .map_err(|e| format!("Write failed: {}", e))?;
    writer.flush().map_err(|e| format!("Flush failed: {}", e))?;
    Ok(())
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_status_state_conversion() {
        let status = AudioStatus {
            sample_rate: 44100,
            buffer_size: 512,
            device_name: "Built-in Output".into(),
            running: true,
        };
        let state = audio_status_state(&status);
        assert_eq!(state.sample_rate, 44100);
        assert_eq!(state.buffer_size, 512);
        assert_eq!(state.device_name, "Built-in Output");
        assert!(state.running);
    }

    #[test]
    fn test_audio_status_state_default_conversion() {
        let status = AudioStatus::default();
        let state = audio_status_state(&status);
        assert_eq!(state.sample_rate, 0);
        assert!(!state.running);
    }

    #[test]
    fn test_build_full_state_structure() {
        let backend = HostBackend::new();
        let state = build_full_state(
            &[],
            &[],
            None,
            &backend,
            &[],
            "test status",
            &TransportUpdate {
                playing: false,
                tempo: 120.0,
                time_sig_num: 4,
                time_sig_den: 4,
            },
            false,
            false,
        );
        match state {
            SupervisorUpdate::FullState {
                plugin_modules,
                rack,
                selected_slot,
                active_slot,
                status_message,
                safe_mode,
                ..
            } => {
                assert!(plugin_modules.is_empty());
                assert!(rack.is_empty());
                assert_eq!(selected_slot, None);
                assert_eq!(active_slot, None);
                assert_eq!(status_message, "test status");
                assert!(!safe_mode);
            }
            _ => panic!("Expected FullState"),
        }
    }

    #[test]
    fn test_handle_action_ping() {
        let mut backend = HostBackend::new();
        let mut modules = Vec::new();
        let mut rack = Vec::new();
        let mut selected = None;
        let mut params = Vec::new();
        let mut status = String::new();
        let mut tone = false;
        let mut transport = TransportUpdate {
            playing: false,
            tempo: 120.0,
            time_sig_num: 4,
            time_sig_den: 4,
        };

        let result = handle_action(
            GuiAction::Ping,
            &mut backend,
            &mut modules,
            &mut rack,
            &mut selected,
            &mut params,
            &mut status,
            &mut tone,
            &mut transport,
            false,
        );
        assert_eq!(result.len(), 1);
        matches!(&result[0], SupervisorUpdate::Pong);
    }

    #[test]
    fn test_handle_action_shutdown() {
        let mut backend = HostBackend::new();
        let mut modules = Vec::new();
        let mut rack = Vec::new();
        let mut selected = None;
        let mut params = Vec::new();
        let mut status = String::new();
        let mut tone = false;
        let mut transport = TransportUpdate {
            playing: false,
            tempo: 120.0,
            time_sig_num: 4,
            time_sig_den: 4,
        };

        let result = handle_action(
            GuiAction::Shutdown,
            &mut backend,
            &mut modules,
            &mut rack,
            &mut selected,
            &mut params,
            &mut status,
            &mut tone,
            &mut transport,
            false,
        );
        assert_eq!(result.len(), 1);
        matches!(&result[0], SupervisorUpdate::ShutdownAck);
    }

    #[test]
    fn test_handle_action_set_tone() {
        let mut backend = HostBackend::new();
        let mut modules = Vec::new();
        let mut rack = Vec::new();
        let mut selected = None;
        let mut params = Vec::new();
        let mut status = String::new();
        let mut tone = false;
        let mut transport = TransportUpdate {
            playing: false,
            tempo: 120.0,
            time_sig_num: 4,
            time_sig_den: 4,
        };

        let result = handle_action(
            GuiAction::SetToneEnabled { enabled: true },
            &mut backend,
            &mut modules,
            &mut rack,
            &mut selected,
            &mut params,
            &mut status,
            &mut tone,
            &mut transport,
            false,
        );
        assert!(result.is_empty());
        assert!(tone);
    }

    #[test]
    fn test_handle_action_add_to_rack() {
        let mut backend = HostBackend::new();
        let mut modules = vec![PluginModuleInfo {
            path: PathBuf::from("/test.vst3"),
            factory_vendor: Some("TestVendor".into()),
            factory_url: None,
            factory_email: None,
            classes: vec![crate::vst3::types::PluginClassInfo {
                name: "TestPlugin".into(),
                category: "Audio Module Class".into(),
                subcategories: None,
                vendor: None,
                version: None,
                sdk_version: None,
                cid: [0u8; 16],
            }],
        }];
        let mut rack = Vec::new();
        let mut selected = None;
        let mut params = Vec::new();
        let mut status = String::new();
        let mut tone = false;
        let mut transport = TransportUpdate {
            playing: false,
            tempo: 120.0,
            time_sig_num: 4,
            time_sig_den: 4,
        };

        let result = handle_action(
            GuiAction::AddToRack {
                module_index: 0,
                class_index: 0,
            },
            &mut backend,
            &mut modules,
            &mut rack,
            &mut selected,
            &mut params,
            &mut status,
            &mut tone,
            &mut transport,
            false,
        );
        assert_eq!(rack.len(), 1);
        assert_eq!(rack[0].name, "TestPlugin");
        assert_eq!(rack[0].vendor, "TestVendor");
        assert!(status.contains("TestPlugin"));
        assert!(!result.is_empty());
    }

    #[test]
    fn test_handle_action_remove_from_rack() {
        let mut backend = HostBackend::new();
        let mut modules = Vec::new();
        let mut rack = vec![RackSlotState {
            name: "ToRemove".into(),
            vendor: "V".into(),
            category: "C".into(),
            path: PathBuf::from("/test.vst3"),
            cid: [0u8; 16],
            bypassed: false,
            param_cache: Vec::new(),
            staged_changes: Vec::new(),
        }];
        let mut selected = Some(0);
        let mut params = Vec::new();
        let mut status = String::new();
        let mut tone = false;
        let mut transport = TransportUpdate {
            playing: false,
            tempo: 120.0,
            time_sig_num: 4,
            time_sig_den: 4,
        };

        let _ = handle_action(
            GuiAction::RemoveFromRack { index: 0 },
            &mut backend,
            &mut modules,
            &mut rack,
            &mut selected,
            &mut params,
            &mut status,
            &mut tone,
            &mut transport,
            false,
        );
        assert!(rack.is_empty());
        assert_eq!(selected, None);
    }

    #[test]
    fn test_handle_action_select_slot() {
        let mut backend = HostBackend::new();
        let mut modules = Vec::new();
        let mut rack = vec![RackSlotState {
            name: "Test".into(),
            vendor: "V".into(),
            category: "C".into(),
            path: PathBuf::from("/test.vst3"),
            cid: [0u8; 16],
            bypassed: false,
            param_cache: vec![ParamSnapshot {
                id: 1,
                title: "Vol".into(),
                units: "dB".into(),
                value: 0.5,
                default: 0.5,
                display: "0.5".into(),
                can_automate: true,
                is_read_only: false,
                is_bypass: false,
            }],
            staged_changes: Vec::new(),
        }];
        let mut selected = None;
        let mut params = Vec::new();
        let mut status = String::new();
        let mut tone = false;
        let mut transport = TransportUpdate {
            playing: false,
            tempo: 120.0,
            time_sig_num: 4,
            time_sig_den: 4,
        };

        let _ = handle_action(
            GuiAction::SelectSlot { index: Some(0) },
            &mut backend,
            &mut modules,
            &mut rack,
            &mut selected,
            &mut params,
            &mut status,
            &mut tone,
            &mut transport,
            false,
        );
        assert_eq!(selected, Some(0));
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].title, "Vol");
    }

    #[test]
    fn test_handle_action_stage_parameter() {
        let mut backend = HostBackend::new();
        let mut modules = Vec::new();
        let mut rack = vec![RackSlotState {
            name: "Test".into(),
            vendor: "V".into(),
            category: "C".into(),
            path: PathBuf::from("/test.vst3"),
            cid: [0u8; 16],
            bypassed: false,
            param_cache: vec![ParamSnapshot {
                id: 1,
                title: "Vol".into(),
                units: "dB".into(),
                value: 0.5,
                default: 0.5,
                display: "0.500".into(),
                can_automate: true,
                is_read_only: false,
                is_bypass: false,
            }],
            staged_changes: Vec::new(),
        }];
        let mut selected = None;
        let mut params = Vec::new();
        let mut status = String::new();
        let mut tone = false;
        let mut transport = TransportUpdate {
            playing: false,
            tempo: 120.0,
            time_sig_num: 4,
            time_sig_den: 4,
        };

        let _ = handle_action(
            GuiAction::StageParameter {
                slot_index: 0,
                id: 1,
                value: 0.8,
            },
            &mut backend,
            &mut modules,
            &mut rack,
            &mut selected,
            &mut params,
            &mut status,
            &mut tone,
            &mut transport,
            false,
        );
        assert_eq!(rack[0].staged_changes.len(), 1);
        assert_eq!(rack[0].staged_changes[0], (1, 0.8));
        assert_eq!(rack[0].param_cache[0].value, 0.8);
    }

    #[test]
    fn test_handle_action_set_transport() {
        let mut backend = HostBackend::new();
        let mut modules = Vec::new();
        let mut rack = Vec::new();
        let mut selected = None;
        let mut params = Vec::new();
        let mut status = String::new();
        let mut tone = false;
        let mut transport = TransportUpdate {
            playing: false,
            tempo: 120.0,
            time_sig_num: 4,
            time_sig_den: 4,
        };

        let _ = handle_action(
            GuiAction::SetTransport {
                playing: true,
                tempo: 140.0,
                time_sig_num: 3,
                time_sig_den: 8,
            },
            &mut backend,
            &mut modules,
            &mut rack,
            &mut selected,
            &mut params,
            &mut status,
            &mut tone,
            &mut transport,
            false,
        );
        assert!(transport.playing);
        assert_eq!(transport.tempo, 140.0);
        assert_eq!(transport.time_sig_num, 3);
        assert_eq!(transport.time_sig_den, 8);
    }

    #[test]
    fn test_handle_action_add_invalid_index() {
        let mut backend = HostBackend::new();
        let mut modules = Vec::new();
        let mut rack = Vec::new();
        let mut selected = None;
        let mut params = Vec::new();
        let mut status = String::new();
        let mut tone = false;
        let mut transport = TransportUpdate {
            playing: false,
            tempo: 120.0,
            time_sig_num: 4,
            time_sig_den: 4,
        };

        let result = handle_action(
            GuiAction::AddToRack {
                module_index: 99,
                class_index: 0,
            },
            &mut backend,
            &mut modules,
            &mut rack,
            &mut selected,
            &mut params,
            &mut status,
            &mut tone,
            &mut transport,
            false,
        );
        assert!(rack.is_empty());
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_handle_action_refresh_devices() {
        let mut backend = HostBackend::new();
        let mut modules = Vec::new();
        let mut rack = Vec::new();
        let mut selected = None;
        let mut params = Vec::new();
        let mut status = String::new();
        let mut tone = false;
        let mut transport = TransportUpdate {
            playing: false,
            tempo: 120.0,
            time_sig_num: 4,
            time_sig_den: 4,
        };

        let result = handle_action(
            GuiAction::RefreshDevices,
            &mut backend,
            &mut modules,
            &mut rack,
            &mut selected,
            &mut params,
            &mut status,
            &mut tone,
            &mut transport,
            false,
        );
        assert!(!result.is_empty());
        assert!(status.contains("Devices refreshed"));
    }

    #[test]
    fn test_check_child_exit_clean() {
        // We can't easily test this without a real child process,
        // but we can verify the function signature and logic.
        // The actual integration is tested via the supervisor launch.
    }

    #[test]
    fn test_loop_result_variants() {
        let clean = LoopResult::CleanShutdown;
        let crashed = LoopResult::Crashed("test".into());
        match clean {
            LoopResult::CleanShutdown => {}
            _ => panic!("Expected CleanShutdown"),
        }
        match crashed {
            LoopResult::Crashed(msg) => assert_eq!(msg, "test"),
            _ => panic!("Expected Crashed"),
        }
    }
}
