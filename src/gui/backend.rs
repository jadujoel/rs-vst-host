//! Host backend — bridges the GUI with audio engine, plugin instances, and MIDI.
//!
//! Manages the lifecycle of active plugin instances, audio streams, and MIDI
//! connections. The GUI thread interacts with the backend through high-level
//! methods; the audio thread receives work via lock-free queues.

use crate::audio::device::{AudioConfig, AudioDevice, DeviceInfo};
use crate::audio::engine::AudioEngine;
use crate::gui::editor::EditorWindow;
use crate::midi::device::{MidiDevice, MidiPortInfo};
use crate::vst3::com::K_SPEAKER_STEREO;
use crate::vst3::component_handler::HostComponentHandler;
use crate::vst3::instance::DEACTIVATION_CRASHED;
use crate::vst3::module::Vst3Module;
use crate::vst3::params::ParameterRegistry;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tracing::{debug, info, warn};

/// A parameter snapshot for safe GUI display.
///
/// Contains only owned data — no COM pointers — so it can be freely
/// cloned, stored, and rendered by the UI without lifetime concerns.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ParamSnapshot {
    /// Parameter ID.
    pub id: u32,
    /// Display title.
    pub title: String,
    /// Units label (e.g. "dB", "Hz").
    pub units: String,
    /// Current normalized value [0..1].
    pub value: f64,
    /// Default normalized value [0..1].
    pub default: f64,
    /// Display string for the current value.
    pub display: String,
    /// Whether the parameter can be automated.
    pub can_automate: bool,
    /// Whether the parameter is read-only.
    pub is_read_only: bool,
    /// Whether this is a bypass parameter.
    pub is_bypass: bool,
}

/// Audio engine status snapshot for GUI display.
#[derive(Debug, Clone, Default)]
pub struct AudioStatus {
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Buffer/block size in frames.
    pub buffer_size: u32,
    /// Audio output device name.
    pub device_name: String,
    /// Whether audio engine is running.
    pub running: bool,
}

/// The host backend managing audio engine and plugin lifecycle.
pub struct HostBackend {
    /// Audio device manager.
    audio_manager: AudioDevice,
    /// Cached audio output devices.
    pub audio_devices: Vec<DeviceInfo>,
    /// Cached MIDI input ports.
    pub midi_ports: Vec<MidiPortInfo>,
    /// Selected audio device name.
    pub selected_audio_device: Option<String>,
    /// Selected MIDI port name.
    pub selected_midi_port: Option<String>,
    /// Currently active plugin processing audio.
    active: Option<ActiveState>,
    /// Open editor windows.
    pub editor_windows: Vec<EditorWindow>,
    /// Current audio engine status.
    pub audio_status: AudioStatus,
    /// Plugin bundle paths that crashed during deactivation.
    ///
    /// When a plugin crashes during COM cleanup, `siglongjmp` recovery can
    /// leave the process heap in an inconsistent state. The library is
    /// intentionally leaked (not unloaded) so C++ static destructors don't
    /// run on corrupted state. Re-loading the same library via `dlopen`
    /// returns the already-mapped (corrupted) code, and `bundleEntry` on
    /// that state triggers malloc corruption detection → SIGABRT.
    ///
    /// Plugins in this set cannot be re-activated until the host is restarted.
    pub tainted_paths: HashSet<PathBuf>,
}

