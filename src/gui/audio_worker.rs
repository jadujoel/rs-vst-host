//! Audio worker — child process that runs the audio engine and plugin backend.
//!
//! The audio worker owns the [`HostBackend`] and all plugin instances. It
//! receives [`AudioCommand`] messages from the supervisor and replies with
//! [`SupervisorUpdate`] messages. If this process crashes (e.g., due to a
//! buggy plugin corrupting the heap), the supervisor stays alive and can
//! restart a fresh audio worker.
//!
//! # Process Architecture
//!
//! ```text
//! ┌─────────────────────┐         ┌─────────────────────┐
//! │  Supervisor Process  │         │  Audio Worker        │
//! │  (lightweight relay) │◄─sock──►│  (HostBackend +      │
//! │                      │         │   AudioEngine +      │
//! │  Relays messages     │         │   plugin instances)  │
//! │  Manages restarts    │         │                      │
//! └─────────────────────┘         └─────────────────────┘
//! ```

use crate::gui::backend::{AudioStatus, HostBackend, ParamSnapshot};
use crate::gui::ipc::*;
use crate::gui::session::Session;
use crate::vst3::types::PluginModuleInfo;
use crate::vst3::{cache, scanner};

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// Launch the audio worker process.
///
/// Connects to the supervisor via the given Unix socket path, then enters
/// a message loop processing [`AudioCommand`]s until shutdown.
///
/// # Arguments
/// * `socket_path` — Path to the Unix domain socket for IPC with the supervisor.
/// * `safe_mode` — If true, no plugins loaded from cache on startup.
/// * `malloc_debug` — If true, enable periodic heap checks.
pub fn launch_audio_worker(
    socket_path: &str,
    safe_mode: bool,
    _malloc_debug: bool,
) -> anyhow::Result<()> {
    let _span = tracing::info_span!("audio_worker").entered();
    info!(
        socket = %socket_path,
        safe_mode,
        "Audio worker starting"
    );

    let stream = UnixStream::connect(socket_path)
        .map_err(|e| anyhow::anyhow!("Failed to connect to supervisor socket: {}", e))?;
    stream
        .set_read_timeout(Some(Duration::from_millis(50)))
        .ok();

    info!("Connected to supervisor");

    // Build initial state
    let mut state = AudioWorkerState::new(safe_mode);

    // Enter message loop
    let result = run_audio_loop(&stream, &mut state, safe_mode);

    // Clean shutdown: deactivate any active plugin
    state.backend.deactivate_plugin();
    info!("Audio worker shut down");

    result
}

/// Internal state of the audio worker process.
struct AudioWorkerState {
    /// The host backend managing audio engine and plugin lifecycle.
    backend: HostBackend,
    /// Available plugin modules from scan cache.
    plugin_modules: Vec<PluginModuleInfo>,
    /// Current rack configuration.
    rack: Vec<RackSlotState>,
    /// Currently selected slot index.
    selected_slot: Option<usize>,
    /// Parameter snapshots for the selected plugin.
    param_snapshots: Vec<ParamSnapshot>,
    /// Status message shown in the GUI.
    status_message: String,
    /// Whether the test tone is enabled.
    tone_enabled: bool,
    /// Transport state.
    transport: TransportUpdate,
    /// Session file path.
    session_path: String,
}

impl AudioWorkerState {
    /// Create a new audio worker state with default configuration.
    fn new(safe_mode: bool) -> Self {
        let backend = HostBackend::new();
        let plugin_modules: Vec<PluginModuleInfo> = if safe_mode {
            Vec::new()
        } else {
            cache::load()
                .ok()
                .flatten()
                .map(|c| c.modules)
                .unwrap_or_default()
        };

        let status_message = if safe_mode {
            "Safe mode — no plugins loaded. Click 'Scan' to discover VST3 plugins.".into()
        } else if plugin_modules.is_empty() {
            "No plugins cached. Click 'Scan' to discover VST3 plugins.".into()
        } else {
            let total: usize = plugin_modules.iter().map(|m| m.classes.len()).sum();
            format!("{} plugin class(es) loaded from cache.", total)
        };

        let session_path = crate::gui::session::sessions_dir()
            .map(|d| d.join("default.json").to_string_lossy().to_string())
            .unwrap_or_else(|| "session.json".into());

        Self {
            backend,
            plugin_modules,
            rack: Vec::new(),
            selected_slot: None,
            param_snapshots: Vec::new(),
            status_message,
            tone_enabled: false,
            transport: TransportUpdate {
                playing: false,
                tempo: 120.0,
                time_sig_num: 4,
                time_sig_den: 4,
            },
            session_path,
        }
    }
}

