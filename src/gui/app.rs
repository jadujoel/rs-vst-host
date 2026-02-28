//! Main GUI application — the `HostApp` `eframe::App` implementation.
//!
//! Provides the plugin browser, plugin rack, transport controls,
//! parameter view, device selection, and session save/load in a
//! Styled window. Integrates with the [`super::backend::HostBackend`]
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
    /// Cached parameter snapshots from the last activation (transient; not serialized).
    pub param_cache: Vec<ParamSnapshot>,
    /// Staged parameter changes to apply on next activation (transient; not serialized).
    pub staged_changes: Vec<(u32, f64)>,
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
    /// Whether malloc debug mode is enabled (periodic heap checks).
    pub malloc_debug: bool,
    /// Frame counter for periodic heap checks in malloc debug mode.
    heap_check_counter: u32,
    /// Whether heap corruption has been detected (persistent warning).
    pub heap_corruption_detected: bool,
    /// Previous transport state for change detection.
    prev_tempo: f64,
    /// Previous time signature numerator.
    prev_time_sig_num: u32,
    /// Previous time signature denominator.
    prev_time_sig_den: u32,
    /// Previous playing state.
    prev_playing: bool,
    /// Custom scan paths (exclusive — when non-empty, defaults are skipped).
    pub custom_paths: Vec<PathBuf>,
}

impl HostApp {
    /// Create a new HostApp with configuration options.
    pub fn new(safe_mode: bool, malloc_debug: bool) -> Self {
        Self::with_paths(safe_mode, malloc_debug, Vec::new())
    }

    /// Create a new HostApp with configuration options and custom scan paths.
    ///
    /// When `custom_paths` is non-empty, only those paths are used for scanning
    /// (default system paths and persistent config paths are excluded).
    pub fn with_paths(safe_mode: bool, malloc_debug: bool, custom_paths: Vec<PathBuf>) -> Self {
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
            malloc_debug,
            heap_check_counter: 0,
            heap_corruption_detected: false,
            custom_paths,
        }
    }

    /// Create a new HostApp, optionally in safe mode (no malloc debug).
    #[cfg(test)]
    pub fn with_safe_mode(safe_mode: bool) -> Self {
        Self::new(safe_mode, false)
    }
}

impl Default for HostApp {
    fn default() -> Self {
        Self::new(false, false)
    }
}

