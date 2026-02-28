//! GUI worker — child process that renders the eframe/egui window.
//!
//! This runs in a separate process from the supervisor. If the GUI crashes
//! (e.g., due to a plugin editor corrupting memory), the supervisor detects
//! it and relaunches a fresh GUI process. Audio processing continues
//! uninterrupted in the supervisor.
//!
//! Communication with the supervisor is via a Unix domain socket using
//! the [`super::ipc`] protocol.

use crate::gui::backend::ParamSnapshot;
use crate::gui::ipc::*;
use crate::gui::theme;
use crate::vst3::types::{PluginClassInfo, PluginModuleInfo};

use eframe::egui;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};
use tracing::{debug, error, info};

// ── Data structures ─────────────────────────────────────────────────────

/// State managed by the GUI worker process.
///
/// This mirrors the fields from `HostApp` but instead of directly managing
/// audio/plugins, it communicates with the supervisor via IPC.
pub struct GuiWorkerApp {
    /// Connection to the supervisor process.
    conn: Arc<Mutex<UnixStream>>,
    /// Available plugin modules (from supervisor).
    plugin_modules: Vec<PluginModuleInfo>,
    /// Rack slots (from supervisor).
    rack: Vec<RackSlotState>,
    /// Currently selected slot.
    selected_slot: Option<usize>,
    /// Active slot (processing audio).
    active_slot: Option<usize>,
    /// Parameter snapshots for the selected plugin.
    param_snapshots: Vec<ParamSnapshot>,
    /// Audio status.
    audio_status: AudioStatusState,
    /// Available audio devices.
    audio_devices: Vec<DeviceState>,
    /// Available MIDI ports.
    midi_ports: Vec<MidiPortState>,
    /// Selected audio device.
    selected_audio_device: Option<String>,
    /// Selected MIDI port.
    selected_midi_port: Option<String>,
    /// Whether process isolation is enabled.
    process_isolation: bool,
    /// Status message.
    status_message: String,
    /// Whether heap corruption has been detected.
    heap_corruption_detected: bool,
    /// Whether the active plugin has an editor.
    has_editor: bool,
    /// Transport state.
    transport: crate::gui::app::TransportState,
    /// Previous transport state for change detection.
    prev_transport: TransportUpdate,
    /// Whether the tone is enabled.
    tone_enabled: bool,
    /// Whether safe mode is active.
    safe_mode: bool,
    /// Whether the theme has been applied.
    theme_applied: bool,
    /// Browser search filter.
    browser_filter: String,
    /// Parameter search filter.
    param_filter: String,
    /// Bottom tab selection.
    bottom_tab: crate::gui::app::BottomTab,
    /// Session file path.
    session_path: String,
    /// Number of tainted paths (for display).
    tainted_count: usize,
    /// Whether the supervisor process has disconnected (crashed or exited).
    ///
    /// When `true`, the GUI stops sending actions and displays a
    /// "supervisor lost" banner. The user should close and restart.
    supervisor_disconnected: bool,
}

impl GuiWorkerApp {
    /// Create a new GUI worker app connected to the supervisor.
    pub fn new(stream: UnixStream) -> Self {
        let default_session_path = crate::gui::session::sessions_dir()
            .map(|d| d.join("default.json").to_string_lossy().to_string())
            .unwrap_or_else(|| "session.json".into());

        Self {
            conn: Arc::new(Mutex::new(stream)),
            plugin_modules: Vec::new(),
            rack: Vec::new(),
            selected_slot: None,
            active_slot: None,
            param_snapshots: Vec::new(),
            audio_status: AudioStatusState::default(),
            audio_devices: Vec::new(),
            midi_ports: Vec::new(),
            selected_audio_device: None,
            selected_midi_port: None,
            process_isolation: false,
            status_message: "Connecting to supervisor…".into(),
            heap_corruption_detected: false,
            has_editor: false,
            transport: crate::gui::app::TransportState::default(),
            prev_transport: TransportUpdate {
                playing: false,
                tempo: 120.0,
                time_sig_num: 4,
                time_sig_den: 4,
            },
            tone_enabled: false,
            safe_mode: false,
            theme_applied: false,
            browser_filter: String::new(),
            param_filter: String::new(),
            bottom_tab: crate::gui::app::BottomTab::Transport,
            session_path: default_session_path,
            tainted_count: 0,
            supervisor_disconnected: false,
        }
    }

    /// Send an action to the supervisor.
    ///
    /// If the supervisor has disconnected, this is a no-op to avoid
    /// spamming "Broken pipe" errors on every frame.
    fn send_action(&mut self, action: GuiAction) {
        if self.supervisor_disconnected {
            return;
        }
        let mut disconnected = false;
        {
            let Ok(conn) = self.conn.lock() else {
                return;
            };
            let data = match encode(&action) {
                Ok(d) => d,
                Err(e) => {
                    error!(error = %e, "Failed to encode action");
                    return;
                }
            };
            let mut writer: &UnixStream = &conn;
            if let Err(e) = writer.write_all(&data) {
                if e.kind() == std::io::ErrorKind::BrokenPipe {
                    disconnected = true;
                } else {
                    error!(error = %e, "Failed to send action to supervisor");
                }
            } else if let Err(e) = writer.flush() {
                if e.kind() == std::io::ErrorKind::BrokenPipe {
                    disconnected = true;
                } else {
                    error!(error = %e, "Failed to flush action");
                }
            }
        }
        if disconnected {
            self.mark_supervisor_disconnected();
        }
    }

    /// Mark the supervisor as disconnected and update the UI state.
    fn mark_supervisor_disconnected(&mut self) {
        if !self.supervisor_disconnected {
            self.supervisor_disconnected = true;
            self.active_slot = None;
            self.audio_status = AudioStatusState::default();
            self.status_message =
                "\u{26A0} Supervisor process died \u{2014} please close and restart the application.".into();
            error!("Supervisor process disconnected \u{2014} GUI is orphaned");
        }
    }

