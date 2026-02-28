//! IPC message protocol for host ↔ plugin process communication.
//!
//! All messages are serialized as JSON over Unix domain sockets.
//! The protocol is request/response: the host sends a [`HostMessage`],
//! the worker replies with a [`WorkerResponse`].

use serde::{Deserialize, Serialize};

// ─── Host → Worker messages ─────────────────────────────────────────────

/// Messages sent from the host process to the plugin worker process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HostMessage {
    /// Load a VST3 plugin from a bundle path.
    LoadPlugin {
        /// Path to the .vst3 bundle.
        path: String,
        /// 16-byte class ID of the plugin to instantiate.
        cid: [u8; 16],
        /// Display name for logging.
        name: String,
    },

    /// Configure the audio processing parameters.
    Configure {
        /// Sample rate in Hz.
        sample_rate: f64,
        /// Maximum block size in samples.
        max_block_size: i32,
        /// Number of output channels.
        output_channels: u32,
        /// Input speaker arrangement (bitmask).
        input_arrangement: u64,
        /// Output speaker arrangement (bitmask).
        output_arrangement: u64,
    },

    /// Activate the plugin (setActive + startProcessing).
    Activate,

    /// Deactivate the plugin (stopProcessing + setActive(0)).
    Deactivate,

    /// Process one audio block.
    ///
    /// Audio data is transferred via shared memory, not in this message.
    /// The message carries only the metadata for the current block.
    Process {
        /// Number of samples in this block.
        num_samples: i32,
        /// MIDI events for this block.
        events: Vec<MidiEvent>,
        /// Parameter changes for this block.
        param_changes: Vec<ParamChange>,
        /// Transport state.
        transport: TransportState,
    },

    /// Set a parameter value on the controller (for display feedback).
    SetParameter {
        /// Parameter ID.
        id: u32,
        /// Normalized value [0..1].
        value: f64,
    },

    /// Query all plugin parameters.
    QueryParameters,

    /// Get the plugin state (for saving).
    GetState,

    /// Set the plugin state (for loading).
    SetState {
        /// Binary state data.
        data: Vec<u8>,
    },

    /// Check if the plugin has an editor view.
    HasEditor,

    /// Graceful shutdown — terminate and unload the plugin.
    Shutdown,

    /// Ping — used for health checks.
    Ping,
}

// ─── Worker → Host messages ─────────────────────────────────────────────

/// Responses sent from the plugin worker process back to the host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkerResponse {
    /// Plugin loaded successfully.
    PluginLoaded {
        /// Plugin display name.
        name: String,
        /// Number of input channels.
        input_channels: usize,
        /// Number of output channels.
        output_channels: usize,
        /// Whether the plugin has an editor.
        has_editor: bool,
    },

    /// Configuration applied successfully.
    Configured,

    /// Plugin activated successfully.
    Activated,

    /// Plugin deactivated successfully.
    Deactivated,

    /// Process block completed — output audio is in shared memory.
    Processed,

    /// Parameter set successfully.
    ParameterSet {
        /// Actual value after setting (read back from controller).
        value: f64,
    },

    /// Parameter list response.
    Parameters {
        /// All plugin parameters.
        params: Vec<ParamInfo>,
    },

    /// Plugin state data.
    State {
        /// Binary state data.
        data: Vec<u8>,
    },

    /// State loaded successfully.
    StateLoaded,

    /// Whether the plugin has an editor.
    EditorAvailable {
        /// True if the plugin has a GUI editor.
        has_editor: bool,
    },

    /// Shutdown acknowledged.
    ShutdownAck,

    /// Pong — response to Ping.
    Pong,

    /// An error occurred.
    Error {
        /// Error description.
        message: String,
    },

    /// The plugin crashed.
    Crashed {
        /// Signal that caused the crash.
        signal: String,
        /// Context of the crash.
        context: String,
        /// Crash backtrace frames.
        backtrace: Vec<String>,
    },

    /// Plugin-initiated parameter changes (from component handler).
    ///
    /// These are sent asynchronously when the plugin modifies its own
    /// parameters (e.g., linked parameter groups, preset changes).
    HandlerChanges {
        /// Parameter changes initiated by the plugin.
        changes: Vec<ParamChange>,
    },
}