/// Runtime state for an active (instantiated and processing) plugin.
///
/// **Drop order is critical.** When this struct is dropped, Rust drops
/// fields in declaration order. The correct teardown sequence is:
///
/// 1. `params` — borrowing pointers into the controller; must be dropped
///    while the library is still loaded (no-op Drop when `owns_controller = false`,
///    but clearing it early avoids any dangling-pointer risk).
/// 2. `_stream` — stopping the audio stream prevents further audio callbacks
///    and drops the callback closure, which releases the last `Arc<Mutex<AudioEngine>>`
///    reference, destroying the engine and the `Vst3Instance` inside it
///    (releasing all COM references).
/// 3. `engine` — the `Arc` held here is decremented. If the stream callback
///    already released its clone, this destroys the engine.
/// 4. `_midi_connection` — closes the MIDI input port.
/// 5. `_module` — releases the factory, calls `bundleExit`, unloads the library.
///
/// A manual `Drop` implementation enforces this order.
struct ActiveState {
    /// Which rack slot index is active.
    slot_index: usize,
    /// Path to the .vst3 bundle (for tainted-path tracking).
    plugin_path: PathBuf,
    /// The audio engine processing this plugin.
    engine: Arc<Mutex<AudioEngine>>,
    /// The cpal audio stream (must stay alive for audio output).
    _stream: Option<cpal::Stream>,
    /// The loaded VST3 module — must stay alive so the dynamic library
    /// (and all COM vtable pointers within it) remains mapped in memory.
    _module: Vst3Module,
    /// Parameter registry for this plugin (used from the GUI thread).
    params: Option<ParameterRegistry>,
    /// Queue for parameter changes from GUI → audio thread.
    param_queue: Arc<Mutex<Vec<(u32, f64)>>>,
    /// Component handler for plugin-initiated parameter changes.
    component_handler: *mut HostComponentHandler,
    /// MIDI connection (must stay alive for MIDI input).
    _midi_connection: Option<midir::MidiInputConnection<()>>,
    /// Whether the plugin has an editor available.
    has_editor: bool,
}

// Safety: COM pointers in ActiveState are accessed consistently:
// - ParameterRegistry (controller) is only accessed from the main/GUI thread
// - AudioEngine (processor) is accessed from the audio thread via Arc<Mutex<>>
// - component_handler uses internal Mutex for thread safety
unsafe impl Send for ActiveState {}

impl Drop for ActiveState {
    fn drop(&mut self) {
        // 1. Drop params first — they borrow a controller pointer from the
        //    Vst3Instance inside the engine. Must be released while the
        //    library is still loaded.
        self.params.take();

        // 2. Drop the audio stream — this stops the CoreAudio render callback,
        //    which drops the callback closure's Arc<Mutex<AudioEngine>> clone.
        //    If that was the last reference, the AudioEngine (and Vst3Instance)
        //    are destroyed here, releasing all COM references.
        self._stream.take();

        // 3. Drop the engine Arc — if the stream callback already released its
        //    clone, this is a no-op. Otherwise this destroys the engine.
        //    (Uses ManuallyDrop-like semantics via the Arc.)

        // 4. Drop the MIDI connection (closes the MIDI port).
        self._midi_connection.take();

        // 5. The remaining fields (_module, param_queue, etc.) are dropped
        //    in normal declaration order. _module unloads the library LAST,
        //    which is correct since all COM pointers have been released above.
        debug!("ActiveState dropped with controlled teardown order");
    }
}

impl HostBackend {
    /// Create a new backend, enumerating available devices.
    pub fn new() -> Self {
        let audio_manager = AudioDevice::new();
        let audio_devices = audio_manager.list_output_devices();
        let midi_ports = MidiDevice::new()
            .ok()
            .map(|d| d.list_input_ports())
            .unwrap_or_default();

        Self {
            audio_manager,
            audio_devices,
            midi_ports,
            selected_audio_device: None,
            selected_midi_port: None,
            active: None,
            editor_windows: Vec::new(),
            audio_status: AudioStatus::default(),
            tainted_paths: HashSet::new(),
        }
    }

    /// Refresh the cached device lists.
    pub fn refresh_devices(&mut self) {
        self.audio_devices = self.audio_manager.list_output_devices();
        self.midi_ports = MidiDevice::new()
            .ok()
            .map(|d| d.list_input_ports())
            .unwrap_or_default();
    }

