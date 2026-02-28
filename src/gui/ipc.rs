//! IPC message protocol for supervisor ↔ GUI process communication.
//!
//! The supervisor process manages audio, plugins, and MIDI.
//! The GUI process renders the eframe/egui window and sends user actions.
//! All messages are JSON-framed over a Unix domain socket (same framing
//! as the plugin worker protocol in [`crate::ipc::messages`]).
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────┐         ┌─────────────────────┐
//! │  Supervisor Process  │         │   GUI Process        │
//! │  (audio + plugins)   │◄─sock──►│  (eframe window)     │
//! │                      │         │                      │
//! │  AudioEngine         │         │  HostApp (egui)      │
//! │  HostBackend         │         │  Renders UI          │
//! │  Plugin lifecycle    │         │  Sends GuiAction     │
//! └─────────────────────┘         └─────────────────────┘
//! ```

use crate::gui::backend::ParamSnapshot;
use crate::vst3::types::PluginModuleInfo;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ── GUI → Supervisor messages ───────────────────────────────────────────

/// Actions sent from the GUI process to the supervisor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GuiAction {
    /// Request a plugin scan.
    ScanPlugins,

    /// Add a plugin to the rack.
    AddToRack {
        /// Index into the plugin_modules list.
        module_index: usize,
        /// Index into the module's classes list.
        class_index: usize,
    },

    /// Remove a slot from the rack.
    RemoveFromRack {
        /// Rack slot index.
        index: usize,
    },

    /// Activate a plugin slot (start audio processing).
    ActivateSlot {
        /// Rack slot index.
        index: usize,
    },

    /// Deactivate the currently active plugin.
    DeactivateSlot,

    /// Set a parameter value on the active plugin.
    SetParameter {
        /// Parameter ID.
        id: u32,
        /// Normalized value [0..1].
        value: f64,
    },

    /// Stage a parameter change for an inactive plugin.
    StageParameter {
        /// Rack slot index.
        slot_index: usize,
        /// Parameter ID.
        id: u32,
        /// Normalized value [0..1].
        value: f64,
    },

    /// Select a rack slot (for parameter view).
    SelectSlot {
        /// Rack slot index, or None to deselect.
        index: Option<usize>,
    },

    /// Toggle the test tone.
    SetToneEnabled {
        /// Whether the tone should be enabled.
        enabled: bool,
    },

    /// Update transport state.
    SetTransport {
        /// Whether playback is active.
        playing: bool,
        /// Tempo in BPM.
        tempo: f64,
        /// Time signature numerator.
        time_sig_num: u32,
        /// Time signature denominator.
        time_sig_den: u32,
    },

    /// Open the plugin editor window.
    OpenEditor,

    /// Save session to file.
    SaveSession {
        /// File path to save to.
        path: String,
    },

    /// Load session from file.
    LoadSession {
        /// File path to load from.
        path: String,
    },

    /// Select audio output device.
    SelectAudioDevice {
        /// Device name, or None for default.
        name: Option<String>,
    },

    /// Select MIDI input port.
    SelectMidiPort {
        /// Port name, or None for no MIDI.
        name: Option<String>,
    },

    /// Refresh device lists.
    RefreshDevices,

    /// Toggle process isolation mode.
    SetProcessIsolation {
        /// Whether to enable process isolation.
        enabled: bool,
    },

    /// GUI is shutting down normally (window closed).
    Shutdown,

    /// Ping (keep-alive / health check).
    Ping,
}

// ── Supervisor → GUI messages ───────────────────────────────────────────

