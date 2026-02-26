//! Main GUI application — the `HostApp` `eframe::App` implementation.
//!
//! Provides the plugin browser, plugin rack, transport controls,
//! parameter view, device selection, and session save/load in a
//! Liquid Glass styled window. Integrates with the [`super::backend::HostBackend`]
//! for live audio processing and plugin management.

use crate::gui::backend::{HostBackend, ParamSnapshot};
use crate::gui::session::Session;
use crate::gui::theme;
use crate::vst3::{cache, scanner, types::PluginClassInfo, types::PluginModuleInfo};

use eframe::egui;
use std::path::PathBuf;

// ── Data Structures ─────────────────────────────────────────────────────────

/// Represents one loaded plugin "slot" in the rack.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PluginSlot {
    /// Display name.
    pub name: String,
    /// Vendor.
    pub vendor: String,
    /// Category string.
    pub category: String,
    /// Path to the .vst3 bundle.
    pub path: PathBuf,
    /// Class ID (for instantiation).
    pub cid: [u8; 16],
    /// Whether the slot is bypassed.
    pub bypassed: bool,
}

/// Transport state tracked by the GUI.
#[derive(Debug, Clone)]
pub struct TransportState {
    /// Whether playback is active.
    pub playing: bool,
    /// Tempo in BPM.
    pub tempo: f64,
    /// Numerator of the time signature.
    pub time_sig_num: u32,
    /// Denominator of the time signature.
    pub time_sig_den: u32,
}

impl Default for TransportState {
    fn default() -> Self {
        Self {
            playing: false,
            tempo: 120.0,
            time_sig_num: 4,
            time_sig_den: 4,
        }
    }
}

/// Filter text for the plugin browser search.
#[derive(Debug, Default, Clone)]
pub struct BrowserFilter {
    pub text: String,
}

/// Which bottom-bar tab is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BottomTab {
    Transport,
    Devices,
    Session,
}

impl Default for BottomTab {
    fn default() -> Self {
        Self::Transport
    }
}

/// Top-level application state.
pub struct HostApp {
    /// Cached plugin modules from last scan.
    pub plugin_modules: Vec<PluginModuleInfo>,
    /// Plugin slots in the rack.
    pub rack: Vec<PluginSlot>,
    /// Transport state.
    pub transport: TransportState,
    /// Browser search filter.
    pub browser_filter: BrowserFilter,
    /// Status / log message shown at the bottom.
    pub status_message: String,
    /// Whether the theme has been applied.
    theme_applied: bool,
    /// Currently selected plugin index in the rack (for parameter view).
    pub selected_slot: Option<usize>,
    /// Audio/MIDI backend.
    pub backend: HostBackend,
    /// Parameter snapshots for the active plugin (refreshed each frame).
    pub param_snapshots: Vec<ParamSnapshot>,
    /// Whether the test tone is enabled.
    pub tone_enabled: bool,
    /// Active bottom-bar tab.
    pub bottom_tab: BottomTab,
    /// Session file path text field.
    pub session_path: String,
    /// Parameter search filter text.
    pub param_filter: String,
    /// Whether safe mode was requested (no plugins from cache).
    pub safe_mode: bool,
    /// Previous transport state for change detection.
    prev_tempo: f64,
    /// Previous time signature numerator.
    prev_time_sig_num: u32,
    /// Previous time signature denominator.
    prev_time_sig_den: u32,
    /// Previous playing state.
    prev_playing: bool,
}

impl HostApp {
    /// Create a new HostApp, optionally in safe mode.
    pub fn with_safe_mode(safe_mode: bool) -> Self {
        // Attempt to load cached plugins (unless safe mode)
        let plugin_modules = if safe_mode {
            Vec::new()
        } else {
            cache::load()
                .ok()
                .flatten()
                .map(|c| c.modules)
                .unwrap_or_default()
        };

        let status = if safe_mode {
            "Safe mode — no plugins loaded. Click 'Scan' to discover VST3 plugins.".into()
        } else if plugin_modules.is_empty() {
            "No plugins cached. Click 'Scan' to discover VST3 plugins.".into()
        } else {
            let total: usize = plugin_modules.iter().map(|m| m.classes.len()).sum();
            format!("{} plugin class(es) loaded from cache.", total)
        };

        let default_session_path = super::session::sessions_dir()
            .map(|d| d.join("default.json").to_string_lossy().to_string())
            .unwrap_or_else(|| "session.json".into());

        let transport = TransportState::default();

        Self {
            prev_tempo: transport.tempo,
            prev_time_sig_num: transport.time_sig_num,
            prev_time_sig_den: transport.time_sig_den,
            prev_playing: transport.playing,
            plugin_modules,
            rack: Vec::new(),
            transport,
            browser_filter: BrowserFilter::default(),
            status_message: status,
            theme_applied: false,
            selected_slot: None,
            backend: HostBackend::new(),
            param_snapshots: Vec::new(),
            tone_enabled: false,
            bottom_tab: BottomTab::default(),
            session_path: default_session_path,
            param_filter: String::new(),
            safe_mode,
        }
    }
}