// ─── Shared data types ──────────────────────────────────────────────────

/// A MIDI event for IPC transfer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidiEvent {
    /// Sample offset within the block.
    pub sample_offset: i32,
    /// MIDI channel (0-15).
    pub channel: i16,
    /// Event type.
    pub event_type: MidiEventType,
}

/// MIDI event types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MidiEventType {
    /// Note On with pitch and velocity.
    NoteOn {
        /// MIDI note number (0-127).
        pitch: i16,
        /// Velocity [0.0..1.0].
        velocity: f32,
    },
    /// Note Off with pitch and velocity.
    NoteOff {
        /// MIDI note number (0-127).
        pitch: i16,
        /// Release velocity [0.0..1.0].
        velocity: f32,
    },
}

/// A parameter change for IPC transfer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamChange {
    /// Parameter ID.
    pub id: u32,
    /// Sample offset within the block (0 for immediate).
    pub sample_offset: i32,
    /// Normalized value [0..1].
    pub value: f64,
}

/// Transport state for IPC transfer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportState {
    /// Whether transport is playing.
    pub playing: bool,
    /// Tempo in BPM.
    pub tempo: f64,
    /// Time signature numerator.
    pub time_sig_numerator: i32,
    /// Time signature denominator.
    pub time_sig_denominator: i32,
    /// Project time in samples.
    pub project_time_samples: i64,
}

impl Default for TransportState {
    fn default() -> Self {
        Self {
            playing: false,
            tempo: 120.0,
            time_sig_numerator: 4,
            time_sig_denominator: 4,
            project_time_samples: 0,
        }
    }
}

/// Parameter info for IPC transfer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamInfo {
    /// Parameter ID.
    pub id: u32,
    /// Display title.
    pub title: String,
    /// Short title.
    pub short_title: String,
    /// Units label.
    pub units: String,
    /// Number of discrete steps (0 = continuous).
    pub step_count: i32,
    /// Default normalized value [0..1].
    pub default_normalized: f64,
    /// Current normalized value [0..1].
    pub current_normalized: f64,
    /// Whether the parameter can be automated.
    pub can_automate: bool,
    /// Whether the parameter is read-only.
    pub is_read_only: bool,
    /// Whether this is a bypass parameter.
    pub is_bypass: bool,
}

// ─── Wire protocol helpers ──────────────────────────────────────────────

/// Framing: each message is a 4-byte little-endian length prefix followed
/// by the JSON payload. This allows reliable reading from a stream socket.
pub const MAX_MESSAGE_SIZE: usize = 16 * 1024 * 1024; // 16 MB max

/// Encode a message to bytes with a 4-byte length prefix.
///
/// Serializes directly into the output buffer to avoid an intermediate allocation.
pub fn encode_message<T: Serialize>(msg: &T) -> Result<Vec<u8>, String> {
    // Write a placeholder length prefix, then serialize directly after it.
    let mut buf = Vec::with_capacity(256);
    buf.extend_from_slice(&[0u8; 4]);
    serde_json::to_writer(&mut buf, msg).map_err(|e| format!("Serialize error: {}", e))?;
    let json_len = buf.len() - 4;
    if json_len > MAX_MESSAGE_SIZE {
        return Err(format!(
            "Message too large: {} bytes (max {})",
            json_len, MAX_MESSAGE_SIZE
        ));
    }
    buf[..4].copy_from_slice(&(json_len as u32).to_le_bytes());
    Ok(buf)
}