/// State updates sent from the supervisor to the GUI process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SupervisorUpdate {
    /// Full state sync — sent on initial connection and after major changes.
    FullState {
        /// Available plugin modules from scan cache.
        plugin_modules: Vec<PluginModuleInfo>,
        /// Current rack state.
        rack: Vec<RackSlotState>,
        /// Currently selected slot index.
        selected_slot: Option<usize>,
        /// Active slot index (processing audio).
        active_slot: Option<usize>,
        /// Parameter snapshots for the selected plugin.
        param_snapshots: Vec<ParamSnapshot>,
        /// Audio status.
        audio_status: AudioStatusState,
        /// Available audio devices.
        audio_devices: Vec<DeviceState>,
        /// Available MIDI ports.
        midi_ports: Vec<MidiPortState>,
        /// Selected audio device name.
        selected_audio_device: Option<String>,
        /// Selected MIDI port name.
        selected_midi_port: Option<String>,
        /// Whether process isolation is enabled.
        process_isolation: bool,
        /// Status message.
        status_message: String,
        /// Whether heap corruption has been detected.
        heap_corruption_detected: bool,
        /// Whether the active plugin has an editor.
        has_editor: bool,
        /// Tainted plugin path count (for display).
        tainted_count: usize,
        /// Transport state.
        transport: TransportUpdate,
        /// Whether tone is enabled.
        tone_enabled: bool,
        /// Whether safe mode is active.
        safe_mode: bool,
    },

    /// Incremental rack update.
    RackUpdated {
        /// Updated rack slots.
        rack: Vec<RackSlotState>,
        /// Active slot index.
        active_slot: Option<usize>,
        /// Selected slot.
        selected_slot: Option<usize>,
    },

    /// Parameter snapshots refreshed.
    ParamsUpdated {
        /// Updated parameter snapshots.
        snapshots: Vec<ParamSnapshot>,
    },

    /// Status message changed.
    StatusMessage {
        /// The new status message.
        message: String,
    },

    /// Audio status changed.
    AudioStatusUpdated {
        /// Updated audio status.
        status: AudioStatusState,
    },

    /// Plugin modules list updated (after scan).
    PluginModulesUpdated {
        /// Updated plugin modules.
        modules: Vec<PluginModuleInfo>,
    },

    /// Device lists updated.
    DevicesUpdated {
        /// Audio output devices.
        audio_devices: Vec<DeviceState>,
        /// MIDI input ports.
        midi_ports: Vec<MidiPortState>,
    },

    /// Heap corruption detected.
    HeapCorruptionDetected,

    /// Active plugin has editor availability changed.
    EditorAvailability {
        /// Whether the active plugin has an editor.
        has_editor: bool,
    },

    /// Pong response to GUI's Ping.
    Pong,

    /// Supervisor is shutting down.
    ShutdownAck,

    /// The audio process crashed and was restarted.
    ///
    /// The supervisor sends this to the GUI when the audio worker dies
    /// and is relaunched. Active plugins are lost but rack config is preserved.
    AudioProcessRestarted {
        /// Status message describing what happened.
        message: String,
        /// How many times the audio process has been restarted.
        restart_count: u32,
    },
}

// ── Shared state types (serializable) ───────────────────────────────────

/// Serializable rack slot state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RackSlotState {
    /// Display name.
    pub name: String,
    /// Vendor.
    pub vendor: String,
    /// Category.
    pub category: String,
    /// Path to the .vst3 bundle.
    pub path: PathBuf,
    /// Class ID.
    pub cid: [u8; 16],
    /// Whether the slot is bypassed.
    pub bypassed: bool,
    /// Cached parameter snapshots.
    pub param_cache: Vec<ParamSnapshot>,
    /// Staged parameter changes.
    pub staged_changes: Vec<(u32, f64)>,
}

/// Serializable audio status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioStatusState {
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Buffer size in frames.
    pub buffer_size: u32,
    /// Device name.
    pub device_name: String,
    /// Whether the audio engine is running.
    pub running: bool,
}

impl Default for AudioStatusState {
    fn default() -> Self {
        Self {
            sample_rate: 0,
            buffer_size: 0,
            device_name: String::new(),
            running: false,
        }
    }
}

/// Serializable audio device info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceState {
    /// Device name.
    pub name: String,
}

/// Serializable MIDI port info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidiPortState {
    /// Port name.
    pub name: String,
}

// ── Supervisor → Audio Worker messages ──────────────────────────────────

/// Commands sent from the supervisor to the audio worker process.
///
/// The audio worker runs the `HostBackend` and audio engine in a separate
/// process. The supervisor relays GUI actions to it and receives
/// `SupervisorUpdate` responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AudioCommand {
    /// Forward a GUI action to the audio worker for processing.
    Action(GuiAction),

    /// Request the audio worker to send its current full state.
    ///
    /// Used when the GUI process reconnects after a crash — the supervisor
    /// asks the audio worker for the latest state and forwards it.
    RequestFullState,

    /// Restore cached state after audio worker restart.
    ///
    /// The supervisor maintains a shadow copy of the rack, plugin modules,
    /// and other state. When the audio worker crashes and is restarted,
    /// this command seeds it with the last known configuration.
    RestoreState {
        /// Scanned plugin modules.
        plugin_modules: Vec<PluginModuleInfo>,
        /// Rack slot configuration.
        rack: Vec<RackSlotState>,
        /// Currently selected slot.
        selected_slot: Option<usize>,
        /// Whether the test tone is enabled.
        tone_enabled: bool,
        /// Transport state.
        transport: TransportUpdate,
        /// Session file path.
        session_path: String,
    },

    /// Shut down the audio worker gracefully.
    Shutdown,
}

