//! GUI supervisor — spawns both the GUI and audio worker in child processes.
//!
//! The supervisor is a lightweight coordinator that:
//!
//! 1. Spawns an **audio worker** child process (runs `HostBackend` + audio engine + plugins)
//! 2. Spawns a **GUI worker** child process (runs `eframe`/`egui` window)
//! 3. Relays [`GuiAction`] messages from GUI → audio worker
//! 4. Relays [`SupervisorUpdate`] messages from audio worker → GUI
//! 5. If the **GUI** crashes, relaunches it and re-syncs state from the audio worker
//! 6. If the **audio worker** crashes, relaunches it, restores cached state, and notifies the GUI
//!
//! This provides complete crash isolation: a plugin that corrupts the heap
//! in the audio process cannot bring down the GUI or the supervisor. The
//! supervisor simply restarts the audio worker. Similarly, a GUI crash
//! doesn't affect audio processing.
//!
//! # Process Architecture
//!
//! ```text
//! ┌─────────────────────────────┐
//! │   Supervisor Process        │
//! │   (lightweight relay)       │
//! │                             │
//! │   Spawns + monitors both    │
//! │   child processes.          │
//! │   Relays IPC messages.      │
//! │   Handles crash recovery.   │
//! └──────┬──────────────┬───────┘
//!        │              │
//!    ┌───▼───┐      ┌───▼──────────┐
//!    │  GUI  │      │ Audio Worker  │
//!    │ Child │      │    Child      │
//!    │       │      │              │
//!    │eframe │      │ HostBackend  │
//!    │egui   │      │ AudioEngine  │
//!    │       │      │ Plugins      │
//!    └───────┘      └──────────────┘
//! ```

use crate::gui::ipc::*;
use crate::vst3::types::PluginModuleInfo;

use std::io::Write;
use std::os::unix::net::UnixListener;
use std::process::{Child, Command};
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// Maximum number of rapid restarts before giving up (per child type).
const MAX_RAPID_RESTARTS: u32 = 5;
/// Time window for counting "rapid" restarts.
const RAPID_RESTART_WINDOW: Duration = Duration::from_secs(30);