    /// Poll for updates from the supervisor (non-blocking).
    fn poll_updates(&mut self) {
        if self.supervisor_disconnected {
            return;
        }

        // Collect updates while holding the lock, then apply after releasing it.
        let mut eof_detected = false;
        let updates: Vec<SupervisorUpdate> = {
            let Ok(mut conn) = self.conn.lock() else {
                return;
            };
            conn.set_read_timeout(Some(std::time::Duration::from_millis(1)))
                .ok();

            let mut collected = Vec::new();
            loop {
                match decode::<SupervisorUpdate>(&mut *conn) {
                    Ok(Some(update)) => {
                        collected.push(update);
                    }
                    Ok(None) => {
                        // EOF — supervisor disconnected
                        eof_detected = true;
                        break;
                    }
                    Err(_) => {
                        // Timeout or would-block — no more data available
                        break;
                    }
                }
            }
            collected
        };

        for update in updates {
            self.apply_update(update);
        }

        if eof_detected {
            self.mark_supervisor_disconnected();
        }
    }

    /// Apply a supervisor update to local state.
    fn apply_update(&mut self, update: SupervisorUpdate) {
        match update {
            SupervisorUpdate::FullState {
                plugin_modules,
                rack,
                selected_slot,
                active_slot,
                param_snapshots,
                audio_status,
                audio_devices,
                midi_ports,
                selected_audio_device,
                selected_midi_port,
                process_isolation,
                status_message,
                heap_corruption_detected,
                has_editor,
                tainted_count,
                transport,
                tone_enabled,
                safe_mode,
            } => {
                self.plugin_modules = plugin_modules;
                self.rack = rack;
                self.selected_slot = selected_slot;
                self.active_slot = active_slot;
                self.param_snapshots = param_snapshots;
                self.audio_status = audio_status;
                self.audio_devices = audio_devices;
                self.midi_ports = midi_ports;
                self.selected_audio_device = selected_audio_device;
                self.selected_midi_port = selected_midi_port;
                self.process_isolation = process_isolation;
                self.status_message = status_message;
                self.heap_corruption_detected = heap_corruption_detected;
                self.has_editor = has_editor;
                self.tainted_count = tainted_count;
                self.transport.playing = transport.playing;
                self.transport.tempo = transport.tempo;
                self.transport.time_sig_num = transport.time_sig_num;
                self.transport.time_sig_den = transport.time_sig_den;
                self.prev_transport = TransportUpdate {
                    playing: self.transport.playing,
                    tempo: self.transport.tempo,
                    time_sig_num: self.transport.time_sig_num,
                    time_sig_den: self.transport.time_sig_den,
                };
                self.tone_enabled = tone_enabled;
                self.safe_mode = safe_mode;
                debug!("Applied full state from supervisor");
            }

            SupervisorUpdate::RackUpdated {
                rack,
                active_slot,
                selected_slot,
            } => {
                self.rack = rack;
                self.active_slot = active_slot;
                self.selected_slot = selected_slot;
            }

            SupervisorUpdate::ParamsUpdated { snapshots } => {
                self.param_snapshots = snapshots;
            }

            SupervisorUpdate::StatusMessage { message } => {
                self.status_message = message;
            }

            SupervisorUpdate::AudioStatusUpdated { status } => {
                self.audio_status = status;
            }

            SupervisorUpdate::PluginModulesUpdated { modules } => {
                self.plugin_modules = modules;
            }

            SupervisorUpdate::DevicesUpdated {
                audio_devices,
                midi_ports,
            } => {
                self.audio_devices = audio_devices;
                self.midi_ports = midi_ports;
            }

            SupervisorUpdate::HeapCorruptionDetected => {
                self.heap_corruption_detected = true;
            }

            SupervisorUpdate::EditorAvailability { has_editor } => {
                self.has_editor = has_editor;
            }

            SupervisorUpdate::Pong => {
                debug!("Received pong from supervisor");
            }

            SupervisorUpdate::ShutdownAck => {
                debug!("Supervisor acknowledged shutdown");
            }

            SupervisorUpdate::AudioProcessRestarted {
                message,
                restart_count: _,
            } => {
                self.status_message = message;
                // Active plugin is lost after audio process restart
                self.active_slot = None;
                self.param_snapshots.clear();
                self.has_editor = false;
                self.audio_status = AudioStatusState::default();
                debug!("Audio process was restarted by supervisor");
            }
        }
    }

    /// Sync transport changes to the supervisor (only sends if changed).
    fn sync_transport(&mut self) {
        let changed = self.transport.tempo != self.prev_transport.tempo
            || self.transport.playing != self.prev_transport.playing
            || self.transport.time_sig_num != self.prev_transport.time_sig_num
            || self.transport.time_sig_den != self.prev_transport.time_sig_den;

        if changed {
            self.send_action(GuiAction::SetTransport {
                playing: self.transport.playing,
                tempo: self.transport.tempo,
                time_sig_num: self.transport.time_sig_num,
                time_sig_den: self.transport.time_sig_den,
            });
            self.prev_transport = TransportUpdate {
                playing: self.transport.playing,
                tempo: self.transport.tempo,
                time_sig_num: self.transport.time_sig_num,
                time_sig_den: self.transport.time_sig_den,
            };
        }
    }

    /// Get the filterd plugin classes for the browser.
    fn filtered_classes(&self) -> Vec<(usize, usize, &PluginModuleInfo, &PluginClassInfo)> {
        let filter = self.browser_filter.to_lowercase();
        let mut results = Vec::new();
        for (mi, module) in self.plugin_modules.iter().enumerate() {
            for (ci, class) in module.classes.iter().enumerate() {
                // Only show Audio Module Class entries — hide Component Controller
                // and Plugin Compatibility classes which are internal VST3 details.
                if class.category != "Audio Module Class" {
                    continue;
                }
                if filter.is_empty()
                    || class.name.to_lowercase().contains(&filter)
                    || class.category.to_lowercase().contains(&filter)
                    || class
                        .subcategories
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&filter)
                    || class
                        .vendor
                        .as_deref()
                        .or(module.factory_vendor.as_deref())
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&filter)
                {
                    results.push((mi, ci, module, class));
                }
            }
        }
        results
    }
}