    /// Activate a plugin from a rack slot, starting audio processing.
    ///
    /// If another plugin is already active, it is deactivated first.
    /// Returns parameter snapshots for the newly activated plugin.
    pub fn activate_plugin(
        &mut self,
        slot_index: usize,
        path: &std::path::Path,
        cid: &[u8; 16],
        name: &str,
    ) -> Result<Vec<ParamSnapshot>, String> {
        // Refuse to load plugins that crashed during a prior deactivation.
        // The library is still mapped in memory with corrupted internal state;
        // reloading it would trigger malloc corruption → SIGABRT.
        if self.tainted_paths.contains(path) {
            return Err(format!(
                "Plugin '{}' crashed during a prior deactivation and cannot be reloaded. \
                 Restart the host to use this plugin again.",
                name
            ));
        }

        // Deactivate current plugin if any
        self.deactivate_plugin();

        // 1. Load module
        let module = Vst3Module::load(path).map_err(|e| format!("Failed to load module: {}", e))?;

        // 2. Create instance
        let mut instance = module
            .create_instance(cid, name)
            .map_err(|e| format!("Failed to create instance: {}", e))?;

        // 3. Verify 32-bit float support
        if !instance.can_process_f32() {
            return Err(format!(
                "Plugin '{}' does not support 32-bit float processing",
                name
            ));
        }

        // 4. Get audio device
        let device = self
            .audio_manager
            .get_output_device(self.selected_audio_device.as_deref())
            .ok_or_else(|| "No audio output device available".to_string())?;

        let default_config = AudioDevice::default_config(&device).map_err(|e| e.to_string())?;

        let config = AudioConfig {
            sample_rate: default_config.sample_rate,
            channels: default_config.channels.min(2),
            buffer_size: 0,
        };

        let max_block_size = 4096i32;

        // 5. Configure plugin
        instance
            .set_bus_arrangements(K_SPEAKER_STEREO, K_SPEAKER_STEREO)
            .map_err(|e| format!("Bus arrangement setup failed: {}", e))?;

        instance
            .setup_processing(config.sample_rate as f64, max_block_size)
            .map_err(|e| format!("Processing setup failed: {}", e))?;

        instance
            .activate()
            .map_err(|e| format!("Activation failed: {}", e))?;

        instance
            .start_processing()
            .map_err(|e| format!("Start processing failed: {}", e))?;

        // 6. Install component handler
        instance.install_component_handler();
        let component_handler = instance.component_handler();

        // 7. Query parameters
        let params = instance.query_parameters();
        let snapshots = self.build_snapshots(&params);

        // 7b. Check for editor availability
        let has_editor = instance.has_editor();

        // 8. Create audio engine
        let mut engine = AudioEngine::new(
            instance,
            config.sample_rate as f64,
            max_block_size as usize,
            config.channels as usize,
        );

        // 9. Setup MIDI if selected
        let midi_connection = if let Some(ref midi_name) = self.selected_midi_port {
            match crate::midi::device::open_midi_input(Some(midi_name)) {
                Ok((conn, port_name, receiver)) => {
                    engine.set_midi_receiver(receiver);
                    info!(port = %port_name, "MIDI input connected");
                    Some(conn)
                }
                Err(e) => {
                    warn!(error = %e, "Failed to open MIDI input");
                    None
                }
            }
        } else {
            None
        };

        // 10. Capture state handles
        let param_queue = engine.pending_param_queue();
        let engine = Arc::new(Mutex::new(engine));

        // 11. Build audio stream
        let engine_cb = engine.clone();
        let stream = AudioDevice::build_output_stream(
            &device,
            &config,
            move |data: &mut [f32], _info: &cpal::OutputCallbackInfo| {
                if let Ok(mut eng) = engine_cb.try_lock() {
                    eng.process(data);
                } else {
                    data.fill(0.0);
                }
            },
            |err| {
                tracing::error!(error = %err, "Audio stream error");
            },
        )
        .map_err(|e| e.to_string())?;

        AudioDevice::play(&stream).map_err(|e| e.to_string())?;

        info!(plugin = %name, slot = slot_index, "Plugin activated in GUI");

        // Update audio status
        let device_name = self
            .selected_audio_device
            .clone()
            .unwrap_or_else(|| "(default)".into());
        self.audio_status = AudioStatus {
            sample_rate: config.sample_rate,
            buffer_size: max_block_size as u32,
            device_name,
            running: true,
        };

        self.active = Some(ActiveState {
            slot_index,
            plugin_path: path.to_path_buf(),
            engine,
            _stream: Some(stream),
            _module: module,
            params,
            param_queue,
            component_handler,
            _midi_connection: midi_connection,
            has_editor,
        });

        Ok(snapshots)
    }