/// Run the GUI supervisor loop.
///
/// This function blocks until the user closes the GUI (clean shutdown)
/// or the maximum restart count is exceeded within the rapid restart window.
///
/// # Arguments
/// * `safe_mode` — if true, no plugins loaded from cache on startup
/// * `malloc_debug` — if true, enable periodic heap checks
/// * `paths` — if non-empty, only these paths are used for plugin scanning (defaults excluded)
pub fn run_supervisor(
    safe_mode: bool,
    malloc_debug: bool,
    paths: Vec<std::path::PathBuf>,
) -> anyhow::Result<()> {
    let _span = tracing::info_span!("gui_supervisor").entered();
    info!(
        "Starting GUI supervisor (safe_mode={}, malloc_debug={}, custom_paths={})",
        safe_mode,
        malloc_debug,
        !paths.is_empty()
    );

    // ── Spawn audio worker ──────────────────────────────────────────────
    let mut audio_ctx = AudioWorkerContext::spawn(safe_mode, malloc_debug, &paths)?;

    // ── Shadow state for crash recovery ─────────────────────────────────
    let mut shadow = ShadowState::new(safe_mode);

    // ── GUI restart loop ────────────────────────────────────────────────
    let mut gui_restart_count: u32 = 0;
    let mut gui_first_restart_time = std::time::Instant::now();

    // ── Audio restart tracking ──────────────────────────────────────────
    let mut audio_restart_count: u32 = 0;
    let mut audio_first_restart_time = std::time::Instant::now();

    loop {
        // Check GUI rapid restart limit
        if gui_restart_count > 0 {
            if gui_first_restart_time.elapsed() > RAPID_RESTART_WINDOW {
                gui_restart_count = 0;
                gui_first_restart_time = std::time::Instant::now();
            }
            if gui_restart_count >= MAX_RAPID_RESTARTS {
                error!(
                    "GUI crashed {} times within {}s — giving up",
                    gui_restart_count,
                    RAPID_RESTART_WINDOW.as_secs()
                );
                // Shut down audio worker
                let _ = send_audio_command(&audio_ctx.stream, &AudioCommand::Shutdown);
                let _ = audio_ctx.child.wait();
                return Err(anyhow::anyhow!(
                    "GUI process crashed {} times rapidly — cannot recover",
                    gui_restart_count
                ));
            }
        }

        // ── Spawn GUI child process ────────────────────────────────────
        let gui_socket_path =
            std::env::temp_dir().join(format!("rs-vst-host-gui-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&gui_socket_path);

        let gui_listener = UnixListener::bind(&gui_socket_path).map_err(|e| {
            anyhow::anyhow!(
                "Failed to create GUI socket at '{}': {}",
                gui_socket_path.display(),
                e
            )
        })?;

        let exe_path = std::env::current_exe()?;
        let mut cmd = Command::new(&exe_path);
        cmd.arg("gui-worker")
            .arg("--socket")
            .arg(gui_socket_path.to_str().unwrap_or(""));
        if safe_mode {
            cmd.arg("--safe-mode");
        }
        if malloc_debug {
            cmd.arg("--malloc-debug");
        }

        let gui_child = cmd
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn GUI process: {}", e))?;

        let gui_pid = gui_child.id();
        info!(pid = gui_pid, "Spawned GUI process");

        // Accept GUI connection with timeout
        gui_listener.set_nonblocking(true).ok();
        let gui_stream = {
            let start = std::time::Instant::now();
            let timeout = Duration::from_secs(15);
            loop {
                match gui_listener.accept() {
                    Ok((stream, _)) => {
                        stream.set_nonblocking(false).ok();
                        stream
                            .set_read_timeout(Some(Duration::from_millis(50)))
                            .ok();
                        break stream;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        if start.elapsed() > timeout {
                            error!("Timed out waiting for GUI process to connect");
                            let _ = std::fs::remove_file(&gui_socket_path);
                            let _ = send_audio_command(&audio_ctx.stream, &AudioCommand::Shutdown);
                            let _ = audio_ctx.child.wait();
                            return Err(anyhow::anyhow!(
                                "GUI process did not connect within {}s",
                                timeout.as_secs()
                            ));
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    Err(e) => {
                        let _ = std::fs::remove_file(&gui_socket_path);
                        let _ = send_audio_command(&audio_ctx.stream, &AudioCommand::Shutdown);
                        let _ = audio_ctx.child.wait();
                        return Err(anyhow::anyhow!("Accept failed: {}", e));
                    }
                }
            }
        };

        info!("GUI process connected");

        // Request full state from audio worker and forward to GUI
        if let Err(e) = send_audio_command(&audio_ctx.stream, &AudioCommand::RequestFullState) {
            warn!(error = %e, "Failed to request full state from audio worker");
        }

        // Read back the full state from audio worker and forward to GUI
        if let Ok(Some(full_state)) = decode::<SupervisorUpdate>(&mut audio_ctx.reader()) {
            shadow.update_from(&full_state);
            if let Err(e) = send_gui_update(&gui_stream, &full_state) {
                warn!(error = %e, "Failed to send initial state to GUI");
            }
        }

        // ── Message relay loop ──────────────────────────────────────────
        let result = run_relay_loop(
            &gui_stream,
            gui_child,
            &mut audio_ctx,
            &mut shadow,
            &mut audio_restart_count,
            &mut audio_first_restart_time,
            safe_mode,
            malloc_debug,
            &paths,
        );

        // Clean up GUI socket
        let _ = std::fs::remove_file(&gui_socket_path);

        match result {
            LoopResult::CleanShutdown => {
                info!("GUI shut down cleanly — shutting down audio worker");
                let _ = send_audio_command(&audio_ctx.stream, &AudioCommand::Shutdown);
                let _ = audio_ctx.child.wait();
                break;
            }
            LoopResult::Crashed(reason) => {
                gui_restart_count += 1;
                if gui_restart_count == 1 {
                    gui_first_restart_time = std::time::Instant::now();
                }
                warn!(
                    reason = %reason,
                    restarts = gui_restart_count,
                    "GUI process crashed — restarting"
                );
                // Audio worker stays alive — brief pause before GUI restart
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }

    info!("Supervisor shut down");
    Ok(())
}

// ── Audio worker management ─────────────────────────────────────────────

/// Context for communicating with the audio worker child process.
struct AudioWorkerContext {
    /// The audio worker child process.
    child: Child,
    /// Unix socket stream to the audio worker.
    stream: std::os::unix::net::UnixStream,
    /// Socket path (for cleanup).
    socket_path: std::path::PathBuf,
}

impl AudioWorkerContext {
    /// Spawn a new audio worker child process and connect via Unix socket.
    fn spawn(
        safe_mode: bool,
        malloc_debug: bool,
        paths: &[std::path::PathBuf],
    ) -> anyhow::Result<Self> {
        let socket_path =
            std::env::temp_dir().join(format!("rs-vst-host-audio-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket_path);

        let listener = UnixListener::bind(&socket_path).map_err(|e| {
            anyhow::anyhow!(
                "Failed to create audio socket at '{}': {}",
                socket_path.display(),
                e
            )
        })?;

        let exe_path = std::env::current_exe()?;
        let mut cmd = Command::new(&exe_path);
        cmd.arg("audio-worker")
            .arg("--socket")
            .arg(socket_path.to_str().unwrap_or(""));
        if !paths.is_empty() {
            cmd.arg("--paths");
            for p in paths {
                cmd.arg(p);
            }
        }
        if safe_mode {
            cmd.arg("--safe-mode");
        }
        if malloc_debug {
            cmd.arg("--malloc-debug");
        }

        let child = cmd
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn audio worker: {}", e))?;

        let child_pid = child.id();
        info!(pid = child_pid, "Spawned audio worker process");

        // Accept connection with timeout
        listener.set_nonblocking(true).ok();
        let stream = {
            let start = std::time::Instant::now();
            let timeout = Duration::from_secs(15);
            loop {
                match listener.accept() {
                    Ok((stream, _)) => {
                        stream.set_nonblocking(false).ok();
                        stream
                            .set_read_timeout(Some(Duration::from_millis(50)))
                            .ok();
                        break stream;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        if start.elapsed() > timeout {
                            let _ = std::fs::remove_file(&socket_path);
                            return Err(anyhow::anyhow!(
                                "Audio worker did not connect within {}s",
                                timeout.as_secs()
                            ));
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    Err(e) => {
                        let _ = std::fs::remove_file(&socket_path);
                        return Err(anyhow::anyhow!("Audio worker accept failed: {}", e));
                    }
                }
            }
        };

        info!("Audio worker connected");

        Ok(Self {
            child,
            stream,
            socket_path,
        })
    }

    /// Get a cloned reader for the audio worker stream.
    fn reader(&self) -> std::os::unix::net::UnixStream {
        let r = self.stream.try_clone().expect("clone audio stream");
        r.set_read_timeout(Some(Duration::from_millis(50))).ok();
        r
    }
}

impl Drop for AudioWorkerContext {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

// ── Shadow state ────────────────────────────────────────────────────────

/// Shadow copy of state that the supervisor caches for crash recovery.
///
/// When the audio worker crashes, the supervisor can restore this state
/// in a newly spawned audio worker, preserving the user's rack config.
struct ShadowState {
    /// Available plugin modules.
    plugin_modules: Vec<PluginModuleInfo>,
    /// Rack slot configuration.
    rack: Vec<RackSlotState>,
    /// Currently selected slot.
    selected_slot: Option<usize>,
    /// Whether tone is enabled.
    tone_enabled: bool,
    /// Transport state.
    transport: TransportUpdate,
    /// Session file path.
    session_path: String,
    /// Whether safe mode is active.
    safe_mode: bool,
}

impl ShadowState {
    fn new(safe_mode: bool) -> Self {
        let session_path = crate::gui::session::sessions_dir()
            .map(|d| d.join("default.json").to_string_lossy().to_string())
            .unwrap_or_else(|| "session.json".into());

        Self {
            plugin_modules: Vec::new(),
            rack: Vec::new(),
            selected_slot: None,
            tone_enabled: false,
            transport: TransportUpdate {
                playing: false,
                tempo: 120.0,
                time_sig_num: 4,
                time_sig_den: 4,
            },
            session_path,
            safe_mode,
        }
    }

    /// Update shadow state from a `SupervisorUpdate` message.
    fn update_from(&mut self, update: &SupervisorUpdate) {
        match update {
            SupervisorUpdate::FullState {
                plugin_modules,
                rack,
                selected_slot,
                transport,
                tone_enabled,
                safe_mode,
                ..
            } => {
                self.plugin_modules = plugin_modules.clone();
                self.rack = rack.clone();
                self.selected_slot = *selected_slot;
                self.transport = transport.clone();
                self.tone_enabled = *tone_enabled;
                self.safe_mode = *safe_mode;
            }
            SupervisorUpdate::RackUpdated {
                rack,
                selected_slot,
                ..
            } => {
                self.rack = rack.clone();
                self.selected_slot = *selected_slot;
            }
            SupervisorUpdate::PluginModulesUpdated { modules } => {
                self.plugin_modules = modules.clone();
            }
            _ => {}
        }
    }

    /// Build a `RestoreState` command from cached shadow state.
    fn to_restore_command(&self) -> AudioCommand {
        AudioCommand::RestoreState {
            plugin_modules: self.plugin_modules.clone(),
            rack: self.rack.clone(),
            selected_slot: self.selected_slot,
            tone_enabled: self.tone_enabled,
            transport: self.transport.clone(),
            session_path: self.session_path.clone(),
        }
    }
}

// ── Loop result ─────────────────────────────────────────────────────────

/// Result of the message relay loop.
enum LoopResult {
    /// GUI shut down normally (window closed).
    CleanShutdown,
    /// GUI process crashed or disconnected.
    Crashed(String),
}

// ── Message relay loop ──────────────────────────────────────────────────

/// Run the message relay loop between the GUI and audio worker.
///
/// Relays messages bidirectionally:
/// - GUI → (GuiAction) → wrap as AudioCommand::Action → Audio Worker
/// - Audio Worker → (SupervisorUpdate) → GUI
///
/// Also monitors both child processes for crashes and handles recovery.
#[allow(clippy::too_many_arguments)]
fn run_relay_loop(
    gui_stream: &std::os::unix::net::UnixStream,
    mut gui_child: Child,
    audio_ctx: &mut AudioWorkerContext,
    shadow: &mut ShadowState,
    audio_restart_count: &mut u32,
    audio_first_restart_time: &mut std::time::Instant,
    safe_mode: bool,
    malloc_debug: bool,
    paths: &[std::path::PathBuf],
) -> LoopResult {
    let mut gui_reader = gui_stream
        .try_clone()
        .expect("clone GUI stream for reading");
    gui_reader
        .set_read_timeout(Some(Duration::from_millis(25)))
        .ok();
    let mut audio_reader = audio_ctx.reader();
    audio_reader
        .set_read_timeout(Some(Duration::from_millis(25)))
        .ok();

    loop {
        // 1. Try to read a GUI action and forward to audio worker
        match decode::<GuiAction>(&mut gui_reader) {
            Ok(Some(action)) => {
                let is_shutdown = matches!(action, GuiAction::Shutdown);
                let cmd = AudioCommand::Action(action);
                if let Err(e) = send_audio_command(&audio_ctx.stream, &cmd) {
                    warn!(error = %e, "Failed to forward action to audio worker");
                    // Audio worker might have crashed — will be detected below
                }
                if is_shutdown {
                    // Wait for ShutdownAck from audio worker, then exit
                    // Give audio worker a moment to send ack
                    std::thread::sleep(Duration::from_millis(100));
                    return LoopResult::CleanShutdown;
                }
            }
            Ok(None) => {
                // EOF — GUI process closed the connection
                return check_gui_exit(&mut gui_child);
            }
            Err(e) if e.is_timeout() => {
                // Expected — polling
            }
            Err(e) => {
                debug!(error = %e, "GUI decode error");
                return check_gui_exit(&mut gui_child);
            }
        }

        // 2. Try to read updates from audio worker and forward to GUI
        match decode::<SupervisorUpdate>(&mut audio_reader) {
            Ok(Some(update)) => {
                // Cache state for crash recovery
                shadow.update_from(&update);

                // Forward to GUI
                if let Err(e) = send_gui_update(gui_stream, &update) {
                    debug!(error = %e, "Failed to forward update to GUI");
                    // GUI might have crashed — will be detected below
                }
            }
            Ok(None) => {
                // EOF — Audio worker disconnected (crashed?)
                warn!("Audio worker disconnected");
                match try_restart_audio_worker(
                    audio_ctx,
                    shadow,
                    gui_stream,
                    audio_restart_count,
                    audio_first_restart_time,
                    safe_mode,
                    malloc_debug,
                    paths,
                ) {
                    Ok(new_reader) => {
                        audio_reader = new_reader;
                        continue;
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to restart audio worker — giving up");
                        // Notify GUI and clean exit
                        let _ = send_gui_update(
                            gui_stream,
                            &SupervisorUpdate::StatusMessage {
                                message: format!(
                                    "✗ Audio process failed to restart: {}. Please restart the host.",
                                    e
                                ),
                            },
                        );
                        // Wait for GUI to close
                        let _ = gui_child.wait();
                        return LoopResult::CleanShutdown;
                    }
                }
            }
            Err(e) if e.is_timeout() => {
                // Expected — polling
            }
            Err(e) => {
                debug!(error = %e, "Audio worker decode error");
                // Check if audio worker died
                match audio_ctx.child.try_wait() {
                    Ok(Some(status)) if !status.success() => {
                        warn!(exit_status = %status, "Audio worker exited abnormally");
                        match try_restart_audio_worker(
                            audio_ctx,
                            shadow,
                            gui_stream,
                            audio_restart_count,
                            audio_first_restart_time,
                            safe_mode,
                            malloc_debug,
                            paths,
                        ) {
                            Ok(new_reader) => {
                                audio_reader = new_reader;
                                continue;
                            }
                            Err(e) => {
                                error!(error = %e, "Failed to restart audio worker");
                                let _ = send_gui_update(
                                    gui_stream,
                                    &SupervisorUpdate::StatusMessage {
                                        message: format!(
                                            "✗ Audio process crashed and could not be restarted: {}",
                                            e
                                        ),
                                    },
                                );
                                let _ = gui_child.wait();
                                return LoopResult::CleanShutdown;
                            }
                        }
                    }
                    _ => {
                        // Audio worker still running — might be a transient error
                    }
                }
            }
        }

        // 3. Check if GUI is still running
        match gui_child.try_wait() {
            Ok(Some(exit_status)) => {
                if exit_status.success() {
                    return LoopResult::CleanShutdown;
                } else {
                    return LoopResult::Crashed(format!("GUI exited with {}", exit_status));
                }
            }
            Ok(None) => {
                // Still running
            }
            Err(e) => {
                return LoopResult::Crashed(format!("GUI wait error: {}", e));
            }
        }

        // 4. Check if audio worker is still running
        match audio_ctx.child.try_wait() {
            Ok(Some(status)) if !status.success() => {
                warn!(exit_status = %status, "Audio worker exited abnormally");
                match try_restart_audio_worker(
                    audio_ctx,
                    shadow,
                    gui_stream,
                    audio_restart_count,
                    audio_first_restart_time,
                    safe_mode,
                    malloc_debug,
                    paths,
                ) {
                    Ok(new_reader) => {
                        audio_reader = new_reader;
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to restart audio worker");
                        let _ = send_gui_update(
                            gui_stream,
                            &SupervisorUpdate::StatusMessage {
                                message: format!(
                                    "✗ Audio process crashed and could not be restarted: {}",
                                    e
                                ),
                            },
                        );
                    }
                }
            }
            _ => {
                // Still running or exited cleanly
            }
        }
    }
}

/// Try to restart the audio worker after a crash.
///
/// Returns the new audio reader stream on success.
#[allow(clippy::too_many_arguments)]
fn try_restart_audio_worker(
    audio_ctx: &mut AudioWorkerContext,
    shadow: &ShadowState,
    gui_stream: &std::os::unix::net::UnixStream,
    restart_count: &mut u32,
    first_restart_time: &mut std::time::Instant,
    safe_mode: bool,
    malloc_debug: bool,
    paths: &[std::path::PathBuf],
) -> anyhow::Result<std::os::unix::net::UnixStream> {
    // Check rapid restart limit
    *restart_count += 1;
    if *restart_count == 1 {
        *first_restart_time = std::time::Instant::now();
    }
    if first_restart_time.elapsed() > RAPID_RESTART_WINDOW {
        *restart_count = 1;
        *first_restart_time = std::time::Instant::now();
    }
    if *restart_count > MAX_RAPID_RESTARTS {
        return Err(anyhow::anyhow!(
            "Audio worker crashed {} times within {}s",
            restart_count,
            RAPID_RESTART_WINDOW.as_secs()
        ));
    }

    info!(
        restarts = *restart_count,
        "Restarting audio worker after crash"
    );

    // Clean up old audio worker
    let _ = audio_ctx.child.kill();
    let _ = audio_ctx.child.wait();

    // Brief pause before restart
    std::thread::sleep(Duration::from_millis(500));

    // Spawn new audio worker
    let new_ctx = AudioWorkerContext::spawn(safe_mode, malloc_debug, paths)?;
    let new_reader = new_ctx.reader();

    // Replace the context
    *audio_ctx = new_ctx;

    // Restore cached state in the new audio worker
    if let Err(e) = send_audio_command(&audio_ctx.stream, &shadow.to_restore_command()) {
        warn!(error = %e, "Failed to send restore state to new audio worker");
    }

    // Wait for restored state response (read up to a few from audio worker)
    let mut temp_reader = audio_ctx.reader();
    temp_reader
        .set_read_timeout(Some(Duration::from_secs(5)))
        .ok();
    if let Ok(Some(restored_state)) = decode::<SupervisorUpdate>(&mut temp_reader) {
        // Forward the restored state to GUI
        if let Err(e) = send_gui_update(gui_stream, &restored_state) {
            debug!(error = %e, "Failed to forward restored state to GUI");
        }
    }

    // Notify GUI that audio was restarted
    let restart_msg = SupervisorUpdate::AudioProcessRestarted {
        message: format!(
            "⚠ Audio process crashed and was restarted (attempt {}). \
             Plugins need to be re-activated.",
            restart_count
        ),
        restart_count: *restart_count,
    };
    if let Err(e) = send_gui_update(gui_stream, &restart_msg) {
        debug!(error = %e, "Failed to send restart notification to GUI");
    }

    Ok(new_reader)
}

// ── Helper functions ────────────────────────────────────────────────────

/// Check the GUI child process exit status.
///
/// When the user closes the window, the eframe event loop ends and the
/// process begins shutting down. The socket may close (causing the
/// supervisor to detect EOF) *before* the process has fully exited.
/// We therefore give the child a short grace period to exit cleanly
/// before declaring it crashed.
fn check_gui_exit(child: &mut Child) -> LoopResult {
    // Give the child a brief grace period to finish exiting.
    // The socket can close before the process exits when the user
    // closes the window normally.
    for _ in 0..20 {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                return LoopResult::CleanShutdown;
            }
            Ok(Some(status)) => {
                return LoopResult::Crashed(format!("GUI exited with {}", status));
            }
            Ok(None) => {
                // Still running — wait a bit
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                return LoopResult::Crashed(format!("wait error: {}", e));
            }
        }
    }
    // Child still hasn't exited after ~1s — treat as crashed
    let _ = child.kill();
    let _ = child.wait();
    LoopResult::Crashed("GUI socket closed unexpectedly and process did not exit".into())
}

/// Send an `AudioCommand` to the audio worker.
fn send_audio_command(
    stream: &std::os::unix::net::UnixStream,
    cmd: &AudioCommand,
) -> Result<(), String> {
    let data = encode(cmd)?;
    let mut writer = stream;
    writer
        .write_all(&data)
        .map_err(|e| format!("Write failed: {}", e))?;
    writer.flush().map_err(|e| format!("Flush failed: {}", e))?;
    Ok(())
}

/// Send a `SupervisorUpdate` to the GUI process.
fn send_gui_update(
    stream: &std::os::unix::net::UnixStream,
    update: &SupervisorUpdate,
) -> Result<(), String> {
    let data = encode(update)?;
    let mut writer = stream;
    writer
        .write_all(&data)
        .map_err(|e| format!("Write failed: {}", e))?;
    writer.flush().map_err(|e| format!("Flush failed: {}", e))?;
    Ok(())
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shadow_state_new() {
        let shadow = ShadowState::new(true);
        assert!(shadow.plugin_modules.is_empty());
        assert!(shadow.rack.is_empty());
        assert_eq!(shadow.selected_slot, None);
        assert!(!shadow.tone_enabled);
        assert!(shadow.safe_mode);
    }

    #[test]
    fn test_shadow_state_update_from_full_state() {
        let mut shadow = ShadowState::new(false);
        let update = SupervisorUpdate::FullState {
            plugin_modules: vec![PluginModuleInfo {
                path: std::path::PathBuf::from("/test.vst3"),
                factory_vendor: Some("TestVendor".into()),
                factory_url: None,
                factory_email: None,
                classes: vec![],
            }],
            rack: vec![RackSlotState {
                name: "TestPlugin".into(),
                vendor: "V".into(),
                category: "C".into(),
                path: std::path::PathBuf::from("/test.vst3"),
                cid: [0u8; 16],
                bypassed: false,
                param_cache: Vec::new(),
                staged_changes: Vec::new(),
            }],
            selected_slot: Some(0),
            active_slot: Some(0),
            param_snapshots: Vec::new(),
            audio_status: AudioStatusState::default(),
            audio_devices: Vec::new(),
            midi_ports: Vec::new(),
            selected_audio_device: None,
            selected_midi_port: None,
            process_isolation: false,
            status_message: "test".into(),
            heap_corruption_detected: false,
            has_editor: false,
            tainted_count: 0,
            transport: TransportUpdate {
                playing: true,
                tempo: 140.0,
                time_sig_num: 3,
                time_sig_den: 4,
            },
            tone_enabled: true,
            safe_mode: true,
        };
        shadow.update_from(&update);
        assert_eq!(shadow.plugin_modules.len(), 1);
        assert_eq!(shadow.rack.len(), 1);
        assert_eq!(shadow.selected_slot, Some(0));
        assert!(shadow.tone_enabled);
        assert!(shadow.transport.playing);
        assert_eq!(shadow.transport.tempo, 140.0);
        assert!(shadow.safe_mode);
    }

    #[test]
    fn test_shadow_state_update_from_rack_updated() {
        let mut shadow = ShadowState::new(false);
        let update = SupervisorUpdate::RackUpdated {
            rack: vec![RackSlotState {
                name: "P1".into(),
                vendor: "V".into(),
                category: "C".into(),
                path: std::path::PathBuf::from("/p1.vst3"),
                cid: [0u8; 16],
                bypassed: false,
                param_cache: Vec::new(),
                staged_changes: Vec::new(),
            }],
            active_slot: None,
            selected_slot: Some(0),
        };
        shadow.update_from(&update);
        assert_eq!(shadow.rack.len(), 1);
        assert_eq!(shadow.selected_slot, Some(0));
    }

    #[test]
    fn test_shadow_state_update_from_modules_updated() {
        let mut shadow = ShadowState::new(false);
        let update = SupervisorUpdate::PluginModulesUpdated {
            modules: vec![PluginModuleInfo {
                path: std::path::PathBuf::from("/test.vst3"),
                factory_vendor: None,
                factory_url: None,
                factory_email: None,
                classes: vec![],
            }],
        };
        shadow.update_from(&update);
        assert_eq!(shadow.plugin_modules.len(), 1);
    }

    #[test]
    fn test_shadow_state_to_restore_command() {
        let shadow = ShadowState {
            plugin_modules: vec![PluginModuleInfo {
                path: std::path::PathBuf::from("/test.vst3"),
                factory_vendor: None,
                factory_url: None,
                factory_email: None,
                classes: vec![],
            }],
            rack: vec![RackSlotState {
                name: "P1".into(),
                vendor: "V".into(),
                category: "C".into(),
                path: std::path::PathBuf::from("/p1.vst3"),
                cid: [0u8; 16],
                bypassed: false,
                param_cache: Vec::new(),
                staged_changes: Vec::new(),
            }],
            selected_slot: Some(0),
            tone_enabled: true,
            transport: TransportUpdate {
                playing: true,
                tempo: 130.0,
                time_sig_num: 3,
                time_sig_den: 8,
            },
            session_path: "test.json".into(),
            safe_mode: false,
        };
        let cmd = shadow.to_restore_command();
        match cmd {
            AudioCommand::RestoreState {
                plugin_modules,
                rack,
                selected_slot,
                tone_enabled,
                transport,
                session_path,
            } => {
                assert_eq!(plugin_modules.len(), 1);
                assert_eq!(rack.len(), 1);
                assert_eq!(selected_slot, Some(0));
                assert!(tone_enabled);
                assert!(transport.playing);
                assert_eq!(transport.tempo, 130.0);
                assert_eq!(session_path, "test.json");
            }
            _ => panic!("Expected RestoreState"),
        }
    }

    #[test]
    fn test_shadow_state_ignores_other_updates() {
        let mut shadow = ShadowState::new(false);
        shadow.update_from(&SupervisorUpdate::Pong);
        shadow.update_from(&SupervisorUpdate::ShutdownAck);
        shadow.update_from(&SupervisorUpdate::HeapCorruptionDetected);
        shadow.update_from(&SupervisorUpdate::StatusMessage {
            message: "test".into(),
        });
        assert!(shadow.plugin_modules.is_empty());
        assert!(shadow.rack.is_empty());
    }

    #[test]
    fn test_loop_result_variants() {
        let clean = LoopResult::CleanShutdown;
        let crashed = LoopResult::Crashed("test".into());
        match clean {
            LoopResult::CleanShutdown => {}
            _ => panic!("Expected CleanShutdown"),
        }
        match crashed {
            LoopResult::Crashed(msg) => assert_eq!(msg, "test"),
            _ => panic!("Expected Crashed"),
        }
    }

    #[test]
    fn test_audio_command_encode_decode() {
        let cmd = AudioCommand::Action(GuiAction::Ping);
        let encoded = encode(&cmd).expect("encode");
        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: Option<AudioCommand> = decode(&mut cursor).expect("decode");
        assert!(decoded.is_some());
    }

    #[test]
    fn test_audio_command_restore_state_roundtrip() {
        let cmd = AudioCommand::RestoreState {
            plugin_modules: vec![],
            rack: vec![],
            selected_slot: Some(2),
            tone_enabled: true,
            transport: TransportUpdate {
                playing: true,
                tempo: 128.0,
                time_sig_num: 7,
                time_sig_den: 8,
            },
            session_path: "/home/test.json".into(),
        };
        let json = serde_json::to_string(&cmd).expect("serialize");
        let decoded: AudioCommand = serde_json::from_str(&json).expect("deserialize");
        let json2 = serde_json::to_string(&decoded).expect("re-serialize");
        assert_eq!(json, json2);
    }

    #[test]
    fn test_audio_process_restarted_roundtrip() {
        let update = SupervisorUpdate::AudioProcessRestarted {
            message: "Audio crashed".into(),
            restart_count: 2,
        };
        let json = serde_json::to_string(&update).expect("serialize");
        let decoded: SupervisorUpdate = serde_json::from_str(&json).expect("deserialize");
        let json2 = serde_json::to_string(&decoded).expect("re-serialize");
        assert_eq!(json, json2);
    }

    #[test]
    fn test_check_gui_exit_clean_shutdown() {
        // Spawn a child that exits successfully
        let mut child = std::process::Command::new("true")
            .spawn()
            .expect("spawn true");
        // Wait for it to finish
        let _ = child.wait();
        let result = check_gui_exit(&mut child);
        match result {
            LoopResult::CleanShutdown => {}
            LoopResult::Crashed(msg) => panic!("Expected CleanShutdown, got Crashed: {}", msg),
        }
    }

    #[test]
    fn test_check_gui_exit_nonzero_exit() {
        // Spawn a child that exits with failure
        let mut child = std::process::Command::new("false")
            .spawn()
            .expect("spawn false");
        // Wait for it to finish
        let _ = child.wait();
        let result = check_gui_exit(&mut child);
        match result {
            LoopResult::Crashed(msg) => {
                assert!(
                    msg.contains("GUI exited with"),
                    "Expected 'GUI exited with' in: {}",
                    msg
                );
            }
            LoopResult::CleanShutdown => panic!("Expected Crashed, got CleanShutdown"),
        }
    }

    #[test]
    fn test_check_gui_exit_waits_for_clean_exit() {
        // Spawn a child that sleeps briefly then exits cleanly.
        // This tests that check_gui_exit waits instead of immediately
        // declaring a crash when the process hasn't exited yet.
        let mut child = std::process::Command::new("sleep")
            .arg("0.1")
            .spawn()
            .expect("spawn sleep");
        let result = check_gui_exit(&mut child);
        match result {
            LoopResult::CleanShutdown => {}
            LoopResult::Crashed(msg) => panic!("Expected CleanShutdown, got Crashed: {}", msg),
        }
    }
}