// ── eframe::App implementation ──────────────────────────────────────────

impl eframe::App for GuiWorkerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Apply theme once
        if !self.theme_applied {
            theme::apply(ctx);
            self.theme_applied = true;
        }

        // Detect window close and send Shutdown to supervisor so it
        // doesn't interpret the disconnect as a crash and relaunch.
        if ctx.input(|i| i.viewport().close_requested()) {
            info!("Window close requested — sending Shutdown to supervisor");
            self.send_action(GuiAction::Shutdown);
            // Allow the message to be transmitted before dropping the socket
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        // Poll for supervisor updates
        self.poll_updates();

        // Sync transport changes
        self.sync_transport();

        // Request repaint to keep polling
        ctx.request_repaint_after(std::time::Duration::from_millis(33)); // ~30fps

        // Keyboard shortcuts
        ctx.input(|input| {
            if input.key_pressed(egui::Key::Space) {
                self.transport.playing = !self.transport.playing;
            }
        });

        // — Supervisor Disconnected Banner —
        if self.supervisor_disconnected {
            egui::TopBottomPanel::top("supervisor_disconnected_warning")
                .frame(egui::Frame {
                    fill: egui::Color32::from_rgb(180, 30, 30),
                    inner_margin: egui::Margin::symmetric(16, 8),
                    ..Default::default()
                })
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("⚠ Supervisor process died — please close and restart the application.")
                                .color(egui::Color32::WHITE)
                                .strong()
                                .size(14.0),
                        );
                    });
                });
        }

        // — Heap Corruption Warning Banner —
        if self.heap_corruption_detected {
            egui::TopBottomPanel::top("heap_corruption_warning")
                .frame(egui::Frame {
                    fill: egui::Color32::from_rgb(180, 30, 30),
                    inner_margin: egui::Margin::symmetric(16, 8),
                    ..Default::default()
                })
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(
                                "⚠ Heap corruption detected — save your session and restart.",
                            )
                            .color(egui::Color32::WHITE)
                            .strong()
                            .size(14.0),
                        );
                    });
                });
        }

        // — Left: Plugin Browser —
        egui::SidePanel::left("plugin_browser")
            .default_width(280.0)
            .resizable(true)
            .frame(egui::Frame {
                fill: theme::BG_BASE,
                inner_margin: egui::Margin::same(12),
                ..Default::default()
            })
            .show(ctx, |ui| {
                self.show_browser(ui);
            });

        // — Right: Parameter View —
        if self.selected_slot.is_some() {
            egui::SidePanel::right("param_panel")
                .default_width(320.0)
                .resizable(true)
                .frame(egui::Frame {
                    fill: theme::BG_BASE,
                    inner_margin: egui::Margin::same(12),
                    ..Default::default()
                })
                .show(ctx, |ui| {
                    self.show_param_panel(ui);
                });
        }

        // — Bottom: Transport / Devices / Session —
        egui::TopBottomPanel::bottom("transport_bar")
            .frame(egui::Frame {
                fill: theme::PANEL_FILL,
                inner_margin: egui::Margin::symmetric(16, 8),
                stroke: egui::Stroke::new(1.0, theme::GLASS_BORDER),
                ..Default::default()
            })
            .show(ctx, |ui| {
                self.show_bottom_bar(ui);
            });

        // — Central: Plugin Rack —
        egui::CentralPanel::default()
            .frame(egui::Frame {
                fill: theme::BG_BASE,
                inner_margin: egui::Margin::same(16),
                ..Default::default()
            })
            .show(ctx, |ui| {
                self.show_rack(ui);
            });
    }
}

// ── Panel renderers ─────────────────────────────────────────────────────