impl Default for HostApp {
    fn default() -> Self {
        Self::with_safe_mode(false)
    }
}

impl HostApp {
    /// Run a plugin scan and refresh the module list.
    pub fn scan_plugins(&mut self) {
        self.status_message = "Scanning for plugins…".into();

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
                    tracing::warn!(path = %bundle_path.display(), error = %e, "scan failed");
                }
            }
        }

        // Save to cache
        let scan_cache = cache::ScanCache::new(modules.clone());
        if let Err(e) = cache::save(&scan_cache) {
            tracing::warn!(error = %e, "cache save failed");
        }

        let class_count: usize = modules.iter().map(|m| m.classes.len()).sum();
        let module_count = modules.len();
        self.plugin_modules = modules;

        let error_str = if error_count > 0 {
            format!(", {} error(s)", error_count)
        } else {
            String::new()
        };
        self.status_message = format!(
            "Scan complete — {} module(s), {} class(es){}.",
            module_count, class_count, error_str
        );
    }

    /// Add a plugin class to the rack.
    pub fn add_to_rack(&mut self, module: &PluginModuleInfo, class: &PluginClassInfo) {
        let vendor = class
            .vendor
            .as_deref()
            .or(module.factory_vendor.as_deref())
            .unwrap_or("Unknown")
            .to_string();

        let slot = PluginSlot {
            name: class.name.clone(),
            vendor,
            category: class.category.clone(),
            path: module.path.clone(),
            cid: class.cid,
            bypassed: false,
        };

        self.status_message = format!("Added '{}' to the rack.", slot.name);
        self.rack.push(slot);
    }

    /// Remove a plugin slot by index.
    pub fn remove_from_rack(&mut self, index: usize) {
        if index < self.rack.len() {
            let name = self.rack[index].name.clone();

            // If the removed slot was active, deactivate it
            if self.backend.active_slot_index() == Some(index) {
                self.backend.deactivate_plugin();
                self.param_snapshots.clear();
            }

            self.rack.remove(index);
            if self.selected_slot == Some(index) {
                self.selected_slot = None;
            } else if let Some(sel) = self.selected_slot {
                if sel > index {
                    self.selected_slot = Some(sel - 1);
                }
            }
            self.status_message = format!("Removed '{}' from the rack.", name);
        }
    }

    /// Activate a plugin slot, starting audio processing through the backend.
    pub fn activate_slot(&mut self, index: usize) {
        if index >= self.rack.len() {
            return;
        }

        let slot = &self.rack[index];
        let path = slot.path.clone();
        let cid = slot.cid;
        let name = slot.name.clone();

        match self.backend.activate_plugin(index, &path, &cid, &name) {
            Ok(snapshots) => {
                self.param_snapshots = snapshots;
                self.selected_slot = Some(index);
                self.status_message = format!("▶ '{}' active — processing audio.", name);
            }
            Err(e) => {
                self.status_message = format!("✗ Failed to activate '{}': {}", name, e);
                tracing::error!(plugin = %name, error = %e, "activation failed");
            }
        }
    }

    /// Deactivate the currently active plugin.
    pub fn deactivate_active(&mut self) {
        self.backend.deactivate_plugin();
        self.param_snapshots.clear();
        self.status_message = "Plugin deactivated.".into();
    }

    /// Refresh parameter snapshots from the backend.
    pub fn refresh_params(&mut self) {
        if self.backend.is_active() {
            // Apply plugin-initiated param changes first
            let handler_changes = self.backend.drain_handler_changes();
            for (id, value) in handler_changes {
                // Update our snapshots to reflect plugin-initiated changes
                if let Some(snap) = self.param_snapshots.iter_mut().find(|s| s.id == id) {
                    snap.value = value;
                    if let Some(display) = self.backend.param_value_string(id, value) {
                        snap.display = display;
                    }
                }
            }

            // Periodically refresh all snapshots (handles display strings etc.)
            self.param_snapshots = self.backend.active_param_snapshots();
        }
    }

    /// Sync GUI transport state changes to the audio engine.
    pub fn sync_transport(&mut self) {
        if !self.backend.is_active() {
            return;
        }

        if self.transport.tempo != self.prev_tempo {
            self.backend.set_tempo(self.transport.tempo);
            self.prev_tempo = self.transport.tempo;
        }

        if self.transport.time_sig_num != self.prev_time_sig_num
            || self.transport.time_sig_den != self.prev_time_sig_den
        {
            self.backend
                .set_time_signature(self.transport.time_sig_num, self.transport.time_sig_den);
            self.prev_time_sig_num = self.transport.time_sig_num;
            self.prev_time_sig_den = self.transport.time_sig_den;
        }

        if self.transport.playing != self.prev_playing {
            self.backend.set_playing(self.transport.playing);
            self.prev_playing = self.transport.playing;
        }
    }

    /// Open the native editor window for the active plugin.
    pub fn open_editor(&mut self) {
        let Some(idx) = self.selected_slot else {
            self.status_message = "No plugin selected.".into();
            return;
        };

        let name = self
            .rack
            .get(idx)
            .map(|s| s.name.clone())
            .unwrap_or_default();

        match self.backend.open_editor(&name) {
            Ok(()) => {
                self.status_message = format!("🎹 Editor opened for '{}'.", name);
            }
            Err(e) => {
                self.status_message = format!("✗ Editor failed: {}", e);
                tracing::warn!(plugin = %name, error = %e, "editor open failed");
            }
        }
    }

    /// Save the current session to the configured path.
    pub fn save_session(&mut self) {
        let session = Session::capture(
            &self.transport,
            &self.rack,
            self.backend.selected_audio_device.clone(),
            self.backend.selected_midi_port.clone(),
        );
        let path = PathBuf::from(&self.session_path);
        match session.save_to_file(&path) {
            Ok(()) => {
                self.status_message = format!("Session saved to {}", path.display());
            }
            Err(e) => {
                self.status_message = format!("✗ Save failed: {}", e);
            }
        }
    }

    /// Load a session from the configured path.
    pub fn load_session(&mut self) {
        let path = PathBuf::from(&self.session_path);
        match Session::load_from_file(&path) {
            Ok(session) => {
                // Deactivate any active plugin
                self.backend.deactivate_plugin();
                self.param_snapshots.clear();

                // Restore state
                let (transport, rack) = session.restore();
                self.transport = transport;
                self.rack = rack;
                self.selected_slot = None;

                // Restore device selections
                self.backend.selected_audio_device = session.audio_device;
                self.backend.selected_midi_port = session.midi_port;

                self.status_message =
                    format!("Session loaded from {} ({} slots)", path.display(), self.rack.len());
            }
            Err(e) => {
                self.status_message = format!("✗ Load failed: {}", e);
            }
        }
    }

    /// Return the flat list of (module_info, class_info) matching the current filter.
    pub fn filtered_classes(&self) -> Vec<(&PluginModuleInfo, &PluginClassInfo)> {
        let filter = self.browser_filter.text.to_lowercase();
        let mut results = Vec::new();
        for module in &self.plugin_modules {
            for class in &module.classes {
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
                    results.push((module, class));
                }
            }
        }
        results
    }
}