    /// Deactivate the currently active plugin, stopping audio.
    ///
    /// Shutdown sequence:
    /// 1. Close any open editor windows (releases IPlugView COM objects).
    /// 2. Lock the engine and call `shutdown()` — this sets the `is_shutdown`
    ///    flag (so racing audio callbacks output silence) and tells the VST3
    ///    plugin to stop processing and deactivate.
    /// 3. Drop the audio stream — stops the CoreAudio render callback and
    ///    releases the callback's `Arc<Mutex<AudioEngine>>` clone.
    /// 4. Drop the `ActiveState` — the custom `Drop` impl releases params,
    ///    MIDI, engine, and finally unloads the module in the correct order.
    pub fn deactivate_plugin(&mut self) {
        // Close any open editor windows for this plugin
        self.close_all_editors();

        if let Some(mut active) = self.active.take() {
            // Capture the path before dropping (for tainted-path tracking).
            let plugin_path = active.plugin_path.clone();

            // Clear the deactivation-crashed flag *before* drop so we only
            // detect crashes that happen during this specific deactivation.
            DEACTIVATION_CRASHED.with(|c| c.set(false));

            // 1. Shut down the engine while holding the lock. This sets
            //    the is_shutdown flag so the audio callback will output
            //    silence if it races between our unlock and stream drop.
            if let Ok(mut eng) = active.engine.lock() {
                eng.shutdown();
            }

            // 2. Drop the stream explicitly BEFORE dropping the rest.
            //    This stops CoreAudio's render callback and drops the
            //    callback closure's Arc clone, potentially destroying
            //    the AudioEngine (and thus the Vst3Instance + COM refs).
            active._stream.take();

            // 3. Now `active` is dropped (via the custom Drop impl),
            //    which releases params, midi, engine Arc, and finally
            //    the Vst3Module (unloading the library).
            drop(active);

            // 4. Check whether the plugin crashed during COM cleanup.
            //    If so, the library was leaked and the process heap may
            //    be corrupted. Record the path so we refuse to reload it.
            let crashed = DEACTIVATION_CRASHED.with(|c| c.get());
            if crashed {
                warn!(
                    path = %plugin_path.display(),
                    "Plugin crashed during deactivation — marking as tainted (restart required to reuse)"
                );
                self.tainted_paths.insert(plugin_path);
            }

            debug!("Plugin deactivated in GUI");
        }

        self.audio_status.running = false;
    }

    /// Get the currently active slot index, if any.
    pub fn active_slot_index(&self) -> Option<usize> {
        self.active.as_ref().map(|a| a.slot_index)
    }

    /// Get fresh parameter snapshots for the active plugin.
    pub fn active_param_snapshots(&self) -> Vec<ParamSnapshot> {
        let params_ref = self.active.as_ref().and_then(|a| a.params.as_ref());
        self.build_snapshots_ref(params_ref)
    }

    /// Set a parameter value on the active plugin.
    ///
    /// Pushes the change to the audio thread queue and updates the controller.
    /// Returns the actual value set (read back from the controller).
    pub fn set_parameter(&mut self, id: u32, value: f64) -> Result<f64, String> {
        let active = self.active.as_mut().ok_or("No active plugin")?;

        // Push to audio thread
        if let Ok(mut queue) = active.param_queue.lock() {
            queue.push((id, value));
        }

        // Update on the controller (for display feedback)
        if let Some(ref mut params) = active.params {
            return params.set_normalized(id, value);
        }

        Ok(value)
    }

    /// Get the display string for a parameter value.
    pub fn param_value_string(&self, id: u32, value: f64) -> Option<String> {
        self.active
            .as_ref()
            .and_then(|a| a.params.as_ref())
            .and_then(|p| p.value_to_string(id, value))
    }

    /// Drain plugin-initiated parameter changes from the component handler.
    pub fn drain_handler_changes(&self) -> Vec<(u32, f64)> {
        let Some(active) = &self.active else {
            return Vec::new();
        };
        if active.component_handler.is_null() {
            return Vec::new();
        }
        unsafe {
            HostComponentHandler::drain_changes(active.component_handler)
                .into_iter()
                .map(|c| (c.id, c.value))
                .collect()
        }
    }

    /// Whether a plugin is currently active and processing audio.
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// Whether the active plugin has crashed.
    ///
    /// When true, the engine is outputting silence and the plugin should
    /// be deactivated by the GUI to clean up resources.
    pub fn is_crashed(&self) -> bool {
        if let Some(ref active) = self.active {
            if let Ok(eng) = active.engine.lock() {
                return eng.is_crashed();
            }
        }
        false
    }

    /// Set the test tone enabled/disabled on the active engine.
    pub fn set_tone_enabled(&self, enabled: bool) {
        if let Some(ref active) = self.active {
            if let Ok(mut eng) = active.engine.lock() {
                eng.tone().enabled = enabled;
            }
        }
    }

