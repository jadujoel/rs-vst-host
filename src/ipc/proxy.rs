//! Host-side plugin process proxy — spawns and communicates with plugin worker processes.
//!
//! [`PluginProcess`] manages the lifecycle of a child process running a single
//! VST3 plugin. It provides a high-level API that mirrors much of what
//! `AudioEngine` + `Vst3Instance` provide, but all communication is via IPC.
//!
//! # Audio Processing Flow
//!
//! ```text
//! Host audio callback:
//!   1. Write input audio to shared memory
//!   2. Send Process message over socket (with MIDI events, param changes)
//!   3. Wait for Processed response
//!   4. Read output audio from shared memory
//! ```

use crate::ipc::messages::*;
use crate::ipc::shm::ShmAudioBuffer;
use std::io::Write;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use tracing::{debug, error, info, warn};

/// A plugin running in an isolated child process.
///
/// Manages:
/// - Child process lifecycle (spawn, monitor, kill)
/// - IPC communication (Unix socket)
/// - Shared memory for audio buffers
/// - Transport and parameter state
pub struct PluginProcess {
    /// The child process handle.
    child: Child,
    /// Unix domain socket for IPC.
    stream: std::os::unix::net::UnixStream,
    /// Path to the socket file (for cleanup).
    socket_path: PathBuf,
    /// Shared memory audio buffer (host side).
    shm: Option<ShmAudioBuffer>,
    /// Plugin name.
    pub name: String,
    /// Number of input channels.
    pub input_channels: usize,
    /// Number of output channels.
    pub output_channels: usize,
    /// Whether the plugin has a GUI editor.
    pub has_editor: bool,
    /// Whether the plugin has crashed.
    crashed: bool,
    /// Whether the process has been shut down.
    is_shutdown: bool,
    /// Transport state.
    transport: TransportState,
    /// Pending parameter changes from the GUI thread.
    pending_param_changes: std::sync::Arc<std::sync::Mutex<Vec<(u32, f64)>>>,
    /// Cached plugin-initiated parameter changes.
    #[allow(dead_code)]
    handler_changes: Vec<ParamChange>,
}

// Safety: PluginProcess is accessed from the audio thread via Arc<Mutex<>>
// (same pattern as AudioEngine).
unsafe impl Send for PluginProcess {}