/// Run the audio worker message loop.
fn run_audio_loop(
    stream: &UnixStream,
    state: &mut AudioWorkerState,
    safe_mode: bool,
) -> anyhow::Result<()> {
    let mut reader = stream.try_clone().expect("clone stream for reading");
    reader
        .set_read_timeout(Some(Duration::from_millis(50)))
        .ok();

    loop {
        // 1. Try to read an AudioCommand from the supervisor
        match decode::<AudioCommand>(&mut reader) {
            Ok(Some(cmd)) => match cmd {
                AudioCommand::Action(action) => {
                    // Check for shutdown
                    let is_shutdown = matches!(action, GuiAction::Shutdown);

                    let responses = handle_action(
                        action,
                        &mut state.backend,
                        &mut state.plugin_modules,
                        &mut state.rack,
                        &mut state.selected_slot,
                        &mut state.param_snapshots,
                        &mut state.status_message,
                        &mut state.tone_enabled,
                        &mut state.transport,
                        safe_mode,
                    );

                    for update in responses {
                        if let Err(e) = send_update(stream, &update) {
                            warn!(error = %e, "Failed to send update to supervisor");
                            return Ok(());
                        }
                    }

                    if is_shutdown {
                        info!("Shutdown requested — exiting audio worker");
                        return Ok(());
                    }
                }
                AudioCommand::RequestFullState => {
                    let full_state = build_full_state(
                        &state.plugin_modules,
                        &state.rack,
                        state.selected_slot,
                        &state.backend,
                        &state.param_snapshots,
                        &state.status_message,
                        &state.transport,
                        state.tone_enabled,
                        safe_mode,
                    );
                    if let Err(e) = send_update(stream, &full_state) {
                        warn!(error = %e, "Failed to send full state to supervisor");
                        return Ok(());
                    }
                }
                AudioCommand::RestoreState {
                    plugin_modules,
                    rack,
                    selected_slot,
                    tone_enabled,
                    transport,
                    session_path,
                } => {
                    info!(
                        modules = plugin_modules.len(),
                        slots = rack.len(),
                        "Restoring state after restart"
                    );
                    state.plugin_modules = plugin_modules;
                    state.rack = rack;
                    state.selected_slot = selected_slot;
                    state.tone_enabled = tone_enabled;
                    state.transport = transport;
                    state.session_path = session_path;
                    state.param_snapshots.clear();
                    state.status_message =
                        "⚠ Audio process restarted — plugins need to be re-activated.".into();

                    // Send updated state back to supervisor
                    let full_state = build_full_state(
                        &state.plugin_modules,
                        &state.rack,
                        state.selected_slot,
                        &state.backend,
                        &state.param_snapshots,
                        &state.status_message,
                        &state.transport,
                        state.tone_enabled,
                        safe_mode,
                    );
                    if let Err(e) = send_update(stream, &full_state) {
                        warn!(error = %e, "Failed to send restored state to supervisor");
                        return Ok(());
                    }
                }
                AudioCommand::Shutdown => {
                    // Send ack and exit
                    let _ = send_update(stream, &SupervisorUpdate::ShutdownAck);
                    info!("Shutdown command received — exiting audio worker");
                    return Ok(());
                }
            },
            Ok(None) => {
                // EOF — supervisor disconnected
                info!("Supervisor disconnected — shutting down audio worker");
                return Ok(());
            }
            Err(e) if e.is_timeout() => {
                // Timeout is expected — we're polling at 50ms intervals
            }
            Err(e) => {
                // Real error
                error!(error = %e, "Audio worker decode error");
                return Err(anyhow::anyhow!("Audio worker IPC error: {}", e));
            }
        }

        // 2. Check for plugin crashes
        if state.backend.is_crashed() {
            let active_name = state
                .backend
                .active_slot_index()
                .and_then(|idx| state.rack.get(idx))
                .map(|s| s.name.clone())
                .unwrap_or_else(|| "Unknown".into());
            state.backend.deactivate_plugin();
            state.status_message = format!(
                "⚠ '{}' crashed — deactivated safely. Audio host is unaffected.",
                active_name
            );

            let updates = vec![
                SupervisorUpdate::StatusMessage {
                    message: state.status_message.clone(),
                },
                SupervisorUpdate::RackUpdated {
                    rack: state.rack.clone(),
                    active_slot: state.backend.active_slot_index(),
                    selected_slot: state.selected_slot,
                },
                SupervisorUpdate::AudioStatusUpdated {
                    status: audio_status_state(&state.backend.audio_status),
                },
            ];
            for update in updates {
                if let Err(e) = send_update(stream, &update) {
                    debug!(error = %e, "Failed to send crash update to supervisor");
                }
            }
        }

        // 3. Periodically refresh parameters for active plugin
        if state.backend.is_active() {
            if let Some(idx) = state.selected_slot {
                let is_active = state.backend.active_slot_index() == Some(idx);
                if is_active {
                    let new_snapshots = state.backend.active_param_snapshots();
                    if new_snapshots != state.param_snapshots {
                        state.param_snapshots = new_snapshots;
                        let _ = send_update(
                            stream,
                            &SupervisorUpdate::ParamsUpdated {
                                snapshots: state.param_snapshots.clone(),
                            },
                        );
                    }
                }
            }
        }
    }
}