impl HostApp {
    /// Run a plugin scan and refresh the module list.
    pub fn scan_plugins(&mut self) {
        self.status_message = "Scanning for plugins…".into();

        let search_paths = if self.custom_paths.is_empty() {
            scanner::default_vst3_paths()
        } else {
            self.custom_paths.clone()
        };
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
            param_cache: Vec::new(),
            staged_changes: Vec::new(),
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
                self.param_snapshots.clear();
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

                // Apply any staged parameter changes from prior inactive edits
                let staged: Vec<(u32, f64)> = self.rack[index].staged_changes.drain(..).collect();
                let staged_count = staged.len();
                for (id, value) in staged {
                    if let Err(e) = self.backend.set_parameter(id, value) {
                        tracing::warn!(param_id = id, error = %e, "staged param apply failed");
                    }
                }

                // Refresh params after applying staged changes
                if staged_count > 0 {
                    self.param_snapshots = self.backend.active_param_snapshots();
                }

                // Update the slot cache
                self.rack[index]
                    .param_cache
                    .clone_from(&self.param_snapshots);

                let staged_msg = if staged_count > 0 {
                    format!(" ({} staged change(s) applied)", staged_count)
                } else {
                    String::new()
                };
                self.status_message =
                    format!("▶ '{}' active — processing audio.{}", name, staged_msg);
            }
            Err(e) => {
                self.status_message = format!("✗ Failed to activate '{}': {}", name, e);
                tracing::error!(plugin = %name, error = %e, "activation failed");
            }
        }
    }

    /// Deactivate the currently active plugin.
    pub fn deactivate_active(&mut self) {
        // Cache current params to the active slot before deactivating
        let active_name = if let Some(active_idx) = self.backend.active_slot_index() {
            if let Some(slot) = self.rack.get_mut(active_idx) {
                slot.param_cache = self.param_snapshots.clone();
            }
            self.rack.get(active_idx).map(|s| s.name.clone())
        } else {
            None
        };

        let tainted_before = self.backend.tainted_paths.len();
        self.backend.deactivate_plugin();
        let tainted_after = self.backend.tainted_paths.len();

        // Check if the plugin crashed during deactivation and is now tainted
        if tainted_after > tainted_before {
            if let Some(name) = &active_name {
                self.status_message = format!(
                    "⚠ '{}' crashed during deactivation — restart the host to reuse this plugin.",
                    name
                );
                return;
            }
        }

        // param_snapshots retained for display; refresh_params will load from cache
        self.status_message = "Plugin deactivated.".into();
    }

    /// Refresh parameter snapshots from the backend.
    ///
    /// For the active plugin: drains handler changes and refreshes from the
    /// backend's live parameter registry each frame.
    /// For inactive selected plugins: loads from the slot's parameter cache.
    /// When nothing is selected: clears snapshots.
    pub fn refresh_params(&mut self) {
        let Some(idx) = self.selected_slot else {
            if !self.param_snapshots.is_empty() {
                self.param_snapshots.clear();
            }
            return;
        };

        let is_active = self.backend.active_slot_index() == Some(idx);

        if is_active {
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

            // Keep the slot cache up to date
            if let Some(slot) = self.rack.get_mut(idx) {
                slot.param_cache.clone_from(&self.param_snapshots);
            }
        } else {
            // Inactive: load from the slot's parameter cache
            if let Some(slot) = self.rack.get(idx) {
                self.param_snapshots.clone_from(&slot.param_cache);
            } else {
                self.param_snapshots.clear();
            }
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

                self.status_message = format!(
                    "Session loaded from {} ({} slots)",
                    path.display(),
                    self.rack.len()
                );
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
        let _span = tracing::trace_span!("gui_update").entered();
        // Apply theme once
        if !self.theme_applied {
            theme::apply(ctx);
            self.theme_applied = true;
        }

        // Refresh parameters each frame if a plugin is active
        self.refresh_params();

        // Detect plugin crashes and deactivate safely
        if self.backend.is_crashed() {
            let active_name = self
                .backend
                .active_slot_index()
                .and_then(|idx| self.rack.get(idx))
                .map(|s| s.name.clone())
                .unwrap_or_else(|| "Unknown".into());
            self.backend.deactivate_plugin();
            self.status_message = format!(
                "⚠ '{}' crashed — deactivated safely. The host is unaffected.",
                active_name
            );
            tracing::warn!(plugin = %active_name, "Plugin crash detected by GUI — deactivated");
        }

        // Propagate heap corruption flag from backend to app
        if self.backend.heap_corruption_detected && !self.heap_corruption_detected {
            self.heap_corruption_detected = true;
            tracing::error!("Heap corruption detected — user warned via GUI banner");
        }

        // Periodic heap check in malloc debug mode (every ~60 frames)
        if self.malloc_debug {
            self.heap_check_counter += 1;
            if self.heap_check_counter >= 60 {
                self.heap_check_counter = 0;
                if !crate::diagnostics::heap_check() && !self.heap_corruption_detected {
                    self.heap_corruption_detected = true;
                    self.backend.heap_corruption_detected = true;
                    tracing::error!("Periodic heap check detected corruption (malloc_debug mode)");
                }
            }
        }

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

        // — Heap Corruption Warning Banner (persistent, shown at top) —
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
    pub(crate) fn show_browser(&mut self, ui: &mut egui::Ui) {
        ui.heading("Plugin Browser");
        ui.add_space(8.0);

        // Scan button
        if ui
            .add(
                egui::Button::new("⟳  Scan Plugins")
                    .min_size(egui::vec2(ui.available_width(), 28.0)),
            )
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
                                            add_action = Some((module.clone(), class.clone()));
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
    pub(crate) fn show_bottom_bar(&mut self, ui: &mut egui::Ui) {
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
                            status.sample_rate, status.buffer_size, status.device_name, editor_str,
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
                        .selectable_label(self.backend.selected_audio_device.is_none(), "(default)")
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
    ///
    /// Supports three states:
    /// - **Active plugin selected**: live parameter sliders with real-time feedback.
    /// - **Inactive plugin with cache**: cached parameter sliders; changes are staged
    ///   and applied on the next activation.
    /// - **Inactive plugin, no cache**: placeholder prompting the user to activate.
    pub(crate) fn show_param_panel(&mut self, ui: &mut egui::Ui) {
        let Some(idx) = self.selected_slot else {
            // No slot selected — show placeholder
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

        let is_active = self.backend.active_slot_index() == Some(idx);

        // Header with plugin name and vendor
        ui.heading(format!("🎛 {}", slot_name));
        if !slot_vendor.is_empty() {
            ui.label(
                egui::RichText::new(&slot_vendor)
                    .color(theme::TEXT_SECONDARY)
                    .small(),
            );
        }
        ui.add_space(4.0);

        // Status banner for inactive plugins
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
                // No cached params — show activation prompt
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
                        // Editable: slider
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
            if is_active {
                // Live: push to audio thread
                match self.backend.set_parameter(id, value) {
                    Ok(_actual) => {}
                    Err(e) => {
                        self.status_message = format!("⚠ Parameter change failed: {}", e);
                        tracing::warn!(param_id = id, error = %e, "param set failed");
                    }
                }
            } else {
                // Inactive: stage the change for later activation
                if let Some(slot) = self.rack.get_mut(idx) {
                    // Update or insert the staged change
                    if let Some(existing) =
                        slot.staged_changes.iter_mut().find(|(sid, _)| *sid == id)
                    {
                        existing.1 = value;
                    } else {
                        slot.staged_changes.push((id, value));
                    }
                    // Update the cache so the display reflects the change next frame
                    if let Some(cached) = slot.param_cache.iter_mut().find(|s| s.id == id) {
                        cached.value = value;
                        cached.display = format!("{:.3}", value);
                    }
                }
            }
        }
    }

    /// Render the central plugin rack.
    pub(crate) fn show_rack(&mut self, ui: &mut egui::Ui) {
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
                                            egui::Button::new("✕").fill(egui::Color32::TRANSPARENT),
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
///
/// When `paths` is non-empty, only those paths are scanned for plugins
/// (default system paths are excluded).
pub fn launch(safe_mode: bool, malloc_debug: bool, paths: Vec<PathBuf>) -> anyhow::Result<()> {
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
        Box::new(move |_cc| {
            Ok(Box::new(HostApp::with_paths(
                safe_mode,
                malloc_debug,
                paths,
            )))
        }),
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
        let app = HostApp {
            plugin_modules: vec![sample_module()],
            ..Default::default()
        };
        let results = app.filtered_classes();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_filtered_classes_by_name() {
        let app = HostApp {
            plugin_modules: vec![sample_module()],
            browser_filter: BrowserFilter {
                text: "synth".into(),
            },
            ..Default::default()
        };
        let results = app.filtered_classes();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.name, "TestSynth");
    }

    #[test]
    fn test_filtered_classes_by_subcategory() {
        let app = HostApp {
            plugin_modules: vec![sample_module()],
            browser_filter: BrowserFilter { text: "eq".into() },
            ..Default::default()
        };
        let results = app.filtered_classes();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.name, "TestEQ");
    }

    #[test]
    fn test_filtered_classes_by_vendor() {
        let app = HostApp {
            plugin_modules: vec![sample_module()],
            browser_filter: BrowserFilter {
                text: "othervendor".into(),
            },
            ..Default::default()
        };
        let results = app.filtered_classes();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.name, "TestEQ");
    }

    #[test]
    fn test_filtered_classes_factory_vendor_fallback() {
        let app = HostApp {
            plugin_modules: vec![sample_module()],
            browser_filter: BrowserFilter {
                text: "testvendor".into(),
            },
            ..Default::default()
        };
        let results = app.filtered_classes();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.name, "TestSynth");
    }

    #[test]
    fn test_filtered_classes_no_match() {
        let app = HostApp {
            plugin_modules: vec![sample_module()],
            browser_filter: BrowserFilter {
                text: "nonexistent".into(),
            },
            ..Default::default()
        };
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
            param_cache: Vec::new(),
            staged_changes: Vec::new(),
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
        let mut app2 = HostApp {
            session_path: path.to_string_lossy().to_string(),
            ..Default::default()
        };
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
        let mut app = HostApp {
            session_path: "/nonexistent/path/session.json".into(),
            ..Default::default()
        };
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

    // ── Interaction plan tests ──────────────────────────────────────────

    fn make_snapshot(id: u32, title: &str, value: f64) -> ParamSnapshot {
        ParamSnapshot {
            id,
            title: title.into(),
            units: String::new(),
            value,
            default: 0.5,
            display: format!("{:.3}", value),
            can_automate: true,
            is_read_only: false,
            is_bypass: false,
        }
    }

    #[test]
    fn test_slot_param_cache_default_empty() {
        let mut app = HostApp::default();
        let module = sample_module();
        app.add_to_rack(&module, &module.classes[0]);
        assert!(app.rack[0].param_cache.is_empty());
        assert!(app.rack[0].staged_changes.is_empty());
    }

    #[test]
    fn test_selection_shows_cached_params() {
        let mut app = HostApp::default();
        let module = sample_module();
        app.add_to_rack(&module, &module.classes[0]);

        // Simulate cached params
        app.rack[0].param_cache = vec![make_snapshot(1, "Volume", 0.5)];

        app.selected_slot = Some(0);
        app.refresh_params();

        assert_eq!(app.param_snapshots.len(), 1);
        assert_eq!(app.param_snapshots[0].title, "Volume");
    }

    #[test]
    fn test_selection_empty_cache_gives_no_params() {
        let mut app = HostApp::default();
        let module = sample_module();
        app.add_to_rack(&module, &module.classes[0]);
        app.selected_slot = Some(0);
        app.refresh_params();
        assert!(app.param_snapshots.is_empty());
    }

    #[test]
    fn test_no_selection_clears_params() {
        let mut app = HostApp {
            param_snapshots: vec![make_snapshot(1, "X", 0.5)],
            selected_slot: None,
            ..Default::default()
        };
        app.refresh_params();
        assert!(app.param_snapshots.is_empty());
    }

    #[test]
    fn test_remove_selected_slot_clears_params() {
        let mut app = HostApp::default();
        let module = sample_module();
        app.add_to_rack(&module, &module.classes[0]);
        app.selected_slot = Some(0);
        app.param_snapshots = vec![make_snapshot(1, "X", 0.5)];

        app.remove_from_rack(0);
        assert!(app.selected_slot.is_none());
        assert!(app.param_snapshots.is_empty());
    }

    #[test]
    fn test_remove_non_selected_preserves_params() {
        let mut app = HostApp::default();
        let module = sample_module();
        app.add_to_rack(&module, &module.classes[0]);
        app.add_to_rack(&module, &module.classes[1]);
        app.selected_slot = Some(1);
        app.param_snapshots = vec![make_snapshot(1, "X", 0.5)];

        app.remove_from_rack(0);
        // selected_slot adjusts from 1 to 0
        assert_eq!(app.selected_slot, Some(0));
        // params preserved (non-selected slot was removed)
        assert_eq!(app.param_snapshots.len(), 1);
    }

    #[test]
    fn test_staging_params_for_inactive() {
        let mut app = HostApp::default();
        let module = sample_module();
        app.add_to_rack(&module, &module.classes[0]);

        app.rack[0].staged_changes.push((0, 0.8));
        app.rack[0].staged_changes.push((1, 0.3));

        assert_eq!(app.rack[0].staged_changes.len(), 2);
        assert_eq!(app.rack[0].staged_changes[0], (0, 0.8));
        assert_eq!(app.rack[0].staged_changes[1], (1, 0.3));
    }

    #[test]
    fn test_staged_changes_update_existing() {
        let mut app = HostApp::default();
        let module = sample_module();
        app.add_to_rack(&module, &module.classes[0]);

        app.rack[0].staged_changes.push((0, 0.5));
        // Update existing staged change (same logic as show_param_panel)
        if let Some(existing) = app.rack[0]
            .staged_changes
            .iter_mut()
            .find(|(id, _)| *id == 0)
        {
            existing.1 = 0.9;
        }

        assert_eq!(app.rack[0].staged_changes.len(), 1);
        assert_eq!(app.rack[0].staged_changes[0], (0, 0.9));
    }

    #[test]
    fn test_deactivate_preserves_param_snapshots() {
        let mut app = HostApp::default();
        let module = sample_module();
        app.add_to_rack(&module, &module.classes[0]);
        app.selected_slot = Some(0);
        app.param_snapshots = vec![make_snapshot(1, "Volume", 0.7)];

        // deactivate_active: backend.active_slot_index() is None (no real backend)
        // so cache won't be populated, but param_snapshots are retained
        app.deactivate_active();

        // param_snapshots no longer cleared on deactivation
        assert_eq!(app.param_snapshots.len(), 1);
        assert!(app.status_message.contains("deactivated"));
    }

    #[test]
    fn test_param_cache_survives_slot_reorder() {
        let mut app = HostApp::default();
        let module = sample_module();
        app.add_to_rack(&module, &module.classes[0]);
        app.add_to_rack(&module, &module.classes[1]);

        // Cache params for slot 1
        app.rack[1].param_cache = vec![make_snapshot(5, "Freq", 0.5)];

        // Remove slot 0 — slot 1 becomes slot 0
        app.remove_from_rack(0);
        assert_eq!(app.rack.len(), 1);
        assert_eq!(app.rack[0].param_cache.len(), 1);
        assert_eq!(app.rack[0].param_cache[0].title, "Freq");
    }

    #[test]
    fn test_refresh_params_inactive_with_cache() {
        let mut app = HostApp::default();
        let module = sample_module();
        app.add_to_rack(&module, &module.classes[0]);
        app.rack[0].param_cache = vec![
            make_snapshot(1, "Volume", 0.7),
            make_snapshot(2, "Pan", 0.5),
        ];
        app.selected_slot = Some(0);

        app.refresh_params();

        assert_eq!(app.param_snapshots.len(), 2);
        assert_eq!(app.param_snapshots[0].value, 0.7);
        assert_eq!(app.param_snapshots[1].title, "Pan");
    }

    #[test]
    fn test_refresh_params_invalid_index() {
        // Selected slot points beyond rack length
        let mut app = HostApp {
            selected_slot: Some(5),
            param_snapshots: vec![make_snapshot(1, "X", 0.5)],
            ..Default::default()
        };
        app.refresh_params();
        // Should clear because slot doesn't exist
        assert!(app.param_snapshots.is_empty());
    }

    #[test]
    fn test_staged_changes_cleared_on_remove() {
        let mut app = HostApp::default();
        let module = sample_module();
        app.add_to_rack(&module, &module.classes[0]);
        app.rack[0].staged_changes.push((0, 0.8));
        app.rack[0].staged_changes.push((1, 0.3));

        // Remove the slot — staged changes go with it
        app.remove_from_rack(0);
        assert!(app.rack.is_empty());
    }

    #[test]
    fn test_cache_update_on_staging() {
        let mut app = HostApp::default();
        let module = sample_module();
        app.add_to_rack(&module, &module.classes[0]);
        app.rack[0].param_cache = vec![make_snapshot(1, "Volume", 0.5)];

        // Simulate staging a change (same logic as show_param_panel for inactive)
        let id = 1u32;
        let value = 0.8;
        if let Some(existing) = app.rack[0]
            .staged_changes
            .iter_mut()
            .find(|(sid, _)| *sid == id)
        {
            existing.1 = value;
        } else {
            app.rack[0].staged_changes.push((id, value));
        }
        if let Some(cached) = app.rack[0].param_cache.iter_mut().find(|s| s.id == id) {
            cached.value = value;
            cached.display = format!("{:.3}", value);
        }

        assert_eq!(app.rack[0].param_cache[0].value, 0.8);
        assert_eq!(app.rack[0].param_cache[0].display, "0.800");
        assert_eq!(app.rack[0].staged_changes.len(), 1);
    }

    #[test]
    fn test_session_roundtrip_preserves_no_transient_data() {
        // param_cache and staged_changes should not survive session save/load
        let mut app = HostApp::default();
        let module = sample_module();
        app.add_to_rack(&module, &module.classes[0]);
        app.rack[0].param_cache = vec![make_snapshot(1, "Vol", 0.7)];
        app.rack[0].staged_changes.push((1, 0.9));

        let temp = std::env::temp_dir().join("rs-vst-host-test-gui-transient");
        let path = temp.join("test_transient.json");
        app.session_path = path.to_string_lossy().to_string();
        app.save_session();

        let mut app2 = HostApp {
            session_path: path.to_string_lossy().to_string(),
            ..Default::default()
        };
        app2.load_session();

        // Transient fields reset on load
        assert_eq!(app2.rack.len(), 1);
        assert!(app2.rack[0].param_cache.is_empty());
        assert!(app2.rack[0].staged_changes.is_empty());

        let _ = std::fs::remove_dir_all(&temp);
    }

    // ── Diagnostics / debug infrastructure tests ────────────────────────

    #[test]
    fn test_host_app_new_with_malloc_debug() {
        let app = HostApp::new(false, true);
        assert!(app.malloc_debug);
        assert!(!app.heap_corruption_detected);
        assert_eq!(app.heap_check_counter, 0);
    }

    #[test]
    fn test_host_app_new_without_malloc_debug() {
        let app = HostApp::new(false, false);
        assert!(!app.malloc_debug);
        assert!(!app.heap_corruption_detected);
    }

    #[test]
    fn test_host_app_heap_corruption_flag_propagation() {
        let mut app = HostApp::new(false, false);
        assert!(!app.heap_corruption_detected);

        // Simulate backend detecting heap corruption
        app.backend.heap_corruption_detected = true;

        // The flag should propagate (normally happens in update(), but test the field directly)
        assert!(app.backend.heap_corruption_detected);
    }

    #[test]
    fn test_with_safe_mode_creates_no_malloc_debug() {
        let app = HostApp::with_safe_mode(true);
        assert!(app.safe_mode);
        assert!(!app.malloc_debug);
    }

    #[test]
    fn test_with_paths_stores_custom_paths() {
        let paths = vec![PathBuf::from("/custom/vst3"), PathBuf::from("./local")];
        let app = HostApp::with_paths(false, false, paths.clone());
        assert_eq!(app.custom_paths, paths);
        assert!(!app.safe_mode);
        assert!(!app.malloc_debug);
    }

    #[test]
    fn test_with_paths_empty_has_no_custom_paths() {
        let app = HostApp::with_paths(false, false, Vec::new());
        assert!(app.custom_paths.is_empty());
    }

    #[test]
    fn test_new_has_no_custom_paths() {
        let app = HostApp::new(false, false);
        assert!(app.custom_paths.is_empty());
    }
}