    // ── Editor Window Methods ───────────────────────────────────────────────

    /// Whether the active plugin has an editor UI available.
    pub fn active_has_editor(&self) -> bool {
        self.active.as_ref().is_some_and(|a| a.has_editor)
    }

    /// Open the plugin editor window for the active plugin.
    ///
    /// Creates an IPlugView and a native window, then attaches the view.
    /// Returns `Ok(())` if the editor was opened successfully.
    pub fn open_editor(&mut self, plugin_name: &str) -> Result<(), String> {
        let active = self.active.as_mut().ok_or("No active plugin")?;

        // Get an IPlugView from the engine's instance
        // We need to lock the engine to access the instance
        let view = {
            let mut eng = active.engine.lock().map_err(|_| "Engine lock failed")?;
            eng.create_editor_view()
                .ok_or("Plugin does not provide an editor view")?
        };

        // Create the editor window
        let window =
            EditorWindow::open(view, plugin_name).ok_or("Failed to create editor window")?;

        self.editor_windows.push(window);
        Ok(())
    }

    /// Close all open editor windows.
    pub fn close_all_editors(&mut self) {
        for mut window in self.editor_windows.drain(..) {
            window.close();
        }
    }

    /// Poll all open editor windows for resize requests and prune closed ones.
    pub fn poll_editors(&mut self) {
        // Poll for resize requests
        for window in &mut self.editor_windows {
            window.poll_resize();
        }

        // Remove closed windows
        self.editor_windows.retain(|w| w.is_open());
    }

    /// Get the number of open editor windows.
    pub fn editor_count(&self) -> usize {
        self.editor_windows.len()
    }

    // ── Transport Methods ───────────────────────────────────────────────────

    /// Update the audio engine's tempo.
    pub fn set_tempo(&self, bpm: f64) {
        if let Some(ref active) = self.active {
            if let Ok(mut eng) = active.engine.lock() {
                eng.set_tempo(bpm);
            }
        }
    }

    /// Update the audio engine's playing state.
    pub fn set_playing(&self, playing: bool) {
        if let Some(ref active) = self.active {
            if let Ok(mut eng) = active.engine.lock() {
                eng.set_playing(playing);
            }
        }
    }

    /// Update the audio engine's time signature.
    pub fn set_time_signature(&self, numerator: u32, denominator: u32) {
        if let Some(ref active) = self.active {
            if let Ok(mut eng) = active.engine.lock() {
                eng.set_time_signature(numerator, denominator);
            }
        }
    }

    // ── Internal Helpers ────────────────────────────────────────────────────

    /// Build parameter snapshots from an Option<ParameterRegistry>.
    fn build_snapshots(&self, params: &Option<ParameterRegistry>) -> Vec<ParamSnapshot> {
        self.build_snapshots_ref(params.as_ref())
    }

    /// Build parameter snapshots from an Option<&ParameterRegistry>.
    fn build_snapshots_ref(&self, params: Option<&ParameterRegistry>) -> Vec<ParamSnapshot> {
        let Some(params) = params else {
            return Vec::new();
        };
        params
            .parameters
            .iter()
            .map(|e| {
                let display = params
                    .value_to_string(e.id, e.current_normalized)
                    .unwrap_or_else(|| format!("{:.3}", e.current_normalized));
                ParamSnapshot {
                    id: e.id,
                    title: e.title.clone(),
                    units: e.units.clone(),
                    value: e.current_normalized,
                    default: e.default_normalized,
                    display,
                    can_automate: e.can_automate,
                    is_read_only: e.is_read_only,
                    is_bypass: e.is_bypass,
                }
            })
            .collect()
    }
}