impl PluginProcess {
    /// Spawn a new plugin process for the given VST3 plugin.
    ///
    /// This creates a Unix socket, spawns the child process, waits for
    /// it to connect, then sends the LoadPlugin + Configure + Activate
    /// sequence.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        plugin_path: &Path,
        cid: &[u8; 16],
        name: &str,
        sample_rate: f64,
        max_block_size: i32,
        output_channels: u32,
        input_arrangement: u64,
        output_arrangement: u64,
    ) -> Result<Self, String> {
        let _span = tracing::info_span!("spawn_plugin_process", plugin = name).entered();

        // Create a unique socket path
        let socket_path = std::env::temp_dir().join(format!(
            "rs-vst-host-{}-{}.sock",
            std::process::id(),
            name.replace(|c: char| !c.is_alphanumeric(), "_")
        ));

        // Remove stale socket if it exists
        let _ = std::fs::remove_file(&socket_path);

        // Create Unix listener
        let listener = UnixListener::bind(&socket_path).map_err(|e| {
            format!(
                "Failed to create socket at '{}': {}",
                socket_path.display(),
                e
            )
        })?;

        // Set a timeout for the accept
        listener
            .set_nonblocking(false)
            .map_err(|e| format!("Failed to set socket blocking: {}", e))?;

        // Get the path to our own executable
        let exe_path = std::env::current_exe()
            .map_err(|e| format!("Failed to get current executable path: {}", e))?;

        // Spawn child process with the worker subcommand
        let child = Command::new(&exe_path)
            .arg("worker")
            .arg("--socket")
            .arg(socket_path.to_str().unwrap_or(""))
            .spawn()
            .map_err(|e| format!("Failed to spawn plugin process: {}", e))?;

        info!(
            pid = child.id(),
            plugin = %name,
            "Spawned plugin worker process"
        );

        // Wait for the child to connect (with timeout)
        // We use SO_RCVTIMEO on the listener's accept via a thread
        let stream = {
            use std::time::Duration;
            listener
                .set_nonblocking(false)
                .map_err(|e| format!("set_nonblocking failed: {}", e))?;

            // Use a simple accept with a timeout approach
            let (stream, _addr) = {
                // Set the listener to blocking with a deadline
                let start = std::time::Instant::now();
                let timeout = Duration::from_secs(10);
                listener
                    .set_nonblocking(true)
                    .map_err(|e| format!("set_nonblocking failed: {}", e))?;

                loop {
                    match listener.accept() {
                        Ok(result) => break result,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            if start.elapsed() > timeout {
                                return Err(format!(
                                    "Timed out waiting for worker to connect ({}s)",
                                    timeout.as_secs()
                                ));
                            }
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(e) => {
                            return Err(format!("Accept failed: {}", e));
                        }
                    }
                }
            };

            // Set stream to blocking for normal operation
            stream
                .set_nonblocking(false)
                .map_err(|e| format!("set_nonblocking failed: {}", e))?;
            // Set read timeout for safety
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(30)))
                .map_err(|e| format!("set_read_timeout failed: {}", e))?;
            stream
        };

        info!("Worker connected");

        let mut process = Self {
            child,
            stream,
            socket_path: socket_path.clone(),
            shm: None,
            name: name.to_string(),
            input_channels: 0,
            output_channels: 0,
            has_editor: false,
            crashed: false,
            is_shutdown: false,
            transport: TransportState::default(),
            pending_param_changes: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            handler_changes: Vec::new(),
        };

        // Send LoadPlugin
        let response = process.send_receive(HostMessage::LoadPlugin {
            path: plugin_path.to_str().unwrap_or("").to_string(),
            cid: *cid,
            name: name.to_string(),
        })?;

        match response {
            WorkerResponse::PluginLoaded {
                name: loaded_name,
                input_channels,
                output_channels: out_ch,
                has_editor,
            } => {
                process.name = loaded_name;
                process.input_channels = input_channels;
                process.output_channels = out_ch;
                process.has_editor = has_editor;
            }
            WorkerResponse::Error { message } => return Err(message),
            _ => return Err("Unexpected response to LoadPlugin".to_string()),
        }

        // Send Configure
        let response = process.send_receive(HostMessage::Configure {
            sample_rate,
            max_block_size,
            output_channels,
            input_arrangement,
            output_arrangement,
        })?;

        match response {
            WorkerResponse::Configured => {}
            WorkerResponse::Error { message } => return Err(message),
            _ => return Err("Unexpected response to Configure".to_string()),
        }

        // Open the shared memory created by the worker
        let shm_name = format!("/rs-vst-host-{}", process.child.id());
        let shm = ShmAudioBuffer::open(
            &shm_name,
            process.input_channels,
            process.output_channels,
            max_block_size as usize,
        )
        .map_err(|e| format!("Failed to open shared memory: {}", e))?;
        process.shm = Some(shm);

        // Send Activate
        let response = process.send_receive(HostMessage::Activate)?;
        match response {
            WorkerResponse::Activated => {}
            WorkerResponse::Error { message } => return Err(message),
            _ => return Err("Unexpected response to Activate".to_string()),
        }

        info!(plugin = %name, "Plugin process fully activated");

        Ok(process)
    }

    /// Send a message to the worker and receive the response.
    fn send_receive(&mut self, msg: HostMessage) -> Result<WorkerResponse, String> {
        let bytes = encode_message(&msg)?;
        self.stream
            .write_all(&bytes)
            .map_err(|e| format!("Failed to write to worker: {}", e))?;
        self.stream
            .flush()
            .map_err(|e| format!("Failed to flush: {}", e))?;

        let response: Option<WorkerResponse> = decode_message(&mut self.stream)?;
        response.ok_or_else(|| "Worker closed connection unexpectedly".to_string())
    }

    /// Process one audio block.
    ///
    /// Called from the audio callback. Writes input audio to shared memory,
    /// sends the Process command with MIDI events and parameter changes,
    /// waits for the response, then reads output from shared memory.
    pub fn process(
        &mut self,
        output: &mut [f32],
        device_channels: usize,
        midi_events: Vec<MidiEvent>,
    ) {
        if self.is_shutdown || self.crashed {
            output.fill(0.0);
            return;
        }

        if device_channels == 0 {
            return;
        }

        let num_samples = output.len() / device_channels;
        if num_samples == 0 {
            return;
        }

        let shm = match self.shm.as_ref() {
            Some(s) => s,
            None => {
                output.fill(0.0);
                return;
            }
        };

        let num_samples = num_samples.min(shm.max_block_size());

        // Gather pending parameter changes from the GUI thread
        let param_changes: Vec<ParamChange> =
            if let Ok(mut pending) = self.pending_param_changes.try_lock() {
                pending
                    .drain(..)
                    .map(|(id, value)| ParamChange {
                        id,
                        sample_offset: 0,
                        value,
                    })
                    .collect()
            } else {
                Vec::new()
            };

        // Write input to shared memory (test tone would be generated by host)
        // For now, the input is silence unless the caller has written to shm
        shm.set_num_samples(num_samples as u32);
        shm.clear_ready();

        // Send process command
        let msg = HostMessage::Process {
            num_samples: num_samples as i32,
            events: midi_events,
            param_changes,
            transport: self.transport.clone(),
        };

        match self.send_receive(msg) {
            Ok(WorkerResponse::Processed) => {
                // Read output from shared memory into interleaved cpal buffer
                self.read_output_interleaved(output, device_channels, num_samples);
            }
            Ok(WorkerResponse::Crashed {
                signal,
                context,
                backtrace: _,
            }) => {
                error!(
                    signal = %signal,
                    context = %context,
                    "Plugin crashed in worker process"
                );
                self.crashed = true;
                output.fill(0.0);
            }
            Ok(WorkerResponse::Error { message }) => {
                error!(error = %message, "Worker process error");
                output.fill(0.0);
            }
            Ok(_) => {
                warn!("Unexpected response from worker during process");
                output.fill(0.0);
            }
            Err(e) => {
                error!(error = %e, "IPC error during process — plugin may have crashed");
                self.crashed = true;
                output.fill(0.0);
            }
        }

        // Advance transport
        self.transport.project_time_samples += num_samples as i64;
    }

    /// Read output audio from shared memory into an interleaved output buffer.
    fn read_output_interleaved(
        &self,
        output: &mut [f32],
        device_channels: usize,
        num_samples: usize,
    ) {
        let shm = match self.shm.as_ref() {
            Some(s) => s,
            None => return,
        };

        let out_channels = shm.output_channels();

        for frame in 0..num_samples {
            for ch in 0..device_channels {
                let idx = frame * device_channels + ch;
                if idx >= output.len() {
                    return;
                }
                if ch < out_channels {
                    if let Some(buf) = unsafe { shm.output_channel(ch) } {
                        output[idx] = buf[frame];
                    } else {
                        output[idx] = 0.0;
                    }
                } else {
                    // Mirror first channel for extra device channels
                    if out_channels > 0 {
                        if let Some(buf) = unsafe { shm.output_channel(0) } {
                            output[idx] = buf[frame];
                        } else {
                            output[idx] = 0.0;
                        }
                    } else {
                        output[idx] = 0.0;
                    }
                }
            }
        }
    }

    /// Write test tone input to shared memory.
    pub fn write_input_tone(&self, tone_buffer: &[f32], num_samples: usize) {
        let shm = match self.shm.as_ref() {
            Some(s) => s,
            None => return,
        };

        for ch in 0..shm.input_channels() {
            if let Some(buf) = unsafe { shm.input_channel_mut(ch) } {
                let n = num_samples.min(buf.len()).min(tone_buffer.len());
                buf[..n].copy_from_slice(&tone_buffer[..n]);
            }
        }
    }

    /// Shut down the plugin process.
    pub fn shutdown(&mut self) {
        if self.is_shutdown {
            return;
        }
        self.is_shutdown = true;

        // Try graceful shutdown
        match self.send_receive(HostMessage::Shutdown) {
            Ok(WorkerResponse::ShutdownAck) => {
                debug!("Worker process acknowledged shutdown");
            }
            Ok(other) => {
                warn!(response = ?other, "Unexpected shutdown response");
            }
            Err(e) => {
                warn!(error = %e, "Failed to send shutdown — killing worker");
            }
        }

        // Wait for the child with a timeout, then kill if necessary
        let timeout = std::time::Duration::from_secs(5);
        let start = std::time::Instant::now();
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    info!(
                        status = ?status,
                        pid = self.child.id(),
                        "Worker process exited"
                    );
                    break;
                }
                Ok(None) => {
                    if start.elapsed() > timeout {
                        warn!("Worker not exiting — sending SIGKILL");
                        let _ = self.child.kill();
                        let _ = self.child.wait();
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(e) => {
                    warn!(error = %e, "Error waiting for worker");
                    break;
                }
            }
        }
    }

    /// Whether the plugin has crashed.
    pub fn is_crashed(&self) -> bool {
        self.crashed
    }

    /// Whether the plugin has been shut down.
    pub fn is_shutdown(&self) -> bool {
        self.is_shutdown
    }

    /// Get a clone of the pending parameter changes queue.
    pub fn pending_param_queue(&self) -> std::sync::Arc<std::sync::Mutex<Vec<(u32, f64)>>> {
        self.pending_param_changes.clone()
    }

    /// Query plugin parameters from the worker.
    pub fn query_parameters(&mut self) -> Result<Vec<ParamInfo>, String> {
        let response = self.send_receive(HostMessage::QueryParameters)?;
        match response {
            WorkerResponse::Parameters { params } => Ok(params),
            WorkerResponse::Error { message } => Err(message),
            _ => Err("Unexpected response to QueryParameters".to_string()),
        }
    }

    /// Set a parameter on the worker's controller.
    pub fn set_parameter_on_worker(&mut self, id: u32, value: f64) -> Result<f64, String> {
        let response = self.send_receive(HostMessage::SetParameter { id, value })?;
        match response {
            WorkerResponse::ParameterSet { value } => Ok(value),
            WorkerResponse::Error { message } => Err(message),
            _ => Err("Unexpected response to SetParameter".to_string()),
        }
    }

    /// Get the component state from the sandboxed plugin.
    ///
    /// Returns the binary state blob or an error description.
    pub fn get_state(&self) -> Result<Vec<u8>, String> {
        // Need &mut for send_receive; use interior mutability pattern
        let self_ptr = self as *const Self as *mut Self;
        let response = unsafe { (*self_ptr).send_receive(HostMessage::GetState)? };
        match response {
            WorkerResponse::State { data } => Ok(data),
            WorkerResponse::Error { message } => Err(message),
            _ => Err("Unexpected response to GetState".to_string()),
        }
    }

    /// Restore component state on the sandboxed plugin.
    ///
    /// `data` should be a blob previously obtained from [`get_state`].
    pub fn set_state(&mut self, data: &[u8]) -> Result<(), String> {
        let response = self.send_receive(HostMessage::SetState {
            data: data.to_vec(),
        })?;
        match response {
            WorkerResponse::StateLoaded => Ok(()),
            WorkerResponse::Error { message } => Err(message),
            _ => Err("Unexpected response to SetState".to_string()),
        }
    }

    /// Set the tempo in BPM.
    pub fn set_tempo(&mut self, bpm: f64) {
        self.transport.tempo = bpm;
    }

    /// Set the playing state.
    pub fn set_playing(&mut self, playing: bool) {
        self.transport.playing = playing;
    }

    /// Set the time signature.
    pub fn set_time_signature(&mut self, numerator: u32, denominator: u32) {
        self.transport.time_sig_numerator = numerator as i32;
        self.transport.time_sig_denominator = denominator as i32;
    }

    /// Get the plugin name.
    pub fn plugin_name(&self) -> &str {
        &self.name
    }

    /// Check if the worker process is still alive.
    pub fn is_alive(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(Some(_)) => false,
            Ok(None) => true,
            Err(_) => false,
        }
    }

    /// Ping the worker process (health check).
    pub fn ping(&mut self) -> bool {
        matches!(
            self.send_receive(HostMessage::Ping),
            Ok(WorkerResponse::Pong)
        )
    }

    /// Get the child process PID.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }
}

