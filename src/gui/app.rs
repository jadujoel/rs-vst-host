//! Main GUI application — the `HostApp` `eframe::App` implementation.
//!
//! Provides the plugin browser, plugin rack, transport controls,
//! and parameter view in a Liquid Glass styled window.

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
}

impl Default for HostApp {
    fn default() -> Self {
        // Attempt to load cached plugins
        let plugin_modules = cache::load()
            .ok()
            .flatten()
            .map(|c| c.modules)
            .unwrap_or_default();

        let status = if plugin_modules.is_empty() {
            "No plugins cached. Click 'Scan' to discover VST3 plugins.".into()
        } else {
            let total: usize = plugin_modules.iter().map(|m| m.classes.len()).sum();
            format!("{} plugin class(es) loaded from cache.", total)
        };

        Self {
            plugin_modules,
            rack: Vec::new(),
            transport: TransportState::default(),
            browser_filter: BrowserFilter::default(),
            status_message: status,
            theme_applied: false,
            selected_slot: None,
        }
    }
}

impl HostApp {
    /// Run a plugin scan and refresh the module list.
    pub fn scan_plugins(&mut self) {
        self.status_message = "Scanning for plugins…".into();

        let search_paths = scanner::default_vst3_paths();
        let bundles = scanner::discover_bundles(&search_paths);

        let mut modules: Vec<PluginModuleInfo> = Vec::new();
        for bundle_path in &bundles {
            match crate::vst3::module::Vst3Module::load(bundle_path) {
                Ok(module) => {
                    if let Ok(info) = module.get_info() {
                        modules.push(info);
                    }
                }
                Err(e) => {
                    tracing::warn!(path = %bundle_path.display(), error = %e, "scan failed");
                }
            }
        }

        // Save to cache
        let scan_cache = cache::ScanCache::new(modules.clone());
        if let Err(e) = cache::save(&scan_cache) {
            tracing::warn!(error = %e, "cache save failed");
        }

        let total: usize = modules.iter().map(|m| m.classes.len()).sum();
        self.plugin_modules = modules;
        self.status_message = format!("Scan complete — {} plugin class(es) found.", total);
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

        // — Bottom panel: Transport + Status —
        egui::TopBottomPanel::bottom("transport_bar")
            .frame(egui::Frame {
                fill: theme::PANEL_FILL,
                inner_margin: egui::Margin::symmetric(16, 8),
                stroke: egui::Stroke::new(1.0, theme::GLASS_BORDER),
                ..Default::default()
            })
            .show(ctx, |ui| {
                self.show_transport_bar(ui);
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

    /// Render the bottom transport / status bar.
    fn show_transport_bar(&mut self, ui: &mut egui::Ui) {
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

            // Status at the right edge
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(&self.status_message)
                        .color(theme::TEXT_SECONDARY)
                        .small(),
                );
            });
        });
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
        let mut new_selected: Option<usize> = self.selected_slot;
        let selected_slot = self.selected_slot;

        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                for (i, slot) in self.rack.iter_mut().enumerate() {
                    let is_selected = selected_slot == Some(i);

                    let frame = if is_selected {
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
                                    .color(theme::ACCENT_DIM)
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
                                    ui.label(
                                        egui::RichText::new(&slot.vendor)
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
    }
}

// ── Launch ──────────────────────────────────────────────────────────────────

/// Launch the GUI window. This blocks until the window is closed.
pub fn launch() -> anyhow::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([800.0, 500.0])
            .with_title("rs-vst-host"),
        ..Default::default()
    };

    eframe::run_native(
        "rs-vst-host",
        options,
        Box::new(|_cc| Ok(Box::new(HostApp::default()))),
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
        // With no cache file, app should start with empty modules.
        let app = HostApp::default();
        // May or may not have cache depending on environment; just check it doesn't panic.
        assert!(app.rack.is_empty());
        assert!(app.selected_slot.is_none());
    }

    #[test]
    fn test_add_to_rack() {
        let mut app = HostApp::default();
        let module = sample_module();
        let class = &module.classes[0];
        app.add_to_rack(&module, class);

        assert_eq!(app.rack.len(), 1);
        assert_eq!(app.rack[0].name, "TestSynth");
        assert_eq!(app.rack[0].vendor, "TestVendor"); // falls back to factory_vendor
        assert!(!app.rack[0].bypassed);
    }

    #[test]
    fn test_add_to_rack_own_vendor() {
        let mut app = HostApp::default();
        let module = sample_module();
        let class = &module.classes[1]; // OtherVendor
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
        app.remove_from_rack(99); // Should not panic
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
        assert_eq!(app.selected_slot, Some(0)); // shifted down
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
        // TestSynth has no per-class vendor, falls back to factory_vendor "TestVendor"
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
}