// ── eframe::App Implementation ──────────────────────────────────────────────

impl eframe::App for HostApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Apply theme once
        if !self.theme_applied {
            theme::apply(ctx);
            self.theme_applied = true;
        }

        // Refresh parameters each frame if a plugin is active
        self.refresh_params();

        // Sync transport state changes to the audio engine
        self.sync_transport();

        // Poll editor windows for resize requests and prune closed ones
        self.backend.poll_editors();

        // Keyboard shortcuts
        ctx.input(|input| {
            if input.key_pressed(egui::Key::Space) {
                self.transport.playing = !self.transport.playing;
            }
        });

        // — Left side panel: Plugin Browser —
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

        // — Right side panel: Parameter View —
        if self.selected_slot.is_some() && !self.param_snapshots.is_empty() {
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

        // — Bottom panel: Transport / Devices / Session + Status —
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

        // — Central panel: Plugin Rack —
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

// ── Panel Renderers ─────────────────────────────────────────────────────────

impl HostApp {
    /// Render the left-side plugin browser panel.
    fn show_browser(&mut self, ui: &mut egui::Ui) {
        ui.heading("Plugin Browser");
        ui.add_space(8.0);

        // Scan button
        if ui
            .add(egui::Button::new("⟳  Scan Plugins").min_size(egui::vec2(ui.available_width(), 28.0)))
            .clicked()
        {
            self.scan_plugins();
        }

        ui.add_space(8.0);

        // Search filter
        ui.horizontal(|ui| {
            ui.label("🔍");
            ui.add(
                egui::TextEdit::singleline(&mut self.browser_filter.text)
                    .hint_text("Filter…")
                    .desired_width(ui.available_width()),
            );
        });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        // Plugin list — collect owned copies to avoid borrow conflict
        let classes: Vec<(PluginModuleInfo, PluginClassInfo)> = self
            .filtered_classes()
            .into_iter()
            .map(|(m, c)| (m.clone(), c.clone()))
            .collect();

        if classes.is_empty() {
            ui.label(
                egui::RichText::new("No plugins found.")
                    .color(theme::TEXT_SECONDARY)
                    .italics(),
            );
        } else {
            // Track pending add action outside the scroll area
            let mut add_action: Option<(PluginModuleInfo, PluginClassInfo)> = None;

            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    for (module, class) in &classes {
                        let vendor = class
                            .vendor
                            .as_deref()
                            .or(module.factory_vendor.as_deref())
                            .unwrap_or("Unknown");

                        let subcats = class
                            .subcategories
                            .as_deref()
                            .unwrap_or("");

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
                                            add_action = Some((
                                                module.clone(),
                                                class.clone(),
                                            ));
                                        }
                                    },
                                );
                            });
                        });

                        ui.add_space(2.0);
                    }
                });

            // Apply deferred add (after borrow of classes ends)
            if let Some((module, class)) = add_action {
                self.add_to_rack(&module, &class);
            }
        }
    }

    /// Render the bottom bar with tabbed views: Transport, Devices, Session.
    fn show_bottom_bar(&mut self, ui: &mut egui::Ui) {
        // Tab selector row
        ui.horizontal(|ui| {
            if ui
                .selectable_label(self.bottom_tab == BottomTab::Transport, "🎵 Transport")
                .clicked()
            {
                self.bottom_tab = BottomTab::Transport;
            }
            if ui
                .selectable_label(self.bottom_tab == BottomTab::Devices, "🔊 Devices")
                .clicked()
            {
                self.bottom_tab = BottomTab::Devices;
            }
            if ui
                .selectable_label(self.bottom_tab == BottomTab::Session, "💾 Session")
                .clicked()
            {
                self.bottom_tab = BottomTab::Session;
            }

            // Status at the right edge
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(&self.status_message)
                        .color(theme::TEXT_SECONDARY)
                        .small(),
                );
            });
        });

        ui.separator();

        // Tab content
        match self.bottom_tab {
            BottomTab::Transport => self.show_transport_content(ui),
            BottomTab::Devices => self.show_devices_content(ui),
            BottomTab::Session => self.show_session_content(ui),
        }
    }

    /// Render transport controls (play/pause, tempo, time sig).
    fn show_transport_content(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            // Play / Pause
            let play_label = if self.transport.playing { "⏸" } else { "▶" };
            if ui.button(play_label).clicked() {
                self.transport.playing = !self.transport.playing;
            }

            ui.separator();

            // Tempo
            ui.label("BPM");
            ui.add(
                egui::DragValue::new(&mut self.transport.tempo)
                    .range(20.0..=300.0)
                    .speed(0.5)
                    .fixed_decimals(1),
            );

            ui.separator();

            // Time signature
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

            // Test tone toggle
            let tone_label = if self.tone_enabled {
                "🔔 Tone On"
            } else {
                "🔕 Tone Off"
            };
            if ui.button(tone_label).clicked() {
                self.tone_enabled = !self.tone_enabled;
                self.backend.set_tone_enabled(self.tone_enabled);
            }

            // Audio engine status (right-aligned)
            let status = &self.backend.audio_status;
            if status.running {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let editors_open = self.backend.editor_count();
                    let editor_str = if editors_open > 0 {
                        format!(" | {} editor(s)", editors_open)
                    } else {
                        String::new()
                    };
                    ui.label(
                        egui::RichText::new(format!(
                            "{} Hz • {} frames • {}{}",
                            status.sample_rate,
                            status.buffer_size,
                            status.device_name,
                            editor_str,
                        ))
                        .color(theme::TEXT_DISABLED)
                        .small(),
                    );
                });
            }
        });
    }

    /// Render device selection controls.
    fn show_devices_content(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            // Audio output device
            ui.label("Audio Output:");
            let current_audio = self
                .backend
                .selected_audio_device
                .clone()
                .unwrap_or_else(|| "(default)".into());

            egui::ComboBox::from_id_salt("audio_device_combo")
                .selected_text(&current_audio)
                .width(250.0)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(
                            self.backend.selected_audio_device.is_none(),
                            "(default)",
                        )
                        .clicked()
                    {
                        self.backend.selected_audio_device = None;
                    }
                    let devices = self.backend.audio_devices.clone();
                    for dev in &devices {
                        if ui
                            .selectable_label(
                                self.backend.selected_audio_device.as_deref() == Some(&dev.name),
                                &dev.name,
                            )
                            .clicked()
                        {
                            self.backend.selected_audio_device = Some(dev.name.clone());
                        }
                    }
                });

            ui.separator();

            // MIDI input port
            ui.label("MIDI Input:");
            let current_midi = self
                .backend
                .selected_midi_port
                .clone()
                .unwrap_or_else(|| "(none)".into());

            egui::ComboBox::from_id_salt("midi_port_combo")
                .selected_text(&current_midi)
                .width(250.0)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(self.backend.selected_midi_port.is_none(), "(none)")
                        .clicked()
                    {
                        self.backend.selected_midi_port = None;
                    }
                    let ports = self.backend.midi_ports.clone();
                    for port in &ports {
                        if ui
                            .selectable_label(
                                self.backend.selected_midi_port.as_deref() == Some(&port.name),
                                &port.name,
                            )
                            .clicked()
                        {
                            self.backend.selected_midi_port = Some(port.name.clone());
                        }
                    }
                });

            ui.separator();

            if ui.button("⟳ Refresh").clicked() {
                self.backend.refresh_devices();
                self.status_message = format!(
                    "Devices refreshed — {} audio, {} MIDI",
                    self.backend.audio_devices.len(),
                    self.backend.midi_ports.len()
                );
            }
        });
    }

    /// Render session save/load controls.
    fn show_session_content(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Path:");
            ui.add(
                egui::TextEdit::singleline(&mut self.session_path)
                    .hint_text("session.json")
                    .desired_width(400.0),
            );

            if ui.button("💾 Save").clicked() {
                self.save_session();
            }
            if ui.button("📂 Load").clicked() {
                self.load_session();
            }
        });
    }

    /// Render the right-side parameter view panel.
    fn show_param_panel(&mut self, ui: &mut egui::Ui) {
        let slot_name = self
            .selected_slot
            .and_then(|i| self.rack.get(i))
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "Parameters".into());

        ui.heading(format!("🎛 {}", slot_name));
        ui.add_space(4.0);

        let is_active = self
            .selected_slot
            .map(|i| self.backend.active_slot_index() == Some(i))
            .unwrap_or(false);

        if !is_active {
            ui.label(
                egui::RichText::new("Plugin is not active. Click ▶ to activate.")
                    .color(theme::TEXT_SECONDARY)
                    .italics(),
            );
            return;
        }

        if self.param_snapshots.is_empty() {
            ui.label(
                egui::RichText::new("No parameters exposed.")
                    .color(theme::TEXT_SECONDARY)
                    .italics(),
            );
            return;
        }

        // Parameter search filter
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

        // Collect parameter changes to apply after iteration
        let mut param_changes: Vec<(u32, f64)> = Vec::new();

        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                for snap in &self.param_snapshots {
                    // Apply filter
                    if !filter_lower.is_empty()
                        && !snap.title.to_lowercase().contains(&filter_lower)
                    {
                        continue;
                    }
                    if snap.is_read_only {
                        // Read-only: just display the value
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(&snap.title)
                                    .color(theme::TEXT_PRIMARY),
                            );
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
                        // Editable: slider
                        let label_color = if snap.is_bypass {
                            theme::WARNING
                        } else {
                            theme::TEXT_PRIMARY
                        };

                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(&snap.title).color(label_color),
                            );
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

                        // Double-click to reset to default
                        if response.double_clicked() {
                            param_changes.push((snap.id, snap.default));
                        }
                    }

                    ui.add_space(2.0);
                }
            });

        // Apply deferred parameter changes
        for (id, value) in param_changes {
            match self.backend.set_parameter(id, value) {
                Ok(_actual) => {}
                Err(e) => {
                    tracing::warn!(param_id = id, error = %e, "param set failed");
                }
            }
        }
    }

    /// Render the central plugin rack.
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
        let active_slot = self.backend.active_slot_index();
        let has_editor = self.backend.active_has_editor();

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
                            // Slot number badge
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

                            // Plugin info (clickable to select)
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

                            // Right side controls
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    // Remove button
                                    if ui
                                        .add(
                                            egui::Button::new("✕")
                                                .fill(egui::Color32::TRANSPARENT),
                                        )
                                        .clicked()
                                    {
                                        remove_index = Some(i);
                                    }

                                    // Bypass toggle
                                    let bypass_label = if slot.bypassed { "🔇" } else { "🔊" };
                                    if ui.button(bypass_label).clicked() {
                                        slot.bypassed = !slot.bypassed;
                                    }

                                    // Activate / Deactivate button
                                    if is_active {
                                        // Editor button (if plugin has an editor, and not in safe mode)
                                        if has_editor && !self.safe_mode {
                                            if ui
                                                .add(
                                                    egui::Button::new("🎹")
                                                        .fill(egui::Color32::TRANSPARENT),
                                                )
                                                .on_hover_text("Open plugin editor")
                                                .clicked()
                                            {
                                                open_editor = true;
                                            }
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
                                            egui::Button::new("▶")
                                                .fill(egui::Color32::TRANSPARENT),
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

        self.selected_slot = new_selected;

        if let Some(idx) = remove_index {
            self.remove_from_rack(idx);
        }
        if deactivate {
            self.deactivate_active();
        }
        if let Some(idx) = activate_index {
            self.activate_slot(idx);
        }
        if open_editor {
            self.open_editor();
        }
    }
}

