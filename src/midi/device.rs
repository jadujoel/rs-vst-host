//! MIDI device enumeration and input capture via `midir`.
//!
//! Handles listing MIDI input ports and opening a connection that
//! forwards raw MIDI messages to a lock-free queue for the audio thread.

use midir::{MidiInput, MidiInputConnection, MidiInputPort};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Information about a MIDI input port.
#[derive(Debug, Clone)]
pub struct MidiPortInfo {
    /// Display name of the port.
    pub name: String,
    /// Port index (for selection).
    pub index: usize,
}

/// Manages MIDI input device access.
pub struct MidiDevice {
    /// The underlying midir input.
    midi_in: MidiInput,
}

impl MidiDevice {
    /// Create a new MIDI device manager.
    pub fn new() -> Result<Self, String> {
        let midi_in = MidiInput::new("rs-vst-host")
            .map_err(|e| format!("Failed to create MIDI input: {}", e))?;
        Ok(Self { midi_in })
    }

    /// List available MIDI input ports.
    pub fn list_input_ports(&self) -> Vec<MidiPortInfo> {
        let ports = self.midi_in.ports();
        ports
            .iter()
            .enumerate()
            .map(|(i, port)| {
                let name = self
                    .midi_in
                    .port_name(port)
                    .unwrap_or_else(|_| format!("Port {}", i));
                MidiPortInfo { name, index: i }
            })
            .collect()
    }

    /// Find a MIDI input port by name (case-insensitive substring match).
    pub fn find_port_by_name(&self, name: &str) -> Option<MidiInputPort> {
        let name_lower = name.to_lowercase();
        let ports = self.midi_in.ports();
        for port in &ports {
            if let Ok(port_name) = self.midi_in.port_name(port) {
                if port_name.to_lowercase().contains(&name_lower) {
                    return Some(port.clone());
                }
            }
        }
        None
    }

    /// Get a MIDI input port by index.
    pub fn get_port_by_index(&self, index: usize) -> Option<MidiInputPort> {
        let ports = self.midi_in.ports();
        ports.get(index).cloned()
    }

    /// Open a MIDI input connection.
    ///
    /// The `callback` receives `(timestamp_us, &[u8])` for each MIDI message.
    /// Returns the connection handle which must be kept alive.
    pub fn open_connection<F>(
        self,
        port: &MidiInputPort,
        callback: F,
    ) -> Result<MidiInputConnection<()>, String>
    where
        F: Fn(u64, &[u8]) + Send + 'static,
    {
        let port_name = self
            .midi_in
            .port_name(port)
            .unwrap_or_else(|_| "unknown".into());

        info!(port = %port_name, "Opening MIDI input connection");

        self.midi_in
            .connect(
                port,
                "rs-vst-host-input",
                move |timestamp_us, data, _| {
                    callback(timestamp_us, data);
                },
                (),
            )
            .map_err(|e| format!("Failed to open MIDI port '{}': {}", port_name, e))
    }
}

/// Open a MIDI input connection that sends raw messages to a lock-free queue.
///
/// This is the primary way to capture MIDI for the audio thread.
/// Returns the connection handle (must be kept alive) and the port name.
pub fn open_midi_input(
    port_name: Option<&str>,
) -> Result<(MidiInputConnection<()>, String, Arc<MidiReceiver>), String> {
    let device = MidiDevice::new()?;

    let port = if let Some(name) = port_name {
        device
            .find_port_by_name(name)
            .ok_or_else(|| format!("MIDI port '{}' not found", name))?
    } else {
        // Use first available port
        device
            .get_port_by_index(0)
            .ok_or_else(|| "No MIDI input ports available".to_string())?
    };

    let actual_name = {
        let temp = MidiDevice::new()?;
        temp.midi_in
            .port_name(&port)
            .unwrap_or_else(|_| "unknown".into())
    };

    let receiver = Arc::new(MidiReceiver::new());
    let recv_clone = receiver.clone();

    let connection = device.open_connection(&port, move |timestamp_us, data| {
        recv_clone.push(timestamp_us, data);
    })?;

    debug!(port = %actual_name, "MIDI input connection established");
    Ok((connection, actual_name, receiver))
}

/// Lock-free MIDI message receiver.
///
/// Uses a simple ring buffer to pass MIDI messages from the input thread
/// to the audio thread without blocking.
pub struct MidiReceiver {
    /// Ring buffer of MIDI messages.
    buffer: std::sync::Mutex<Vec<RawMidiMessage>>,
}