impl Drop for PluginProcess {
    fn drop(&mut self) {
        if !self.is_shutdown {
            self.shutdown();
        }
        // Clean up the socket file
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_state_default() {
        let t = TransportState::default();
        assert!(!t.playing);
        assert!((t.tempo - 120.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_read_output_interleaved_silence() {
        // Without shared memory, output should be unchanged (function returns early)
        let process = PluginProcess {
            child: std::process::Command::new("true").spawn().unwrap(),
            stream: {
                // Create a dummy socket pair for testing
                let (s1, _s2) = std::os::unix::net::UnixStream::pair().unwrap();
                s1
            },
            socket_path: PathBuf::from("/tmp/test-proxy-dummy.sock"),
            shm: None,
            name: "test".into(),
            input_channels: 0,
            output_channels: 2,
            has_editor: false,
            crashed: false,
            is_shutdown: true,
            transport: TransportState::default(),
            pending_param_changes: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            handler_changes: Vec::new(),
        };

        let mut output = vec![999.0f32; 64];
        // read_output_interleaved returns early when no shm
        process.read_output_interleaved(&mut output, 2, 32);
        // Output should be unchanged (function returns early)
        assert!((output[0] - 999.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_plugin_process_transport_setters() {
        let mut process = PluginProcess {
            child: std::process::Command::new("true").spawn().unwrap(),
            stream: {
                let (s1, _s2) = std::os::unix::net::UnixStream::pair().unwrap();
                s1
            },
            socket_path: PathBuf::from("/tmp/test-proxy-transport.sock"),
            shm: None,
            name: "test".into(),
            input_channels: 0,
            output_channels: 2,
            has_editor: false,
            crashed: false,
            is_shutdown: true,
            transport: TransportState::default(),
            pending_param_changes: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            handler_changes: Vec::new(),
        };

        process.set_tempo(140.0);
        assert!((process.transport.tempo - 140.0).abs() < f64::EPSILON);

        process.set_playing(true);
        assert!(process.transport.playing);

        process.set_time_signature(3, 4);
        assert_eq!(process.transport.time_sig_numerator, 3);
        assert_eq!(process.transport.time_sig_denominator, 4);
    }

    #[test]
    fn test_plugin_process_crashed_state() {
        let process = PluginProcess {
            child: std::process::Command::new("true").spawn().unwrap(),
            stream: {
                let (s1, _s2) = std::os::unix::net::UnixStream::pair().unwrap();
                s1
            },
            socket_path: PathBuf::from("/tmp/test-proxy-crashed.sock"),
            shm: None,
            name: "test".into(),
            input_channels: 0,
            output_channels: 2,
            has_editor: false,
            crashed: true,
            is_shutdown: true,
            transport: TransportState::default(),
            pending_param_changes: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            handler_changes: Vec::new(),
        };

        assert!(process.is_crashed());
        assert!(process.is_shutdown());
    }

    #[test]
    fn test_pending_param_queue() {
        let process = PluginProcess {
            child: std::process::Command::new("true").spawn().unwrap(),
            stream: {
                let (s1, _s2) = std::os::unix::net::UnixStream::pair().unwrap();
                s1
            },
            socket_path: PathBuf::from("/tmp/test-proxy-params.sock"),
            shm: None,
            name: "test".into(),
            input_channels: 0,
            output_channels: 2,
            has_editor: false,
            crashed: false,
            is_shutdown: true,
            transport: TransportState::default(),
            pending_param_changes: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            handler_changes: Vec::new(),
        };

        let queue = process.pending_param_queue();
        queue.lock().unwrap().push((1, 0.5));
        queue.lock().unwrap().push((2, 0.8));
        assert_eq!(queue.lock().unwrap().len(), 2);
    }

    #[test]
    fn test_process_outputs_silence_when_crashed() {
        let mut process = PluginProcess {
            child: std::process::Command::new("true").spawn().unwrap(),
            stream: {
                let (s1, _s2) = std::os::unix::net::UnixStream::pair().unwrap();
                s1
            },
            socket_path: PathBuf::from("/tmp/test-proxy-crash-silence.sock"),
            shm: None,
            name: "test".into(),
            input_channels: 0,
            output_channels: 2,
            has_editor: false,
            crashed: true,
            is_shutdown: false,
            transport: TransportState::default(),
            pending_param_changes: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            handler_changes: Vec::new(),
        };

        let mut output = vec![1.0f32; 64];
        process.process(&mut output, 2, Vec::new());
        assert!(output.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn test_process_outputs_silence_when_shutdown() {
        let mut process = PluginProcess {
            child: std::process::Command::new("true").spawn().unwrap(),
            stream: {
                let (s1, _s2) = std::os::unix::net::UnixStream::pair().unwrap();
                s1
            },
            socket_path: PathBuf::from("/tmp/test-proxy-shutdown-silence.sock"),
            shm: None,
            name: "test".into(),
            input_channels: 0,
            output_channels: 2,
            has_editor: false,
            crashed: false,
            is_shutdown: true,
            transport: TransportState::default(),
            pending_param_changes: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            handler_changes: Vec::new(),
        };

        let mut output = vec![1.0f32; 64];
        process.process(&mut output, 2, Vec::new());
        assert!(output.iter().all(|&s| s == 0.0));
    }
}