// ── Launch ──────────────────────────────────────────────────────────────────

/// Launch the GUI window. This blocks until the window is closed.
///
/// When `safe_mode` is true, plugin editors are disabled (only parameter
/// sliders are shown) to avoid potential crashes from misbehaving plugins.
pub fn launch(safe_mode: bool) -> anyhow::Result<()> {
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
        Box::new(move |_cc| Ok(Box::new(HostApp::with_safe_mode(safe_mode)))),
    )
    .map_err(|e| anyhow::anyhow!("GUI error: {}", e))
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_module() -> PluginModuleInfo {
        PluginModuleInfo {
            path: PathBuf::from("/Library/Audio/Plug-Ins/VST3/TestPlugin.vst3"),
            factory_vendor: Some("TestVendor".into()),
            factory_url: None,
            factory_email: None,
            classes: vec![
                PluginClassInfo {
                    name: "TestSynth".into(),
                    category: "Audio Module Class".into(),
                    subcategories: Some("Instrument|Synth".into()),
                    vendor: None,
                    version: Some("1.0.0".into()),
                    sdk_version: None,
                    cid: [0u8; 16],
                },
                PluginClassInfo {
                    name: "TestEQ".into(),
                    category: "Audio Module Class".into(),
                    subcategories: Some("Fx|EQ".into()),
                    vendor: Some("OtherVendor".into()),
                    version: None,
                    sdk_version: None,
                    cid: [1u8; 16],
                },
            ],
        }
    }

    #[test]
    fn test_transport_default() {
        let t = TransportState::default();
        assert!(!t.playing);
        assert_eq!(t.tempo, 120.0);
        assert_eq!(t.time_sig_num, 4);
        assert_eq!(t.time_sig_den, 4);
    }

    #[test]
    fn test_host_app_default_empty() {
        let app = HostApp::default();
        assert!(app.rack.is_empty());
        assert!(app.selected_slot.is_none());
        assert!(!app.backend.is_active());
        assert!(app.param_snapshots.is_empty());
        assert!(!app.tone_enabled);
        assert_eq!(app.bottom_tab, BottomTab::Transport);
    }

    #[test]
    fn test_add_to_rack() {
        let mut app = HostApp::default();
        let module = sample_module();
        let class = &module.classes[0];
        app.add_to_rack(&module, class);

        assert_eq!(app.rack.len(), 1);
        assert_eq!(app.rack[0].name, "TestSynth");
        assert_eq!(app.rack[0].vendor, "TestVendor");
        assert!(!app.rack[0].bypassed);
    }

    #[test]
    fn test_add_to_rack_own_vendor() {
        let mut app = HostApp::default();
        let module = sample_module();
        let class = &module.classes[1];
        app.add_to_rack(&module, class);

        assert_eq!(app.rack.len(), 1);
        assert_eq!(app.rack[0].vendor, "OtherVendor");
    }

    #[test]
    fn test_remove_from_rack() {
        let mut app = HostApp::default();
        let module = sample_module();
        app.add_to_rack(&module, &module.classes[0]);
        app.add_to_rack(&module, &module.classes[1]);
        assert_eq!(app.rack.len(), 2);

        app.remove_from_rack(0);
        assert_eq!(app.rack.len(), 1);
        assert_eq!(app.rack[0].name, "TestEQ");
    }

    #[test]
    fn test_remove_from_rack_invalid_index() {
        let mut app = HostApp::default();
        app.remove_from_rack(99);
        assert!(app.rack.is_empty());
    }

    #[test]
    fn test_selected_slot_clears_on_remove() {
        let mut app = HostApp::default();
        let module = sample_module();
        app.add_to_rack(&module, &module.classes[0]);
        app.add_to_rack(&module, &module.classes[1]);
        app.selected_slot = Some(0);

        app.remove_from_rack(0);
        assert_eq!(app.selected_slot, None);
    }

    #[test]
    fn test_selected_slot_adjusts_index() {
        let mut app = HostApp::default();
        let module = sample_module();
        app.add_to_rack(&module, &module.classes[0]);
        app.add_to_rack(&module, &module.classes[1]);
        app.selected_slot = Some(1);

        app.remove_from_rack(0);
        assert_eq!(app.selected_slot, Some(0));
    }

    #[test]
    fn test_filtered_classes_empty_filter() {
        let mut app = HostApp::default();
        app.plugin_modules = vec![sample_module()];
        let results = app.filtered_classes();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_filtered_classes_by_name() {
        let mut app = HostApp::default();
        app.plugin_modules = vec![sample_module()];
        app.browser_filter.text = "synth".into();
        let results = app.filtered_classes();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.name, "TestSynth");
    }

    #[test]
    fn test_filtered_classes_by_subcategory() {
        let mut app = HostApp::default();
        app.plugin_modules = vec![sample_module()];
        app.browser_filter.text = "eq".into();
        let results = app.filtered_classes();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.name, "TestEQ");
    }

    #[test]
    fn test_filtered_classes_by_vendor() {
        let mut app = HostApp::default();
        app.plugin_modules = vec![sample_module()];
        app.browser_filter.text = "othervendor".into();
        let results = app.filtered_classes();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.name, "TestEQ");
    }

    #[test]
    fn test_filtered_classes_factory_vendor_fallback() {
        let mut app = HostApp::default();
        app.plugin_modules = vec![sample_module()];
        app.browser_filter.text = "testvendor".into();
        let results = app.filtered_classes();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.name, "TestSynth");
    }

    #[test]
    fn test_filtered_classes_no_match() {
        let mut app = HostApp::default();
        app.plugin_modules = vec![sample_module()];
        app.browser_filter.text = "nonexistent".into();
        let results = app.filtered_classes();
        assert!(results.is_empty());
    }

    #[test]
    fn test_plugin_slot_bypass_toggle() {
        let mut slot = PluginSlot {
            name: "Test".into(),
            vendor: "V".into(),
            category: "C".into(),
            path: PathBuf::from("/test"),
            cid: [0u8; 16],
            bypassed: false,
        };
        assert!(!slot.bypassed);
        slot.bypassed = true;
        assert!(slot.bypassed);
    }

    #[test]
    fn test_browser_filter_default() {
        let f = BrowserFilter::default();
        assert!(f.text.is_empty());
    }

    #[test]
    fn test_status_message_after_add() {
        let mut app = HostApp::default();
        let module = sample_module();
        app.add_to_rack(&module, &module.classes[0]);
        assert!(app.status_message.contains("TestSynth"));
    }

    #[test]
    fn test_status_message_after_remove() {
        let mut app = HostApp::default();
        let module = sample_module();
        app.add_to_rack(&module, &module.classes[0]);
        app.remove_from_rack(0);
        assert!(app.status_message.contains("TestSynth"));
        assert!(app.status_message.contains("Removed"));
    }

    #[test]
    fn test_multiple_adds_increments_rack() {
        let mut app = HostApp::default();
        let module = sample_module();
        app.add_to_rack(&module, &module.classes[0]);
        app.add_to_rack(&module, &module.classes[0]);
        app.add_to_rack(&module, &module.classes[1]);
        assert_eq!(app.rack.len(), 3);
    }

    #[test]
    fn test_bottom_tab_default() {
        let tab = BottomTab::default();
        assert_eq!(tab, BottomTab::Transport);
    }

    #[test]
    fn test_bottom_tab_variants() {
        assert_ne!(BottomTab::Transport, BottomTab::Devices);
        assert_ne!(BottomTab::Devices, BottomTab::Session);
        assert_ne!(BottomTab::Transport, BottomTab::Session);
    }

    #[test]
    fn test_deactivate_active_no_panic() {
        let mut app = HostApp::default();
        app.deactivate_active(); // Should not panic when nothing is active
        assert!(app.param_snapshots.is_empty());
        assert!(app.status_message.contains("deactivated"));
    }

    #[test]
    fn test_activate_slot_invalid_index() {
        let mut app = HostApp::default();
        app.activate_slot(99); // Should not panic with empty rack
        assert!(!app.backend.is_active());
    }

    #[test]
    fn test_refresh_params_no_active() {
        let mut app = HostApp::default();
        app.refresh_params(); // Should not panic
        assert!(app.param_snapshots.is_empty());
    }

    #[test]
    fn test_session_path_default() {
        let app = HostApp::default();
        assert!(!app.session_path.is_empty());
        assert!(app.session_path.contains("session") || app.session_path.contains("json"));
    }

    #[test]
    fn test_save_session_creates_file() {
        let mut app = HostApp::default();
        let temp = std::env::temp_dir().join("rs-vst-host-test-gui-session");
        let path = temp.join("test_gui.json");
        app.session_path = path.to_string_lossy().to_string();

        let module = sample_module();
        app.add_to_rack(&module, &module.classes[0]);
        app.transport.tempo = 145.0;

        app.save_session();
        assert!(path.exists());
        assert!(app.status_message.contains("saved"));

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_load_session_roundtrip() {
        let mut app = HostApp::default();
        let temp = std::env::temp_dir().join("rs-vst-host-test-gui-session-rt");
        let path = temp.join("test_rt.json");
        app.session_path = path.to_string_lossy().to_string();

        let module = sample_module();
        app.add_to_rack(&module, &module.classes[0]);
        app.add_to_rack(&module, &module.classes[1]);
        app.transport.tempo = 155.0;
        app.transport.time_sig_num = 3;

        app.save_session();
        assert!(path.exists());

        // Create new app and load
        let mut app2 = HostApp::default();
        app2.session_path = path.to_string_lossy().to_string();
        app2.load_session();

        assert_eq!(app2.rack.len(), 2);
        assert_eq!(app2.rack[0].name, "TestSynth");
        assert_eq!(app2.rack[1].name, "TestEQ");
        assert_eq!(app2.transport.tempo, 155.0);
        assert_eq!(app2.transport.time_sig_num, 3);
        assert!(app2.status_message.contains("loaded"));

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_load_session_nonexistent() {
        let mut app = HostApp::default();
        app.session_path = "/nonexistent/path/session.json".into();
        app.load_session();
        assert!(app.status_message.contains("failed"));
    }

    #[test]
    fn test_tone_enabled_default() {
        let app = HostApp::default();
        assert!(!app.tone_enabled);
    }

    // ── New feature tests ───────────────────────────────────────────────

    #[test]
    fn test_safe_mode_constructor() {
        let app = HostApp::with_safe_mode(true);
        assert!(app.safe_mode);
        assert!(app.status_message.contains("Safe mode"));
    }

    #[test]
    fn test_safe_mode_false_constructor() {
        let app = HostApp::with_safe_mode(false);
        assert!(!app.safe_mode);
    }

    #[test]
    fn test_default_delegates_to_with_safe_mode() {
        let app = HostApp::default();
        assert!(!app.safe_mode);
        assert!(app.param_filter.is_empty());
    }

    #[test]
    fn test_param_filter_default_empty() {
        let app = HostApp::default();
        assert!(app.param_filter.is_empty());
    }

    #[test]
    fn test_prev_transport_defaults() {
        let app = HostApp::default();
        assert_eq!(app.prev_tempo, 120.0);
        assert_eq!(app.prev_time_sig_num, 4);
        assert_eq!(app.prev_time_sig_den, 4);
        assert!(!app.prev_playing);
    }

    #[test]
    fn test_sync_transport_skips_when_no_active() {
        let mut app = HostApp::default();

        // Change transport state
        app.transport.tempo = 140.0;
        app.transport.playing = true;

        // Sync should NOT update prev values (no active plugin)
        app.sync_transport();

        // prev values unchanged because no active plugin
        assert_eq!(app.prev_tempo, 120.0);
        assert!(!app.prev_playing);
    }

    #[test]
    fn test_sync_transport_no_change_no_update() {
        let mut app = HostApp::default();
        // Initial prev values match transport defaults
        assert_eq!(app.prev_tempo, app.transport.tempo);
        assert_eq!(app.prev_playing, app.transport.playing);

        // Sync with no changes should not alter anything
        app.sync_transport();
        assert_eq!(app.prev_tempo, 120.0);
    }

    #[test]
    fn test_open_editor_no_selected_slot() {
        let mut app = HostApp::default();
        app.open_editor(); // Should not panic
        assert!(app.status_message.contains("No plugin selected"));
    }

    #[test]
    fn test_open_editor_selected_but_no_active() {
        let mut app = HostApp::default();
        let module = sample_module();
        app.add_to_rack(&module, &module.classes[0]);
        app.selected_slot = Some(0);
        app.open_editor(); // Should not panic, will fail gracefully
        assert!(app.status_message.contains("failed") || app.status_message.contains("Editor"));
    }

    #[test]
    fn test_editor_count_default() {
        let app = HostApp::default();
        assert_eq!(app.backend.editor_count(), 0);
    }

    #[test]
    fn test_audio_status_default() {
        let app = HostApp::default();
        assert!(!app.backend.audio_status.running);
        assert_eq!(app.backend.audio_status.sample_rate, 0);
        assert_eq!(app.backend.audio_status.buffer_size, 0);
    }
}