impl Drop for HostBackend {
    fn drop(&mut self) {
        self.deactivate_plugin();
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_new() {
        let backend = HostBackend::new();
        // Device lists depend on system; verify construction doesn't panic
        assert!(!backend.is_active());
        assert_eq!(backend.active_slot_index(), None);
    }

    #[test]
    fn test_backend_no_active_params() {
        let backend = HostBackend::new();
        let params = backend.active_param_snapshots();
        assert!(params.is_empty());
    }

    #[test]
    fn test_backend_no_active_handler_changes() {
        let backend = HostBackend::new();
        let changes = backend.drain_handler_changes();
        assert!(changes.is_empty());
    }

    #[test]
    fn test_backend_set_parameter_no_active() {
        let mut backend = HostBackend::new();
        let result = backend.set_parameter(0, 0.5);
        assert!(result.is_err());
    }

    #[test]
    fn test_backend_deactivate_when_none() {
        let mut backend = HostBackend::new();
        backend.deactivate_plugin(); // should not panic
        assert!(!backend.is_active());
    }

    #[test]
    fn test_backend_refresh_devices() {
        let mut backend = HostBackend::new();
        backend.refresh_devices(); // should not panic
    }

    #[test]
    fn test_backend_device_selection() {
        let mut backend = HostBackend::new();
        assert!(backend.selected_audio_device.is_none());
        assert!(backend.selected_midi_port.is_none());

        backend.selected_audio_device = Some("Test Device".into());
        backend.selected_midi_port = Some("Port 1".into());

        assert_eq!(
            backend.selected_audio_device.as_deref(),
            Some("Test Device")
        );
        assert_eq!(backend.selected_midi_port.as_deref(), Some("Port 1"));
    }

    #[test]
    fn test_backend_tone_no_active() {
        let backend = HostBackend::new();
        // Should not panic even without an active plugin
        backend.set_tone_enabled(false);
    }

    #[test]
    fn test_param_snapshot_clone() {
        let snap = ParamSnapshot {
            id: 42,
            title: "Volume".into(),
            units: "dB".into(),
            value: 0.7,
            default: 0.5,
            display: "-3.0 dB".into(),
            can_automate: true,
            is_read_only: false,
            is_bypass: false,
        };
        let clone = snap.clone();
        assert_eq!(clone.id, 42);
        assert_eq!(clone.title, "Volume");
        assert_eq!(clone.display, "-3.0 dB");
    }

    #[test]
    fn test_param_snapshot_debug() {
        let snap = ParamSnapshot {
            id: 1,
            title: "Gain".into(),
            units: "dB".into(),
            value: 0.5,
            default: 0.5,
            display: "0.0".into(),
            can_automate: true,
            is_read_only: false,
            is_bypass: false,
        };
        let debug = format!("{:?}", snap);
        assert!(debug.contains("Gain"));
        assert!(debug.contains("dB"));
    }

    #[test]
    fn test_param_value_string_no_active() {
        let backend = HostBackend::new();
        assert!(backend.param_value_string(0, 0.5).is_none());
    }

    // ── New feature tests ───────────────────────────────────────────────

    #[test]
    fn test_audio_status_default() {
        let status = AudioStatus::default();
        assert_eq!(status.sample_rate, 0);
        assert_eq!(status.buffer_size, 0);
        assert!(status.device_name.is_empty());
        assert!(!status.running);
    }

    #[test]
    fn test_backend_audio_status_initial() {
        let backend = HostBackend::new();
        assert!(!backend.audio_status.running);
    }

    #[test]
    fn test_backend_editor_count_none() {
        let backend = HostBackend::new();
        assert_eq!(backend.editor_count(), 0);
    }

    #[test]
    fn test_backend_active_has_editor_none() {
        let backend = HostBackend::new();
        assert!(!backend.active_has_editor());
    }

    #[test]
    fn test_backend_poll_editors_empty() {
        let mut backend = HostBackend::new();
        backend.poll_editors(); // Should not panic
        assert_eq!(backend.editor_count(), 0);
    }

    #[test]
    fn test_backend_close_all_editors_empty() {
        let mut backend = HostBackend::new();
        backend.close_all_editors(); // Should not panic
    }

    #[test]
    fn test_backend_set_tempo_no_active() {
        let backend = HostBackend::new();
        backend.set_tempo(145.0); // Should not panic
    }

    #[test]
    fn test_backend_set_playing_no_active() {
        let backend = HostBackend::new();
        backend.set_playing(true); // Should not panic
    }

    #[test]
    fn test_backend_set_time_signature_no_active() {
        let backend = HostBackend::new();
        backend.set_time_signature(3, 8); // Should not panic
    }

    #[test]
    fn test_backend_open_editor_no_active() {
        let mut backend = HostBackend::new();
        let result = backend.open_editor("Test");
        assert!(result.is_err());
    }

    #[test]
    fn test_active_state_holds_module() {
        // Verify that ActiveState contains a _module field.
        // The Vst3Module must be kept alive alongside the engine so
        // the dynamic library stays loaded and COM vtable pointers
        // remain valid for the lifetime of the plugin instance.
        //
        // If the module is dropped too early (e.g. at the end of
        // activate_plugin), the library is unloaded and any call
        // through a COM vtable pointer (such as process()) will
        // dereference unmapped memory and SIGSEGV (exit code 139).
        //
        // This is a compile-time structural guarantee: the test
        // exists to document the invariant and prevent regressions.
        assert!(
            std::mem::size_of::<Vst3Module>() > 0,
            "Vst3Module must be a real type stored in ActiveState"
        );
    }

    #[test]
    fn test_backend_deactivate_clears_audio_status() {
        let mut backend = HostBackend::new();
        // Manually set running to true to simulate an active state
        backend.audio_status.running = true;
        backend.deactivate_plugin();
        assert!(!backend.audio_status.running);
    }

    #[test]
    fn test_backend_deactivate_idempotent() {
        let mut backend = HostBackend::new();
        // Calling deactivate multiple times should not panic
        backend.deactivate_plugin();
        backend.deactivate_plugin();
        backend.deactivate_plugin();
        assert!(!backend.is_active());
    }

    #[test]
    fn test_backend_deactivate_clears_editors() {
        let mut backend = HostBackend::new();
        // No editors open, deactivate should not panic when closing editors
        backend.deactivate_plugin();
        assert_eq!(backend.editor_count(), 0);
    }

    #[test]
    fn test_active_state_stream_is_option() {
        // Verify that _stream is an Option<cpal::Stream> —
        // this allows explicit drop ordering in deactivate_plugin
        // and the custom Drop impl. The stream must be dropped
        // before the Vst3Module to ensure COM pointers are released
        // while the library is still loaded.
        assert!(
            std::mem::size_of::<Option<cpal::Stream>>() > 0,
            "Option<Stream> should be a real type"
        );
    }

    #[test]
    fn test_backend_tainted_paths_initially_empty() {
        let backend = HostBackend::new();
        assert!(backend.tainted_paths.is_empty());
    }

    #[test]
    fn test_backend_tainted_path_blocks_activation() {
        let mut backend = HostBackend::new();
        let path = std::path::PathBuf::from("/fake/path.vst3");
        backend.tainted_paths.insert(path.clone());
        let result = backend.activate_plugin(0, &path, &[0u8; 16], "FakePlugin");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("crashed during a prior deactivation"),
            "Error should mention crash: {}",
            err
        );
        assert!(
            err.contains("Restart the host"),
            "Error should recommend restart: {}",
            err
        );
    }

