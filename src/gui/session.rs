//! Session save/load — serialize rack state and transport to JSON.
//!
//! A session captures the current host state: transport settings, rack
//! plugin slots, and selected devices. This allows the user to save
//! their setup and restore it later.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Session file format version.
const SESSION_VERSION: &str = "1.0";

/// A serializable session containing the full host state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// File format version.
    pub version: String,
    /// Transport state snapshot.
    pub transport: TransportSnapshot,
    /// Plugin rack slots.
    pub rack: Vec<SlotSnapshot>,
    /// Selected audio output device name.
    pub audio_device: Option<String>,
    /// Selected MIDI input port name.
    pub midi_port: Option<String>,
}

/// Serializable transport state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportSnapshot {
    /// Tempo in BPM.
    pub tempo: f64,
    /// Time signature numerator.
    pub time_sig_num: u32,
    /// Time signature denominator.
    pub time_sig_den: u32,
}

/// Serializable rack slot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotSnapshot {
    /// Plugin display name.
    pub name: String,
    /// Plugin vendor.
    pub vendor: String,
    /// Plugin category.
    pub category: String,
    /// Path to the .vst3 bundle.
    pub path: PathBuf,
    /// Class ID for instantiation.
    pub cid: [u8; 16],
    /// Whether the slot is bypassed.
    pub bypassed: bool,
}

impl Session {
    /// Create a session from current host state.
    pub fn capture(
        transport: &super::app::TransportState,
        rack: &[super::app::PluginSlot],
        audio_device: Option<String>,
        midi_port: Option<String>,
    ) -> Self {
        Session {
            version: SESSION_VERSION.to_string(),
            transport: TransportSnapshot {
                tempo: transport.tempo,
                time_sig_num: transport.time_sig_num,
                time_sig_den: transport.time_sig_den,
            },
            rack: rack
                .iter()
                .map(|slot| SlotSnapshot {
                    name: slot.name.clone(),
                    vendor: slot.vendor.clone(),
                    category: slot.category.clone(),
                    path: slot.path.clone(),
                    cid: slot.cid,
                    bypassed: slot.bypassed,
                })
                .collect(),
            audio_device,
            midi_port,
        }
    }

    /// Restore transport and rack state from this session.
    pub fn restore(&self) -> (super::app::TransportState, Vec<super::app::PluginSlot>) {
        let transport = super::app::TransportState {
            playing: false,
            tempo: self.transport.tempo,
            time_sig_num: self.transport.time_sig_num,
            time_sig_den: self.transport.time_sig_den,
        };

        let rack = self
            .rack
            .iter()
            .map(|snap| super::app::PluginSlot {
                name: snap.name.clone(),
                vendor: snap.vendor.clone(),
                category: snap.category.clone(),
                path: snap.path.clone(),
                cid: snap.cid,
                bypassed: snap.bypassed,
            })
            .collect();

        (transport, rack)
    }

    /// Save session to a JSON file.
    pub fn save_to_file(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, &json)?;
        tracing::info!(path = %path.display(), "Session saved");
        Ok(())
    }

    /// Load session from a JSON file.
    pub fn load_from_file(path: &Path) -> anyhow::Result<Self> {
        let json = std::fs::read_to_string(path)?;
        let session: Session = serde_json::from_str(&json)?;
        tracing::info!(path = %path.display(), "Session loaded");
        Ok(session)
    }
}