impl GuiWorkerApp {
    fn show_browser(&mut self, ui: &mut egui::Ui) {
        ui.heading("Plugin Browser");
        ui.add_space(8.0);

        if ui
            .add(
                egui::Button::new("⟳  Scan Plugins")
                    .min_size(egui::vec2(ui.available_width(), 28.0)),
            )
            .clicked()
        {
            self.send_action(GuiAction::ScanPlugins);
        }

        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.label("🔍");
            ui.add(
                egui::TextEdit::singleline(&mut self.browser_filter)
                    .hint_text("Filter…")
                    .desired_width(ui.available_width()),
            );
        });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        let classes: Vec<(usize, usize, PluginModuleInfo, PluginClassInfo)> = self
            .filtered_classes()
            .into_iter()
            .map(|(mi, ci, m, c)| (mi, ci, m.clone(), c.clone()))
            .collect();

        if classes.is_empty() {
            ui.label(
                egui::RichText::new("No plugins found.")
                    .color(theme::TEXT_SECONDARY)
                    .italics(),
            );
        } else {
            let mut add_action: Option<(usize, usize)> = None;

            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    for (mi, ci, module, class) in &classes {
                        let vendor = class
                            .vendor
                            .as_deref()
                            .or(module.factory_vendor.as_deref())
                            .unwrap_or("Unknown");

                        let subcats = class.subcategories.as_deref().unwrap_or("");

                        theme::glass_card_frame().show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(
                                        egui::RichText::new(&class.name)
                                            .color(theme::TEXT_PRIMARY)
                                            .strong(),
                                    );
                                    ui.label(
                                        egui::RichText::new(vendor)
                                            .color(theme::TEXT_SECONDARY)
                                            .small(),
                                    );
                                    if !subcats.is_empty() {
                                        ui.label(
                                            egui::RichText::new(subcats)
                                                .color(theme::TEXT_DISABLED)
                                                .small(),
                                        );
                                    }
                                });
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.button("＋").clicked() {
                                            add_action = Some((*mi, *ci));
                                        }
                                    },
                                );
                            });
                        });

                        ui.add_space(2.0);
                    }
                });

            if let Some((mi, ci)) = add_action {
                self.send_action(GuiAction::AddToRack {
                    module_index: mi,
                    class_index: ci,
                });
            }
        }
    }

    fn show_bottom_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui
                .selectable_label(
                    self.bottom_tab == crate::gui::app::BottomTab::Transport,
                    "🎵 Transport",
                )
                .clicked()
            {
                self.bottom_tab = crate::gui::app::BottomTab::Transport;
            }
            if ui
                .selectable_label(
                    self.bottom_tab == crate::gui::app::BottomTab::Devices,
                    "🔊 Devices",
                )
                .clicked()
            {
                self.bottom_tab = crate::gui::app::BottomTab::Devices;
            }
            if ui
                .selectable_label(
                    self.bottom_tab == crate::gui::app::BottomTab::Session,
                    "💾 Session",
                )
                .clicked()
            {
                self.bottom_tab = crate::gui::app::BottomTab::Session;
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(&self.status_message)
                        .color(theme::TEXT_SECONDARY)
                        .small(),
                );
            });
        });

        ui.separator();

        match self.bottom_tab {
            crate::gui::app::BottomTab::Transport => self.show_transport(ui),
            crate::gui::app::BottomTab::Devices => self.show_devices(ui),
            crate::gui::app::BottomTab::Session => self.show_session(ui),
        }
    }

    fn show_transport(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let play_label = if self.transport.playing { "⏸" } else { "▶" };
            if ui.button(play_label).clicked() {
                self.transport.playing = !self.transport.playing;
            }

            ui.separator();

            ui.label("BPM");
            ui.add(
                egui::DragValue::new(&mut self.transport.tempo)
                    .range(20.0..=300.0)
                    .speed(0.5)
                    .fixed_decimals(1),
            );

            ui.separator();

            ui.label("Time");
            ui.add(
                egui::DragValue::new(&mut self.transport.time_sig_num)
                    .range(1..=16)
                    .speed(0.1),
            );
            ui.label("/");
            ui.add(
                egui::DragValue::new(&mut self.transport.time_sig_den)
                    .range(1..=16)
                    .speed(0.1),
            );

            ui.separator();

            let tone_label = if self.tone_enabled {
                "🔔 Tone On"
            } else {
                "🔕 Tone Off"
            };
            if ui.button(tone_label).clicked() {
                self.tone_enabled = !self.tone_enabled;
                self.send_action(GuiAction::SetToneEnabled {
                    enabled: self.tone_enabled,
                });
            }

            if self.audio_status.running {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} Hz • {} frames • {}",
                            self.audio_status.sample_rate,
                            self.audio_status.buffer_size,
                            self.audio_status.device_name,
                        ))
                        .color(theme::TEXT_DISABLED)
                        .small(),
                    );
                });
            }
        });
    }

    fn show_devices(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Audio Output:");
            let current_audio = self
                .selected_audio_device
                .clone()
                .unwrap_or_else(|| "(default)".into());

            egui::ComboBox::from_id_salt("audio_device_combo")
                .selected_text(&current_audio)
                .width(250.0)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(self.selected_audio_device.is_none(), "(default)")
                        .clicked()
                    {
                        self.selected_audio_device = None;
                        self.send_action(GuiAction::SelectAudioDevice { name: None });
                    }
                    let devices = self.audio_devices.clone();
                    for dev in &devices {
                        if ui
                            .selectable_label(
                                self.selected_audio_device.as_deref() == Some(&dev.name),
                                &dev.name,
                            )
                            .clicked()
                        {
                            self.selected_audio_device = Some(dev.name.clone());
                            self.send_action(GuiAction::SelectAudioDevice {
                                name: Some(dev.name.clone()),
                            });
                        }
                    }
                });

            ui.separator();

            ui.label("MIDI Input:");
            let current_midi = self
                .selected_midi_port
                .clone()
                .unwrap_or_else(|| "(none)".into());

            egui::ComboBox::from_id_salt("midi_port_combo")
                .selected_text(&current_midi)
                .width(250.0)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(self.selected_midi_port.is_none(), "(none)")
                        .clicked()
                    {
                        self.selected_midi_port = None;
                        self.send_action(GuiAction::SelectMidiPort { name: None });
                    }
                    let ports = self.midi_ports.clone();
                    for port in &ports {
                        if ui
                            .selectable_label(
                                self.selected_midi_port.as_deref() == Some(&port.name),
                                &port.name,
                            )
                            .clicked()
                        {
                            self.selected_midi_port = Some(port.name.clone());
                            self.send_action(GuiAction::SelectMidiPort {
                                name: Some(port.name.clone()),
                            });
                        }
                    }
                });

            ui.separator();

            if ui.button("⟳ Refresh").clicked() {
                self.send_action(GuiAction::RefreshDevices);
            }
        });
    }

    fn show_session(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Path:");
            ui.add(
                egui::TextEdit::singleline(&mut self.session_path)
                    .hint_text("session.json")
                    .desired_width(400.0),
            );

            if ui.button("💾 Save").clicked() {
                self.send_action(GuiAction::SaveSession {
                    path: self.session_path.clone(),
                });
            }
            if ui.button("📂 Load").clicked() {
                self.send_action(GuiAction::LoadSession {
                    path: self.session_path.clone(),
                });
            }
        });
    }

    fn show_param_panel(&mut self, ui: &mut egui::Ui) {
        let Some(idx) = self.selected_slot else {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label(
                    egui::RichText::new("Select a plugin from the rack to view its parameters.")
                        .color(theme::TEXT_SECONDARY)
                        .italics(),
                );
            });
            return;
        };

        let (slot_name, slot_vendor) = self
            .rack
            .get(idx)
            .map(|s| (s.name.clone(), s.vendor.clone()))
            .unwrap_or_else(|| ("Parameters".into(), String::new()));

        let is_active = self.active_slot == Some(idx);

        ui.heading(format!("🎛 {}", slot_name));
        if !slot_vendor.is_empty() {
            ui.label(
                egui::RichText::new(&slot_vendor)
                    .color(theme::TEXT_SECONDARY)
                    .small(),
            );
        }
        ui.add_space(4.0);

        if !is_active {
            if !self.param_snapshots.is_empty() {
                ui.label(
                    egui::RichText::new(
                        "⚠ Plugin is inactive — changes will be applied on activation.",
                    )
                    .color(theme::WARNING)
                    .small(),
                );
                ui.add_space(4.0);
            } else {
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    ui.label(
                        egui::RichText::new(
                            "Activate this plugin to view and edit its parameters.",
                        )
                        .color(theme::TEXT_SECONDARY)
                        .italics(),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("Click ▶ in the rack to activate.")
                            .color(theme::TEXT_DISABLED)
                            .small(),
                    );
                });
                return;
            }
        }

        if self.param_snapshots.is_empty() {
            ui.label(
                egui::RichText::new("No parameters exposed.")
                    .color(theme::TEXT_SECONDARY)
                    .italics(),
            );
            return;
        }

        ui.horizontal(|ui| {
            ui.label("🔍");
            ui.add(
                egui::TextEdit::singleline(&mut self.param_filter)
                    .hint_text("Filter parameters…")
                    .desired_width(ui.available_width()),
            );
        });
        ui.add_space(4.0);

        let filter_lower = self.param_filter.to_lowercase();
        let mut param_changes: Vec<(u32, f64)> = Vec::new();

        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                for snap in &self.param_snapshots {
                    if !filter_lower.is_empty()
                        && !snap.title.to_lowercase().contains(&filter_lower)
                    {
                        continue;
                    }
                    if snap.is_read_only {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(&snap.title).color(theme::TEXT_PRIMARY));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(&snap.display)
                                            .color(theme::TEXT_SECONDARY)
                                            .monospace(),
                                    );
                                },
                            );
                        });
                    } else {
                        let label_color = if snap.is_bypass {
                            theme::WARNING
                        } else {
                            theme::TEXT_PRIMARY
                        };

                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(&snap.title).color(label_color));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let unit_str = if snap.units.is_empty() {
                                        String::new()
                                    } else {
                                        format!(" {}", snap.units)
                                    };
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{}{}",
                                            snap.display, unit_str
                                        ))
                                        .color(theme::TEXT_SECONDARY)
                                        .monospace()
                                        .small(),
                                    );
                                },
                            );
                        });

                        let mut value = snap.value;
                        let slider = egui::Slider::new(&mut value, 0.0..=1.0)
                            .show_value(false)
                            .custom_formatter(|v, _| format!("{:.3}", v));

                        let response = ui.add(slider);
                        if response.changed() {
                            param_changes.push((snap.id, value));
                        }
                        if response.double_clicked() {
                            param_changes.push((snap.id, snap.default));
                        }
                    }
                    ui.add_space(2.0);
                }
            });

        for (id, value) in param_changes {
            if is_active {
                self.send_action(GuiAction::SetParameter { id, value });
            } else {
                self.send_action(GuiAction::StageParameter {
                    slot_index: idx,
                    id,
                    value,
                });
            }
        }
    }

    fn show_rack(&mut self, ui: &mut egui::Ui) {
        ui.heading("Plugin Rack");
        ui.add_space(8.0);

        if self.rack.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(80.0);
                ui.label(
                    egui::RichText::new("No plugins loaded.")
                        .color(theme::TEXT_SECONDARY)
                        .size(16.0),
                );
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("Use the browser on the left to add plugins to the rack.")
                        .color(theme::TEXT_DISABLED),
                );
            });
            return;
        }

        let mut remove_index: Option<usize> = None;
        let mut activate_index: Option<usize> = None;
        let mut deactivate = false;
        let mut open_editor = false;
        let mut new_selected: Option<usize> = self.selected_slot;
        let selected_slot = self.selected_slot;
        let active_slot = self.active_slot;
        let has_editor = self.has_editor;

        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                for (i, slot) in self.rack.iter_mut().enumerate() {
                    let is_selected = selected_slot == Some(i);
                    let is_active = active_slot == Some(i);

                    let frame = if is_active {
                        egui::Frame {
                            stroke: egui::Stroke::new(2.0, theme::SUCCESS),
                            ..theme::glass_card_frame()
                        }
                    } else if is_selected {
                        egui::Frame {
                            stroke: theme::accent_stroke(),
                            ..theme::glass_card_frame()
                        }
                    } else {
                        theme::glass_card_frame()
                    };

                    frame.show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("{:>2}", i + 1))
                                    .color(if is_active {
                                        theme::SUCCESS
                                    } else {
                                        theme::ACCENT_DIM
                                    })
                                    .monospace(),
                            );

                            ui.separator();

                            let resp = ui
                                .vertical(|ui| {
                                    ui.label(
                                        egui::RichText::new(&slot.name)
                                            .color(theme::TEXT_PRIMARY)
                                            .strong(),
                                    );
                                    let status_text = if is_active {
                                        format!("{} • active", slot.vendor)
                                    } else {
                                        slot.vendor.clone()
                                    };
                                    ui.label(
                                        egui::RichText::new(status_text)
                                            .color(theme::TEXT_SECONDARY)
                                            .small(),
                                    );
                                })
                                .response;

                            if resp.clicked() {
                                new_selected = Some(i);
                            }

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add(
                                            egui::Button::new("✕").fill(egui::Color32::TRANSPARENT),
                                        )
                                        .clicked()
                                    {
                                        remove_index = Some(i);
                                    }

                                    let bypass_label = if slot.bypassed { "🔇" } else { "🔊" };
                                    if ui.button(bypass_label).clicked() {
                                        slot.bypassed = !slot.bypassed;
                                    }

                                    if is_active {
                                        if has_editor
                                            && !self.safe_mode
                                            && ui
                                                .add(
                                                    egui::Button::new("🎹")
                                                        .fill(egui::Color32::TRANSPARENT),
                                                )
                                                .on_hover_text("Open plugin editor")
                                                .clicked()
                                        {
                                            open_editor = true;
                                        }

                                        if ui
                                            .add(
                                                egui::Button::new("⏹")
                                                    .fill(egui::Color32::TRANSPARENT),
                                            )
                                            .on_hover_text("Stop processing")
                                            .clicked()
                                        {
                                            deactivate = true;
                                        }
                                    } else if ui
                                        .add(
                                            egui::Button::new("▶").fill(egui::Color32::TRANSPARENT),
                                        )
                                        .on_hover_text("Activate and start processing")
                                        .clicked()
                                    {
                                        activate_index = Some(i);
                                    }
                                },
                            );
                        });
                    });

                    ui.add_space(4.0);
                }
            });

        // Handle selection change
        if new_selected != self.selected_slot {
            self.selected_slot = new_selected;
            self.send_action(GuiAction::SelectSlot {
                index: new_selected,
            });
        }

        if let Some(idx) = remove_index {
            self.send_action(GuiAction::RemoveFromRack { index: idx });
        }
        if deactivate {
            self.send_action(GuiAction::DeactivateSlot);
        }
        if let Some(idx) = activate_index {
            self.send_action(GuiAction::ActivateSlot { index: idx });
        }
        if open_editor {
            self.send_action(GuiAction::OpenEditor);
        }
    }
}