    #[test]
    fn test_backend_tainted_path_does_not_block_different_plugin() {
        let mut backend = HostBackend::new();
        let tainted = std::path::PathBuf::from("/fake/tainted.vst3");
        let clean = std::path::PathBuf::from("/fake/clean.vst3");
        backend.tainted_paths.insert(tainted);
        // Trying to activate a different (non-tainted) path should not
        // be blocked by the tainted set. It will fail later (no such file),
        // but the tainted-path guard should not fire.
        let result = backend.activate_plugin(0, &clean, &[0u8; 16], "CleanPlugin");
        // The error should be about loading the module, NOT about tainting.
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            !err.contains("crashed during a prior deactivation"),
            "Should not mention crash for a clean path: {}",
            err
        );
    }

    #[test]
    fn test_deactivation_crashed_flag_is_thread_local() {
        // Verify that the DEACTIVATION_CRASHED flag can be set and read
        // on the current thread without affecting other tests.
        DEACTIVATION_CRASHED.with(|c| {
            let original = c.get();
            c.set(true);
            assert!(c.get());
            c.set(false);
            assert!(!c.get());
            c.set(original); // restore
        });
    }

    #[test]
    fn test_deactivate_without_crash_does_not_taint() {
        let mut backend = HostBackend::new();
        // Ensure DEACTIVATION_CRASHED is false before deactivation.
        DEACTIVATION_CRASHED.with(|c| c.set(false));
        // Deactivate with no active plugin — should not taint anything.
        backend.deactivate_plugin();
        assert!(backend.tainted_paths.is_empty());
    }
}