/// Get the default sessions directory.
pub fn sessions_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("rs-vst-host").join("sessions"))
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui::app::{PluginSlot, TransportState};
    use std::path::PathBuf;

    fn sample_transport() -> TransportState {
        TransportState {
            playing: true,
            tempo: 140.0,
            time_sig_num: 3,
            time_sig_den: 4,
        }
    }

    fn sample_rack() -> Vec<PluginSlot> {
        vec![
            PluginSlot {
                name: "TestSynth".into(),
                vendor: "TestVendor".into(),
                category: "Audio Module Class".into(),
                path: PathBuf::from("/Library/Audio/Plug-Ins/VST3/TestSynth.vst3"),
                cid: [1u8; 16],
                bypassed: false,
            },
            PluginSlot {
                name: "TestEQ".into(),
                vendor: "OtherVendor".into(),
                category: "Audio Module Class".into(),
                path: PathBuf::from("/Library/Audio/Plug-Ins/VST3/TestEQ.vst3"),
                cid: [2u8; 16],
                bypassed: true,
            },
        ]
    }

    #[test]
    fn test_session_capture() {
        let transport = sample_transport();
        let rack = sample_rack();

        let session = Session::capture(&transport, &rack, Some("Speakers".into()), None);

        assert_eq!(session.version, SESSION_VERSION);
        assert_eq!(session.transport.tempo, 140.0);
        assert_eq!(session.transport.time_sig_num, 3);
        assert_eq!(session.transport.time_sig_den, 4);
        assert_eq!(session.rack.len(), 2);
        assert_eq!(session.rack[0].name, "TestSynth");
        assert_eq!(session.rack[1].bypassed, true);
        assert_eq!(session.audio_device, Some("Speakers".into()));
        assert_eq!(session.midi_port, None);
    }

    #[test]
    fn test_session_restore() {
        let transport = sample_transport();
        let rack = sample_rack();
        let session = Session::capture(&transport, &rack, None, Some("Port 1".into()));

        let (restored_transport, restored_rack) = session.restore();

        // Playing is always reset to false on restore
        assert!(!restored_transport.playing);
        assert_eq!(restored_transport.tempo, 140.0);
        assert_eq!(restored_transport.time_sig_num, 3);
        assert_eq!(restored_rack.len(), 2);
        assert_eq!(restored_rack[0].name, "TestSynth");
        assert_eq!(restored_rack[1].name, "TestEQ");
        assert_eq!(restored_rack[1].bypassed, true);
    }

    #[test]
    fn test_session_roundtrip_serde() {
        let transport = sample_transport();
        let rack = sample_rack();
        let session = Session::capture(
            &transport,
            &rack,
            Some("Device".into()),
            Some("MIDI".into()),
        );

        let json = serde_json::to_string_pretty(&session).unwrap();
        let restored: Session = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.version, session.version);
        assert_eq!(restored.transport.tempo, session.transport.tempo);
        assert_eq!(restored.rack.len(), session.rack.len());
        assert_eq!(restored.rack[0].cid, session.rack[0].cid);
        assert_eq!(restored.audio_device, session.audio_device);
        assert_eq!(restored.midi_port, session.midi_port);
    }

    #[test]
    fn test_session_file_roundtrip() {
        let dir = std::env::temp_dir().join("rs-vst-host-test-session");
        let path = dir.join("test_session.json");

        let transport = sample_transport();
        let rack = sample_rack();
        let session = Session::capture(&transport, &rack, None, None);

        session.save_to_file(&path).unwrap();
        assert!(path.exists());

        let loaded = Session::load_from_file(&path).unwrap();
        assert_eq!(loaded.rack.len(), 2);
        assert_eq!(loaded.transport.tempo, 140.0);

        // Clean up
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_session_empty_rack() {
        let transport = TransportState::default();
        let session = Session::capture(&transport, &[], None, None);

        assert!(session.rack.is_empty());

        let (_, rack) = session.restore();
        assert!(rack.is_empty());
    }

    #[test]
    fn test_session_load_invalid_json() {
        let dir = std::env::temp_dir().join("rs-vst-host-test-session-bad");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.json");
        std::fs::write(&path, "not valid json").unwrap();

        let result = Session::load_from_file(&path);
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_session_load_missing_file() {
        let path = PathBuf::from("/nonexistent/dir/session.json");
        let result = Session::load_from_file(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_sessions_dir() {
        // Should return Some on most systems
        let dir = sessions_dir();
        if let Some(d) = &dir {
            assert!(d.to_string_lossy().contains("rs-vst-host"));
        }
    }

    #[test]
    fn test_session_version_constant() {
        assert_eq!(SESSION_VERSION, "1.0");
    }

    #[test]
    fn test_snapshot_cid_preservation() {
        let cid = [
            0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80, 0x90, 0xA0, 0xB0, 0xC0, 0xD0, 0xE0,
            0xF0, 0xFF,
        ];
        let slot = PluginSlot {
            name: "P".into(),
            vendor: "V".into(),
            category: "C".into(),
            path: PathBuf::from("/test"),
            cid,
            bypassed: false,
        };

        let session = Session::capture(&TransportState::default(), &[slot], None, None);
        let (_, rack) = session.restore();
        assert_eq!(rack[0].cid, cid);
    }
}