/// A raw MIDI message with timestamp.
#[derive(Debug, Clone)]
pub struct RawMidiMessage {
    /// Timestamp in microseconds from midir.
    #[allow(dead_code)]
    pub timestamp_us: u64,
    /// Raw MIDI bytes (typically 1-3 bytes).
    pub data: [u8; 3],
    /// Number of valid bytes in `data`.
    pub len: u8,
}

impl MidiReceiver {
    /// Create a new MIDI receiver.
    pub fn new() -> Self {
        Self {
            buffer: std::sync::Mutex::new(Vec::with_capacity(256)),
        }
    }

    /// Push a raw MIDI message (called from the MIDI input thread).
    pub fn push(&self, timestamp_us: u64, data: &[u8]) {
        if data.is_empty() || data.len() > 3 {
            return;
        }

        let mut msg = RawMidiMessage {
            timestamp_us,
            data: [0; 3],
            len: data.len() as u8,
        };
        msg.data[..data.len()].copy_from_slice(data);

        if let Ok(mut buf) = self.buffer.lock() {
            // Cap at reasonable limit to avoid unbounded growth
            if buf.len() < 4096 {
                buf.push(msg);
            } else {
                warn!("MIDI buffer overflow, dropping message");
            }
        }
    }

    /// Drain all pending MIDI messages (called from the audio thread).
    ///
    /// Returns the messages collected since the last drain.
    pub fn drain(&self) -> Vec<RawMidiMessage> {
        if let Ok(mut buf) = self.buffer.try_lock() {
            let messages = buf.clone();
            buf.clear();
            messages
        } else {
            Vec::new()
        }
    }

    /// Check if there are pending messages without draining.
    #[allow(dead_code)]
    pub fn has_pending(&self) -> bool {
        if let Ok(buf) = self.buffer.try_lock() {
            !buf.is_empty()
        } else {
            false
        }
    }
}

impl Default for MidiReceiver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_midi_receiver_push_and_drain() {
        let recv = MidiReceiver::new();

        recv.push(1000, &[0x90, 60, 100]); // Note On
        recv.push(2000, &[0x80, 60, 0]); // Note Off

        let messages = recv.drain();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].data[0], 0x90);
        assert_eq!(messages[0].data[1], 60);
        assert_eq!(messages[0].data[2], 100);
        assert_eq!(messages[0].len, 3);
        assert_eq!(messages[1].data[0], 0x80);
    }

    #[test]
    fn test_midi_receiver_drain_clears() {
        let recv = MidiReceiver::new();
        recv.push(0, &[0x90, 60, 100]);

        let first = recv.drain();
        assert_eq!(first.len(), 1);

        let second = recv.drain();
        assert_eq!(second.len(), 0);
    }

    #[test]
    fn test_midi_receiver_ignores_empty() {
        let recv = MidiReceiver::new();
        recv.push(0, &[]);

        let messages = recv.drain();
        assert_eq!(messages.len(), 0);
    }

    #[test]
    fn test_midi_receiver_ignores_oversized() {
        let recv = MidiReceiver::new();
        recv.push(0, &[0x90, 60, 100, 0xFF]); // 4 bytes - too large

        let messages = recv.drain();
        assert_eq!(messages.len(), 0);
    }

    #[test]
    fn test_midi_receiver_has_pending() {
        let recv = MidiReceiver::new();
        assert!(!recv.has_pending());

        recv.push(0, &[0x90, 60, 100]);
        assert!(recv.has_pending());

        recv.drain();
        assert!(!recv.has_pending());
    }

    #[test]
    fn test_midi_port_info() {
        let info = MidiPortInfo {
            name: "Test Port".to_string(),
            index: 0,
        };
        assert_eq!(info.name, "Test Port");
        assert_eq!(info.index, 0);
    }

    #[test]
    fn test_raw_midi_message_clone() {
        let msg = RawMidiMessage {
            timestamp_us: 12345,
            data: [0x90, 60, 100],
            len: 3,
        };
        let cloned = msg.clone();
        assert_eq!(cloned.timestamp_us, 12345);
        assert_eq!(cloned.data, [0x90, 60, 100]);
        assert_eq!(cloned.len, 3);
    }
}