/// Decode a message from a stream, reading the length prefix first.
///
/// Returns `Ok(None)` if the stream is closed (EOF on length read).
pub fn decode_message<T: for<'de> Deserialize<'de>>(
    reader: &mut impl std::io::Read,
) -> Result<Option<T>, String> {
    let mut len_buf = [0u8; 4];
    match reader.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(format!("Read length error: {}", e)),
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > MAX_MESSAGE_SIZE {
        return Err(format!(
            "Message too large: {} bytes (max {})",
            len, MAX_MESSAGE_SIZE
        ));
    }
    let mut payload = vec![0u8; len];
    reader
        .read_exact(&mut payload)
        .map_err(|e| format!("Read payload error: {}", e))?;
    let msg: T =
        serde_json::from_slice(&payload).map_err(|e| format!("Deserialize error: {}", e))?;
    Ok(Some(msg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_message_serialize_roundtrip_load() {
        let msg = HostMessage::LoadPlugin {
            path: "/Library/Audio/Plug-Ins/VST3/Test.vst3".into(),
            cid: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
            name: "Test Plugin".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let decoded: HostMessage = serde_json::from_str(&json).unwrap();
        match decoded {
            HostMessage::LoadPlugin { path, cid, name } => {
                assert_eq!(path, "/Library/Audio/Plug-Ins/VST3/Test.vst3");
                assert_eq!(cid[0], 1);
                assert_eq!(cid[15], 16);
                assert_eq!(name, "Test Plugin");
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_host_message_serialize_roundtrip_configure() {
        let msg = HostMessage::Configure {
            sample_rate: 44100.0,
            max_block_size: 1024,
            output_channels: 2,
            input_arrangement: 0x03,
            output_arrangement: 0x03,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let decoded: HostMessage = serde_json::from_str(&json).unwrap();
        match decoded {
            HostMessage::Configure {
                sample_rate,
                max_block_size,
                ..
            } => {
                assert!((sample_rate - 44100.0).abs() < f64::EPSILON);
                assert_eq!(max_block_size, 1024);
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_host_message_serialize_roundtrip_process() {
        let msg = HostMessage::Process {
            num_samples: 512,
            events: vec![MidiEvent {
                sample_offset: 0,
                channel: 0,
                event_type: MidiEventType::NoteOn {
                    pitch: 60,
                    velocity: 0.8,
                },
            }],
            param_changes: vec![ParamChange {
                id: 42,
                sample_offset: 0,
                value: 0.5,
            }],
            transport: TransportState::default(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let decoded: HostMessage = serde_json::from_str(&json).unwrap();
        match decoded {
            HostMessage::Process {
                num_samples,
                events,
                param_changes,
                transport,
            } => {
                assert_eq!(num_samples, 512);
                assert_eq!(events.len(), 1);
                assert_eq!(param_changes.len(), 1);
                assert!((transport.tempo - 120.0).abs() < f64::EPSILON);
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_worker_response_serialize_roundtrip() {
        let resp = WorkerResponse::PluginLoaded {
            name: "TestSynth".into(),
            input_channels: 0,
            output_channels: 2,
            has_editor: true,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let decoded: WorkerResponse = serde_json::from_str(&json).unwrap();
        match decoded {
            WorkerResponse::PluginLoaded {
                name,
                input_channels,
                output_channels,
                has_editor,
            } => {
                assert_eq!(name, "TestSynth");
                assert_eq!(input_channels, 0);
                assert_eq!(output_channels, 2);
                assert!(has_editor);
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_worker_response_error() {
        let resp = WorkerResponse::Error {
            message: "Plugin not found".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let decoded: WorkerResponse = serde_json::from_str(&json).unwrap();
        match decoded {
            WorkerResponse::Error { message } => {
                assert_eq!(message, "Plugin not found");
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_worker_response_crashed() {
        let resp = WorkerResponse::Crashed {
            signal: "SIGSEGV".into(),
            context: "process".into(),
            backtrace: vec!["frame1".into(), "frame2".into()],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let decoded: WorkerResponse = serde_json::from_str(&json).unwrap();
        match decoded {
            WorkerResponse::Crashed {
                signal,
                context,
                backtrace,
            } => {
                assert_eq!(signal, "SIGSEGV");
                assert_eq!(context, "process");
                assert_eq!(backtrace.len(), 2);
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let msg = HostMessage::Ping;
        let bytes = encode_message(&msg).unwrap();
        assert_eq!(&bytes[..4], &(bytes.len() as u32 - 4).to_le_bytes());

        let mut cursor = std::io::Cursor::new(&bytes);
        let decoded: HostMessage = decode_message(&mut cursor).unwrap().unwrap();
        match decoded {
            HostMessage::Ping => {}
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_decode_eof_returns_none() {
        let mut cursor = std::io::Cursor::new(Vec::<u8>::new());
        let result: Result<Option<HostMessage>, String> = decode_message(&mut cursor);
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_encode_oversized_message() {
        // Create a message that exceeds MAX_MESSAGE_SIZE when serialized
        let msg = HostMessage::SetState {
            data: vec![0u8; MAX_MESSAGE_SIZE + 1],
        };
        let result = encode_message(&msg);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("too large"));
    }

    #[test]
    fn test_transport_state_default() {
        let t = TransportState::default();
        assert!(!t.playing);
        assert!((t.tempo - 120.0).abs() < f64::EPSILON);
        assert_eq!(t.time_sig_numerator, 4);
        assert_eq!(t.time_sig_denominator, 4);
        assert_eq!(t.project_time_samples, 0);
    }

    #[test]
    fn test_param_info_serialize() {
        let info = ParamInfo {
            id: 1,
            title: "Volume".into(),
            short_title: "Vol".into(),
            units: "dB".into(),
            step_count: 0,
            default_normalized: 0.7,
            current_normalized: 0.5,
            can_automate: true,
            is_read_only: false,
            is_bypass: false,
        };
        let json = serde_json::to_string(&info).unwrap();
        let decoded: ParamInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.id, 1);
        assert_eq!(decoded.title, "Volume");
        assert_eq!(decoded.units, "dB");
    }

    #[test]
    fn test_midi_event_note_on_serialize() {
        let event = MidiEvent {
            sample_offset: 128,
            channel: 0,
            event_type: MidiEventType::NoteOn {
                pitch: 60,
                velocity: 0.75,
            },
        };
        let json = serde_json::to_string(&event).unwrap();
        let decoded: MidiEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.sample_offset, 128);
        assert_eq!(decoded.channel, 0);
        match decoded.event_type {
            MidiEventType::NoteOn { pitch, velocity } => {
                assert_eq!(pitch, 60);
                assert!((velocity - 0.75).abs() < f32::EPSILON);
            }
            _ => panic!("Wrong event type"),
        }
    }

    #[test]
    fn test_midi_event_note_off_serialize() {
        let event = MidiEvent {
            sample_offset: 256,
            channel: 3,
            event_type: MidiEventType::NoteOff {
                pitch: 72,
                velocity: 0.0,
            },
        };
        let json = serde_json::to_string(&event).unwrap();
        let decoded: MidiEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.channel, 3);
        match decoded.event_type {
            MidiEventType::NoteOff { pitch, .. } => {
                assert_eq!(pitch, 72);
            }
            _ => panic!("Wrong event type"),
        }
    }

    #[test]
    fn test_all_host_message_variants_serialize() {
        let variants: Vec<HostMessage> = vec![
            HostMessage::LoadPlugin {
                path: "test.vst3".into(),
                cid: [0; 16],
                name: "test".into(),
            },
            HostMessage::Configure {
                sample_rate: 48000.0,
                max_block_size: 512,
                output_channels: 2,
                input_arrangement: 3,
                output_arrangement: 3,
            },
            HostMessage::Activate,
            HostMessage::Deactivate,
            HostMessage::Process {
                num_samples: 256,
                events: vec![],
                param_changes: vec![],
                transport: TransportState::default(),
            },
            HostMessage::SetParameter { id: 1, value: 0.5 },
            HostMessage::QueryParameters,
            HostMessage::GetState,
            HostMessage::SetState {
                data: vec![1, 2, 3],
            },
            HostMessage::HasEditor,
            HostMessage::Shutdown,
            HostMessage::Ping,
        ];
        for msg in &variants {
            let json = serde_json::to_string(msg).unwrap();
            let _: HostMessage = serde_json::from_str(&json).unwrap();
        }
    }

    #[test]
    fn test_all_worker_response_variants_serialize() {
        let variants: Vec<WorkerResponse> = vec![
            WorkerResponse::PluginLoaded {
                name: "t".into(),
                input_channels: 2,
                output_channels: 2,
                has_editor: false,
            },
            WorkerResponse::Configured,
            WorkerResponse::Activated,
            WorkerResponse::Deactivated,
            WorkerResponse::Processed,
            WorkerResponse::ParameterSet { value: 0.5 },
            WorkerResponse::Parameters { params: vec![] },
            WorkerResponse::State { data: vec![] },
            WorkerResponse::StateLoaded,
            WorkerResponse::EditorAvailable { has_editor: true },
            WorkerResponse::ShutdownAck,
            WorkerResponse::Pong,
            WorkerResponse::Error {
                message: "err".into(),
            },
            WorkerResponse::Crashed {
                signal: "SIGSEGV".into(),
                context: "test".into(),
                backtrace: vec![],
            },
            WorkerResponse::HandlerChanges { changes: vec![] },
        ];
        for resp in &variants {
            let json = serde_json::to_string(resp).unwrap();
            let _: WorkerResponse = serde_json::from_str(&json).unwrap();
        }
    }

    #[test]
    fn test_param_change_serialize() {
        let change = ParamChange {
            id: 42,
            sample_offset: 128,
            value: 0.75,
        };
        let json = serde_json::to_string(&change).unwrap();
        let decoded: ParamChange = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.id, 42);
        assert_eq!(decoded.sample_offset, 128);
        assert!((decoded.value - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn test_encode_decode_complex_process_message() {
        let msg = HostMessage::Process {
            num_samples: 1024,
            events: vec![
                MidiEvent {
                    sample_offset: 0,
                    channel: 0,
                    event_type: MidiEventType::NoteOn {
                        pitch: 60,
                        velocity: 1.0,
                    },
                },
                MidiEvent {
                    sample_offset: 512,
                    channel: 0,
                    event_type: MidiEventType::NoteOff {
                        pitch: 60,
                        velocity: 0.0,
                    },
                },
            ],
            param_changes: vec![
                ParamChange {
                    id: 1,
                    sample_offset: 0,
                    value: 0.5,
                },
                ParamChange {
                    id: 2,
                    sample_offset: 256,
                    value: 0.8,
                },
            ],
            transport: TransportState {
                playing: true,
                tempo: 140.0,
                time_sig_numerator: 3,
                time_sig_denominator: 4,
                project_time_samples: 44100,
            },
        };

        let bytes = encode_message(&msg).unwrap();
        let mut cursor = std::io::Cursor::new(&bytes);
        let decoded: HostMessage = decode_message(&mut cursor).unwrap().unwrap();

        match decoded {
            HostMessage::Process {
                num_samples,
                events,
                param_changes,
                transport,
            } => {
                assert_eq!(num_samples, 1024);
                assert_eq!(events.len(), 2);
                assert_eq!(param_changes.len(), 2);
                assert!(transport.playing);
                assert!((transport.tempo - 140.0).abs() < f64::EPSILON);
            }
            _ => panic!("Wrong variant"),
        }
    }
}