// ── Shared state types (serializable) ───

/// Transport state for IPC.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportUpdate {
    /// Whether playback is active.
    pub playing: bool,
    /// Tempo in BPM.
    pub tempo: f64,
    /// Time signature numerator.
    pub time_sig_num: u32,
    /// Time signature denominator.
    pub time_sig_den: u32,
}

// ── Wire helpers (reuse from ipc::messages) ─────────────────────────────

/// Error type for GUI IPC decode operations.
#[derive(Debug)]
pub enum DecodeError {
    /// Timeout or would-block — no data available yet (expected during polling).
    Timeout,
    /// A real I/O or deserialization error.
    Other(String),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::Timeout => write!(f, "timeout"),
            DecodeError::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl DecodeError {
    /// Returns true if this is a timeout/would-block error (expected during polling).
    pub fn is_timeout(&self) -> bool {
        matches!(self, DecodeError::Timeout)
    }
}

/// Encode a GUI IPC message with length-prefix framing.
pub fn encode<T: Serialize>(msg: &T) -> Result<Vec<u8>, String> {
    crate::ipc::messages::encode_message(msg)
}

/// Decode a GUI IPC message from a stream.
///
/// Returns:
/// - `Ok(Some(msg))` — a message was decoded
/// - `Ok(None)` — EOF (peer disconnected)
/// - `Err(DecodeError::Timeout)` — no data available (normal during polling)
/// - `Err(DecodeError::Other(_))` — real error
pub fn decode<T: for<'de> Deserialize<'de>>(
    reader: &mut impl std::io::Read,
) -> Result<Option<T>, DecodeError> {
    match crate::ipc::messages::decode_message(reader) {
        Ok(msg) => Ok(msg),
        Err(e) => {
            // Check for timeout/would-block errors across platforms.
            // macOS: "Resource temporarily unavailable (os error 35)"
            // Linux: "Resource temporarily unavailable (os error 11)"
            // The underlying decode_message formats as "Read length error: <io error>"
            let lower = e.to_lowercase();
            if lower.contains("timed out")
                || lower.contains("would block")
                || lower.contains("wouldblock")
                || lower.contains("resource temporarily unavailable")
                || lower.contains("os error 35")
                || lower.contains("os error 11")
            {
                Err(DecodeError::Timeout)
            } else {
                Err(DecodeError::Other(e))
            }
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gui_action_serialize_roundtrip() {
        let actions = vec![
            GuiAction::ScanPlugins,
            GuiAction::AddToRack {
                module_index: 0,
                class_index: 1,
            },
            GuiAction::RemoveFromRack { index: 2 },
            GuiAction::ActivateSlot { index: 0 },
            GuiAction::DeactivateSlot,
            GuiAction::SetParameter {
                id: 42,
                value: 0.75,
            },
            GuiAction::StageParameter {
                slot_index: 0,
                id: 1,
                value: 0.5,
            },
            GuiAction::SelectSlot { index: Some(1) },
            GuiAction::SelectSlot { index: None },
            GuiAction::SetToneEnabled { enabled: true },
            GuiAction::SetTransport {
                playing: true,
                tempo: 140.0,
                time_sig_num: 3,
                time_sig_den: 4,
            },
            GuiAction::OpenEditor,
            GuiAction::SaveSession {
                path: "/tmp/test.json".into(),
            },
            GuiAction::LoadSession {
                path: "/tmp/test.json".into(),
            },
            GuiAction::SelectAudioDevice {
                name: Some("Test".into()),
            },
            GuiAction::SelectMidiPort { name: None },
            GuiAction::RefreshDevices,
            GuiAction::SetProcessIsolation { enabled: true },
            GuiAction::Shutdown,
            GuiAction::Ping,
        ];

        for action in &actions {
            let json = serde_json::to_string(action).expect("serialize");
            let decoded: GuiAction = serde_json::from_str(&json).expect("deserialize");
            let json2 = serde_json::to_string(&decoded).expect("re-serialize");
            assert_eq!(json, json2, "roundtrip failed for {:?}", action);
        }
    }

    #[test]
    fn test_supervisor_update_serialize_roundtrip() {
        let updates = vec![
            SupervisorUpdate::StatusMessage {
                message: "Hello".into(),
            },
            SupervisorUpdate::HeapCorruptionDetected,
            SupervisorUpdate::Pong,
            SupervisorUpdate::ShutdownAck,
            SupervisorUpdate::ParamsUpdated {
                snapshots: vec![ParamSnapshot {
                    id: 1,
                    title: "Volume".into(),
                    units: "dB".into(),
                    value: 0.5,
                    default: 0.5,
                    display: "0.0".into(),
                    can_automate: true,
                    is_read_only: false,
                    is_bypass: false,
                }],
            },
            SupervisorUpdate::AudioStatusUpdated {
                status: AudioStatusState::default(),
            },
            SupervisorUpdate::EditorAvailability { has_editor: true },
            SupervisorUpdate::AudioProcessRestarted {
                message: "Audio crashed".into(),
                restart_count: 3,
            },
        ];

        for update in &updates {
            let json = serde_json::to_string(update).expect("serialize");
            let decoded: SupervisorUpdate = serde_json::from_str(&json).expect("deserialize");
            let json2 = serde_json::to_string(&decoded).expect("re-serialize");
            assert_eq!(json, json2, "roundtrip failed for {:?}", update);
        }
    }

    #[test]
    fn test_rack_slot_state_serialize() {
        let slot = RackSlotState {
            name: "TestPlugin".into(),
            vendor: "TestVendor".into(),
            category: "Audio Module Class".into(),
            path: PathBuf::from("/test.vst3"),
            cid: [0u8; 16],
            bypassed: false,
            param_cache: Vec::new(),
            staged_changes: Vec::new(),
        };
        let json = serde_json::to_string(&slot).expect("serialize");
        let decoded: RackSlotState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded.name, "TestPlugin");
        assert_eq!(decoded.vendor, "TestVendor");
    }

    #[test]
    fn test_audio_status_state_default() {
        let status = AudioStatusState::default();
        assert_eq!(status.sample_rate, 0);
        assert_eq!(status.buffer_size, 0);
        assert!(status.device_name.is_empty());
        assert!(!status.running);
    }

    #[test]
    fn test_transport_update_serialize() {
        let transport = TransportUpdate {
            playing: true,
            tempo: 145.0,
            time_sig_num: 3,
            time_sig_den: 8,
        };
        let json = serde_json::to_string(&transport).expect("serialize");
        let decoded: TransportUpdate = serde_json::from_str(&json).expect("deserialize");
        assert!(decoded.playing);
        assert_eq!(decoded.tempo, 145.0);
        assert_eq!(decoded.time_sig_num, 3);
        assert_eq!(decoded.time_sig_den, 8);
    }

    #[test]
    fn test_encode_decode_gui_action() {
        let action = GuiAction::SetParameter {
            id: 42,
            value: 0.75,
        };
        let encoded = encode(&action).expect("encode");
        assert!(encoded.len() > 4); // length prefix + payload
        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: Option<GuiAction> = decode(&mut cursor).expect("decode");
        assert!(decoded.is_some());
    }

    #[test]
    fn test_encode_decode_supervisor_update() {
        let update = SupervisorUpdate::StatusMessage {
            message: "Test status".into(),
        };
        let encoded = encode(&update).expect("encode");
        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: Option<SupervisorUpdate> = decode(&mut cursor).expect("decode");
        assert!(decoded.is_some());
    }

    #[test]
    fn test_device_state_serialize() {
        let dev = DeviceState {
            name: "Built-in Output".into(),
        };
        let json = serde_json::to_string(&dev).expect("serialize");
        assert!(json.contains("Built-in Output"));
    }

    #[test]
    fn test_midi_port_state_serialize() {
        let port = MidiPortState {
            name: "IAC Driver".into(),
        };
        let json = serde_json::to_string(&port).expect("serialize");
        assert!(json.contains("IAC Driver"));
    }

    #[test]
    fn test_audio_command_serialize_roundtrip() {
        let commands = vec![
            AudioCommand::Action(GuiAction::Ping),
            AudioCommand::Action(GuiAction::ScanPlugins),
            AudioCommand::RequestFullState,
            AudioCommand::RestoreState {
                plugin_modules: Vec::new(),
                rack: Vec::new(),
                selected_slot: Some(0),
                tone_enabled: true,
                transport: TransportUpdate {
                    playing: true,
                    tempo: 128.0,
                    time_sig_num: 3,
                    time_sig_den: 4,
                },
                session_path: "/tmp/session.json".into(),
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

    #[test]
    fn test_audio_command_encode_decode() {
        let cmd = AudioCommand::Action(GuiAction::SetToneEnabled { enabled: true });
        let encoded = encode(&cmd).expect("encode");
        assert!(encoded.len() > 4);
        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: Option<AudioCommand> = decode(&mut cursor).expect("decode");
        assert!(decoded.is_some());
    }
}