// ── Launch function ─────────────────────────────────────────────────────

/// Launch the GUI worker process. Connects to the supervisor and runs the eframe window.
///
/// This is called from the `gui-worker` CLI subcommand.
pub fn launch_worker(
    socket_path: &str,
    _safe_mode: bool,
    _malloc_debug: bool,
) -> anyhow::Result<()> {
    info!(socket = %socket_path, "GUI worker connecting to supervisor");

    let stream = UnixStream::connect(socket_path).map_err(|e| {
        anyhow::anyhow!(
            "Failed to connect to supervisor socket '{}': {}",
            socket_path,
            e
        )
    })?;

    // Set a reasonable read timeout
    stream
        .set_read_timeout(Some(std::time::Duration::from_millis(50)))
        .ok();

    info!("Connected to supervisor");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([1024.0, 640.0])
            .with_title("rs-vst-host"),
        ..Default::default()
    };

    eframe::run_native(
        "rs-vst-host",
        options,
        Box::new(move |_cc| Ok(Box::new(GuiWorkerApp::new(stream)))),
    )
    .map_err(|e| anyhow::anyhow!("GUI worker error: {}", e))
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vst3::types::PluginClassInfo;
    use std::path::PathBuf;

    #[test]
    fn test_gui_worker_app_default_state() {
        // Create a dummy socket pair for testing
        let (s1, _s2) = std::os::unix::net::UnixStream::pair().expect("socketpair");
        let app = GuiWorkerApp::new(s1);

        assert!(app.plugin_modules.is_empty());
        assert!(app.rack.is_empty());
        assert_eq!(app.selected_slot, None);
        assert_eq!(app.active_slot, None);
        assert!(app.param_snapshots.is_empty());
        assert!(!app.audio_status.running);
        assert!(!app.heap_corruption_detected);
        assert!(!app.has_editor);
        assert!(!app.tone_enabled);
        assert!(!app.safe_mode);
        assert!(app.browser_filter.is_empty());
        assert!(app.param_filter.is_empty());
        assert!(app.status_message.contains("Connecting"));
    }

    #[test]
    fn test_apply_full_state() {
        let (s1, _s2) = std::os::unix::net::UnixStream::pair().expect("socketpair");
        let mut app = GuiWorkerApp::new(s1);

        app.apply_update(SupervisorUpdate::FullState {
            plugin_modules: vec![],
            rack: vec![RackSlotState {
                name: "TestPlugin".into(),
                vendor: "TestVendor".into(),
                category: "Fx".into(),
                path: PathBuf::from("/test.vst3"),
                cid: [0u8; 16],
                bypassed: false,
                param_cache: Vec::new(),
                staged_changes: Vec::new(),
            }],
            selected_slot: Some(0),
            active_slot: Some(0),
            param_snapshots: vec![ParamSnapshot {
                id: 1,
                title: "Volume".into(),
                units: "dB".into(),
                value: 0.7,
                default: 0.5,
                display: "-3.0".into(),
                can_automate: true,
                is_read_only: false,
                is_bypass: false,
            }],
            audio_status: AudioStatusState {
                sample_rate: 44100,
                buffer_size: 512,
                device_name: "Built-in".into(),
                running: true,
            },
            audio_devices: vec![DeviceState {
                name: "Built-in".into(),
            }],
            midi_ports: vec![],
            selected_audio_device: Some("Built-in".into()),
            selected_midi_port: None,
            process_isolation: false,
            status_message: "Active".into(),
            heap_corruption_detected: false,
            has_editor: true,
            tainted_count: 0,
            transport: TransportUpdate {
                playing: true,
                tempo: 140.0,
                time_sig_num: 3,
                time_sig_den: 4,
            },
            tone_enabled: true,
            safe_mode: false,
        });

        assert_eq!(app.rack.len(), 1);
        assert_eq!(app.rack[0].name, "TestPlugin");
        assert_eq!(app.selected_slot, Some(0));
        assert_eq!(app.active_slot, Some(0));
        assert_eq!(app.param_snapshots.len(), 1);
        assert!(app.audio_status.running);
        assert!(app.has_editor);
        assert!(app.transport.playing);
        assert_eq!(app.transport.tempo, 140.0);
        assert!(app.tone_enabled);
        assert_eq!(app.status_message, "Active");
    }

    #[test]
    fn test_apply_incremental_updates() {
        let (s1, _s2) = std::os::unix::net::UnixStream::pair().expect("socketpair");
        let mut app = GuiWorkerApp::new(s1);

        app.apply_update(SupervisorUpdate::StatusMessage {
            message: "New status".into(),
        });
        assert_eq!(app.status_message, "New status");

        app.apply_update(SupervisorUpdate::HeapCorruptionDetected);
        assert!(app.heap_corruption_detected);

        app.apply_update(SupervisorUpdate::EditorAvailability { has_editor: true });
        assert!(app.has_editor);

        app.apply_update(SupervisorUpdate::AudioStatusUpdated {
            status: AudioStatusState {
                sample_rate: 48000,
                buffer_size: 256,
                device_name: "USB".into(),
                running: true,
            },
        });
        assert_eq!(app.audio_status.sample_rate, 48000);
        assert!(app.audio_status.running);
    }

    #[test]
    fn test_apply_rack_update() {
        let (s1, _s2) = std::os::unix::net::UnixStream::pair().expect("socketpair");
        let mut app = GuiWorkerApp::new(s1);

        app.apply_update(SupervisorUpdate::RackUpdated {
            rack: vec![
                RackSlotState {
                    name: "A".into(),
                    vendor: "V".into(),
                    category: "C".into(),
                    path: PathBuf::from("/a.vst3"),
                    cid: [0u8; 16],
                    bypassed: false,
                    param_cache: Vec::new(),
                    staged_changes: Vec::new(),
                },
                RackSlotState {
                    name: "B".into(),
                    vendor: "V".into(),
                    category: "C".into(),
                    path: PathBuf::from("/b.vst3"),
                    cid: [1u8; 16],
                    bypassed: true,
                    param_cache: Vec::new(),
                    staged_changes: Vec::new(),
                },
            ],
            active_slot: Some(0),
            selected_slot: Some(1),
        });

        assert_eq!(app.rack.len(), 2);
        assert_eq!(app.rack[0].name, "A");
        assert_eq!(app.rack[1].name, "B");
        assert!(app.rack[1].bypassed);
        assert_eq!(app.active_slot, Some(0));
        assert_eq!(app.selected_slot, Some(1));
    }

    #[test]
    fn test_apply_params_update() {
        let (s1, _s2) = std::os::unix::net::UnixStream::pair().expect("socketpair");
        let mut app = GuiWorkerApp::new(s1);

        app.apply_update(SupervisorUpdate::ParamsUpdated {
            snapshots: vec![ParamSnapshot {
                id: 1,
                title: "Freq".into(),
                units: "Hz".into(),
                value: 0.3,
                default: 0.5,
                display: "440".into(),
                can_automate: true,
                is_read_only: false,
                is_bypass: false,
            }],
        });

        assert_eq!(app.param_snapshots.len(), 1);
        assert_eq!(app.param_snapshots[0].title, "Freq");
    }

    #[test]
    fn test_apply_devices_update() {
        let (s1, _s2) = std::os::unix::net::UnixStream::pair().expect("socketpair");
        let mut app = GuiWorkerApp::new(s1);

        app.apply_update(SupervisorUpdate::DevicesUpdated {
            audio_devices: vec![
                DeviceState {
                    name: "Dev1".into(),
                },
                DeviceState {
                    name: "Dev2".into(),
                },
            ],
            midi_ports: vec![MidiPortState {
                name: "Port1".into(),
            }],
        });

        assert_eq!(app.audio_devices.len(), 2);
        assert_eq!(app.midi_ports.len(), 1);
    }

    #[test]
    fn test_filtered_classes_empty() {
        let (s1, _s2) = std::os::unix::net::UnixStream::pair().expect("socketpair");
        let app = GuiWorkerApp::new(s1);
        assert!(app.filtered_classes().is_empty());
    }

    #[test]
    fn test_filtered_classes_with_modules() {
        let (s1, _s2) = std::os::unix::net::UnixStream::pair().expect("socketpair");
        let mut app = GuiWorkerApp::new(s1);
        app.plugin_modules = vec![PluginModuleInfo {
            path: PathBuf::from("/test.vst3"),
            factory_vendor: Some("TestVendor".into()),
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
                    cid: [0u8; 16],
                },
                PluginClassInfo {
                    name: "EQ".into(),
                    category: "Audio Module Class".into(),
                    subcategories: Some("Fx|EQ".into()),
                    vendor: None,
                    version: None,
                    sdk_version: None,
                    cid: [1u8; 16],
                },
            ],
        }];

        // No filter — all classes
        assert_eq!(app.filtered_classes().len(), 2);

        // Filter by name
        app.browser_filter = "synth".into();
        let results = app.filtered_classes();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].3.name, "Synth");

        // Filter by subcategory
        app.browser_filter = "eq".into();
        let results = app.filtered_classes();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].3.name, "EQ");

        // No match
        app.browser_filter = "nonexistent".into();
        assert!(app.filtered_classes().is_empty());
    }

    #[test]
    fn test_transport_change_detection() {
        let (s1, s2) = std::os::unix::net::UnixStream::pair().expect("socketpair");
        s2.set_read_timeout(Some(std::time::Duration::from_millis(10)))
            .ok();
        let mut app = GuiWorkerApp::new(s1);

        // No change — should not send
        app.sync_transport();

        // Change tempo — should send
        app.transport.tempo = 140.0;
        app.sync_transport();
        assert_eq!(app.prev_transport.tempo, 140.0);

        // Change playing — should send
        app.transport.playing = true;
        app.sync_transport();
        assert!(app.prev_transport.playing);
    }

    #[test]
    fn test_send_action_to_paired_socket() {
        let (s1, s2) = std::os::unix::net::UnixStream::pair().expect("socketpair");
        s2.set_read_timeout(Some(std::time::Duration::from_millis(100)))
            .ok();

        let mut app = GuiWorkerApp::new(s1);
        app.send_action(GuiAction::Ping);

        // Read the action from the other end
        let mut reader = s2;
        let decoded: Option<GuiAction> = decode(&mut reader).expect("decode");
        assert!(decoded.is_some());
        match decoded.unwrap() {
            GuiAction::Ping => {}
            other => panic!("Expected Ping, got {:?}", other),
        }
    }

    // ── Supervisor disconnection tests ──────────────────────────────────

    #[test]
    fn test_supervisor_disconnected_default_false() {
        let (s1, _s2) = std::os::unix::net::UnixStream::pair().expect("socketpair");
        let app = GuiWorkerApp::new(s1);
        assert!(!app.supervisor_disconnected);
    }

    #[test]
    fn test_mark_supervisor_disconnected() {
        let (s1, _s2) = std::os::unix::net::UnixStream::pair().expect("socketpair");
        let mut app = GuiWorkerApp::new(s1);

        app.active_slot = Some(0);
        app.audio_status.running = true;

        app.mark_supervisor_disconnected();

        assert!(app.supervisor_disconnected);
        assert_eq!(app.active_slot, None);
        assert!(!app.audio_status.running);
        assert!(app.status_message.contains("Supervisor process died"));
    }

    #[test]
    fn test_mark_supervisor_disconnected_idempotent() {
        let (s1, _s2) = std::os::unix::net::UnixStream::pair().expect("socketpair");
        let mut app = GuiWorkerApp::new(s1);

        app.mark_supervisor_disconnected();
        let msg1 = app.status_message.clone();
        app.mark_supervisor_disconnected(); // second call should be no-op
        assert_eq!(app.status_message, msg1);
    }

    #[test]
    fn test_send_action_noop_when_disconnected() {
        let (s1, s2) = std::os::unix::net::UnixStream::pair().expect("socketpair");
        s2.set_read_timeout(Some(std::time::Duration::from_millis(50)))
            .ok();
        let mut app = GuiWorkerApp::new(s1);

        app.supervisor_disconnected = true;
        app.send_action(GuiAction::Ping);

        // Nothing should have been sent — read should timeout
        let mut reader = s2;
        let result = decode::<GuiAction>(&mut reader);
        assert!(
            result.is_err(),
            "Expected timeout/no data when disconnected"
        );
    }

    #[test]
    fn test_send_action_detects_broken_pipe() {
        let (s1, s2) = std::os::unix::net::UnixStream::pair().expect("socketpair");
        let mut app = GuiWorkerApp::new(s1);

        // Close the remote end to trigger broken pipe
        drop(s2);

        app.send_action(GuiAction::Ping);
        assert!(
            app.supervisor_disconnected,
            "Should detect broken pipe and mark disconnected"
        );
    }

    #[test]
    fn test_poll_updates_detects_eof() {
        let (s1, s2) = std::os::unix::net::UnixStream::pair().expect("socketpair");
        let mut app = GuiWorkerApp::new(s1);

        // Close the remote end to trigger EOF
        drop(s2);

        app.poll_updates();
        assert!(
            app.supervisor_disconnected,
            "Should detect EOF and mark disconnected"
        );
    }

    #[test]
    fn test_poll_updates_noop_when_disconnected() {
        let (s1, _s2) = std::os::unix::net::UnixStream::pair().expect("socketpair");
        let mut app = GuiWorkerApp::new(s1);

        app.supervisor_disconnected = true;
        // Should return immediately without trying to read
        app.poll_updates();
        assert!(app.supervisor_disconnected);
    }

    #[test]
    fn test_apply_audio_process_restarted() {
        let (s1, _s2) = std::os::unix::net::UnixStream::pair().expect("socketpair");
        let mut app = GuiWorkerApp::new(s1);
        app.active_slot = Some(0);
        app.has_editor = true;
        app.audio_status = AudioStatusState {
            sample_rate: 48000,
            buffer_size: 256,
            device_name: "Test".into(),
            running: true,
        };
        app.param_snapshots = vec![ParamSnapshot {
            id: 1,
            title: "Vol".into(),
            units: "dB".into(),
            value: 0.5,
            default: 0.5,
            display: "0.5".into(),
            can_automate: true,
            is_read_only: false,
            is_bypass: false,
        }];

        app.apply_update(SupervisorUpdate::AudioProcessRestarted {
            message: "Audio crashed and was restarted".into(),
            restart_count: 1,
        });

        assert_eq!(app.active_slot, None);
        assert!(app.param_snapshots.is_empty());
        assert!(!app.has_editor);
        assert!(!app.audio_status.running);
        assert!(app.status_message.contains("Audio crashed"));
    }

    #[test]
    fn test_send_shutdown_action_on_close() {
        // Verify that GuiAction::Shutdown can be sent and received over the socket,
        // which is what happens when the window close is detected.
        let (s1, s2) = std::os::unix::net::UnixStream::pair().expect("socketpair");
        s2.set_read_timeout(Some(std::time::Duration::from_secs(1)))
            .ok();
        let mut app = GuiWorkerApp::new(s1);

        // Simulate what happens when close_requested() is detected
        app.send_action(GuiAction::Shutdown);

        // The supervisor should receive the Shutdown action
        let mut reader = s2;
        let result = decode::<GuiAction>(&mut reader).expect("decode");
        match result {
            Some(GuiAction::Shutdown) => {} // expected
            other => panic!("Expected Shutdown, got {:?}", other),
        }
    }
}