// ── Action handling (moved from supervisor) ──────────────────────────────

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

/// Build a full state update for sending to the supervisor.
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

/// Send a supervisor update to the supervisor via the socket.
fn send_update(
    stream: &UnixStream,
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
    fn test_audio_worker_state_default() {
        let state = AudioWorkerState::new(true);
        assert!(state.plugin_modules.is_empty());
        assert!(state.rack.is_empty());
        assert_eq!(state.selected_slot, None);
        assert!(state.param_snapshots.is_empty());
        assert!(state.status_message.contains("Safe mode"));
        assert!(!state.tone_enabled);
    }

    #[test]
    fn test_audio_worker_state_normal() {
        let state = AudioWorkerState::new(false);
        // Modules may or may not exist depending on cache
        assert!(state.rack.is_empty());
        assert_eq!(state.selected_slot, None);
    }

    #[test]
    fn test_audio_status_state_conversion() {
        let status = AudioStatus {
            sample_rate: 48000,
            buffer_size: 256,
            device_name: "Test Device".into(),
            running: true,
        };
        let state = audio_status_state(&status);
        assert_eq!(state.sample_rate, 48000);
        assert_eq!(state.buffer_size, 256);
        assert_eq!(state.device_name, "Test Device");
        assert!(state.running);
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
    fn test_audio_command_serialize_roundtrip() {
        let commands = vec![
            AudioCommand::Action(GuiAction::Ping),
            AudioCommand::RequestFullState,
            AudioCommand::RestoreState {
                plugin_modules: Vec::new(),
                rack: Vec::new(),
                selected_slot: None,
                tone_enabled: false,
                transport: TransportUpdate {
                    playing: false,
                    tempo: 120.0,
                    time_sig_num: 4,
                    time_sig_den: 4,
                },
                session_path: "test.json".into(),
            },
            AudioCommand::Shutdown,
        ];

        for cmd in &commands {
            let json = serde_json::to_string(cmd).expect("serialize");
            let decoded: AudioCommand = serde_json::from_str(&json).expect("deserialize");
            let json2 = serde_json::to_string(&decoded).expect("re-serialize");
            assert_eq!(json, json2, "roundtrip failed for {:?}", cmd);
        }
    }
}
