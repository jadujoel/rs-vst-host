//! Plugin worker process — child process entry point for process-per-plugin sandboxing.
//!
//! Each plugin runs in its own process. The worker:
//! 1. Opens a Unix domain socket connection to the host
//! 2. Opens the shared memory audio buffer
//! 3. Loads the VST3 plugin
//! 4. Processes audio blocks on request
//! 5. Forwards parameter changes and plugin state
//!
//! The worker process is spawned by [`super::proxy::PluginProcess`] and
//! communicates exclusively via IPC — no shared address space with the host.

use crate::ipc::messages::*;
use crate::ipc::shm::ShmAudioBuffer;
use crate::vst3::com::{
    IEventList, IParameterChanges, ProcessContext as VstProcessContext, make_note_off_event,
    make_note_on_event,
};
use crate::vst3::component_handler::HostComponentHandler;
use crate::vst3::event_list::HostEventList;
use crate::vst3::instance::Vst3Instance;
use crate::vst3::module::Vst3Module;
use crate::vst3::param_changes::HostParameterChanges;
use crate::vst3::params::ParameterRegistry;
use crate::vst3::process::ProcessBuffers;
use crate::vst3::process_context::ProcessContext;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::Path;
use tracing::{debug, info};

/// Run the plugin worker process.
///
/// This is the main entry point for the child process. It connects to the
/// host via the given Unix socket path, then enters a message loop handling
/// host commands until shutdown.
///
/// # Arguments
/// * `socket_path` — Path to the Unix domain socket for IPC with the host.
pub fn run_worker(socket_path: &str) -> Result<(), String> {
    info!(socket = %socket_path, "Plugin worker starting");

    let mut stream = UnixStream::connect(socket_path)
        .map_err(|e| format!("Failed to connect to host socket: {}", e))?;

    info!("Connected to host process");

    let mut state = WorkerState::new();

    loop {
        let msg: Option<HostMessage> =
            decode_message(&mut stream).map_err(|e| format!("Failed to read message: {}", e))?;

        let msg = match msg {
            Some(m) => m,
            None => {
                info!("Host closed connection — shutting down worker");
                break;
            }
        };

        let response = state.handle_message(msg);

        // Check for any plugin-initiated parameter changes and queue them
        // (we'll send them along with the response or after it)

        let bytes =
            encode_message(&response).map_err(|e| format!("Failed to encode response: {}", e))?;
        stream
            .write_all(&bytes)
            .map_err(|e| format!("Failed to write response: {}", e))?;
        stream
            .flush()
            .map_err(|e| format!("Failed to flush: {}", e))?;

        if matches!(response, WorkerResponse::ShutdownAck) {
            info!("Shutdown acknowledged — exiting worker");
            break;
        }
    }

    Ok(())
}

/// Internal state of the plugin worker process.
struct WorkerState {
    /// Loaded VST3 module (library).
    module: Option<Vst3Module>,
    /// VST3 instance (component + processor).
    instance: Option<Vst3Instance>,
    /// Pre-allocated process buffers.
    buffers: Option<ProcessBuffers>,
    /// Host event list for MIDI events.
    event_list: *mut HostEventList,
    /// Parameter changes queue.
    param_changes: *mut HostParameterChanges,
    /// Process context (transport state).
    process_context: Option<ProcessContext>,
    /// Shared memory audio buffer.
    shm: Option<ShmAudioBuffer>,
    /// Parameter registry.
    params: Option<ParameterRegistry>,
    /// Component handler for plugin callbacks.
    component_handler: *mut HostComponentHandler,
    /// Whether the plugin has been activated.
    activated: bool,
    /// Configuration state.
    config: Option<WorkerConfig>,
}

/// Audio configuration received from the host.
#[allow(dead_code)]
struct WorkerConfig {
    sample_rate: f64,
    max_block_size: i32,
    output_channels: u32,
    input_arrangement: u64,
    output_arrangement: u64,
}

// Safety: Worker runs single-threaded for plugin interaction.
unsafe impl Send for WorkerState {}

impl WorkerState {
    fn new() -> Self {
        Self {
            module: None,
            instance: None,
            buffers: None,
            event_list: HostEventList::new(),
            param_changes: HostParameterChanges::new(),
            process_context: None,
            shm: None,
            params: None,
            component_handler: std::ptr::null_mut(),
            activated: false,
            config: None,
        }
    }

    /// Handle a single message from the host.
    fn handle_message(&mut self, msg: HostMessage) -> WorkerResponse {
        match msg {
            HostMessage::LoadPlugin { path, cid, name } => self.load_plugin(&path, &cid, &name),
            HostMessage::Configure {
                sample_rate,
                max_block_size,
                output_channels,
                input_arrangement,
                output_arrangement,
            } => self.configure(
                sample_rate,
                max_block_size,
                output_channels,
                input_arrangement,
                output_arrangement,
            ),
            HostMessage::Activate => self.activate(),
            HostMessage::Deactivate => self.deactivate(),
            HostMessage::Process {
                num_samples,
                events,
                param_changes,
                transport,
            } => self.process(num_samples, &events, &param_changes, &transport),
            HostMessage::SetParameter { id, value } => self.set_parameter(id, value),
            HostMessage::QueryParameters => self.query_parameters(),
            HostMessage::GetState => self.get_state(),
            HostMessage::SetState { data } => self.set_state(&data),
            HostMessage::HasEditor => self.has_editor(),
            HostMessage::Shutdown => self.shutdown(),
            HostMessage::Ping => WorkerResponse::Pong,
        }
    }

    fn load_plugin(&mut self, path: &str, cid: &[u8; 16], name: &str) -> WorkerResponse {
        info!(plugin = %name, path = %path, "Loading plugin in worker process");

        let module = match Vst3Module::load(Path::new(path)) {
            Ok(m) => m,
            Err(e) => {
                return WorkerResponse::Error {
                    message: format!("Failed to load module: {}", e),
                };
            }
        };

        let instance = match module.create_instance(cid, name) {
            Ok(i) => i,
            Err(e) => {
                return WorkerResponse::Error {
                    message: format!("Failed to create instance: {}", e),
                };
            }
        };

        let input_channels = instance.input_channels;
        let output_channels = instance.output_channels;
        let mut instance = instance;
        let has_editor = instance.has_editor();
        let plugin_name = instance.name.clone();

        self.instance = Some(instance);
        self.module = Some(module);

        WorkerResponse::PluginLoaded {
            name: plugin_name,
            input_channels,
            output_channels,
            has_editor,
        }
    }

    fn configure(
        &mut self,
        sample_rate: f64,
        max_block_size: i32,
        output_channels: u32,
        input_arrangement: u64,
        output_arrangement: u64,
    ) -> WorkerResponse {
        let instance = match self.instance.as_mut() {
            Some(i) => i,
            None => {
                return WorkerResponse::Error {
                    message: "No plugin loaded".into(),
                };
            }
        };

        // Verify 32-bit float support
        if !instance.can_process_f32() {
            return WorkerResponse::Error {
                message: "Plugin does not support 32-bit float processing".into(),
            };
        }

        // Set bus arrangements
        if let Err(e) = instance.set_bus_arrangements(input_arrangement, output_arrangement) {
            return WorkerResponse::Error {
                message: format!("Bus arrangement setup failed: {}", e),
            };
        }

        // Setup processing
        if let Err(e) = instance.setup_processing(sample_rate, max_block_size) {
            return WorkerResponse::Error {
                message: format!("Processing setup failed: {}", e),
            };
        }

        // Install component handler
        instance.install_component_handler();
        self.component_handler = instance.component_handler();

        // Create process buffers
        let input_ch = instance.input_channels;
        let output_ch = instance.output_channels;
        self.buffers = Some(ProcessBuffers::new(
            input_ch,
            output_ch,
            max_block_size as usize,
        ));

        // Create process context
        let mut ctx = ProcessContext::new(sample_rate);
        ctx.set_playing(true);
        self.process_context = Some(ctx);

        // Open shared memory
        let shm_name = format!("/rs-vst-host-{}", std::process::id());
        match ShmAudioBuffer::create(&shm_name, input_ch, output_ch, max_block_size as usize) {
            Ok(shm) => {
                self.shm = Some(shm);
            }
            Err(e) => {
                return WorkerResponse::Error {
                    message: format!("Failed to create shared memory: {}", e),
                };
            }
        }

        self.config = Some(WorkerConfig {
            sample_rate,
            max_block_size,
            output_channels,
            input_arrangement,
            output_arrangement,
        });

        debug!(
            sample_rate,
            max_block_size,
            input_channels = input_ch,
            output_channels = output_ch,
            "Worker configured"
        );

        WorkerResponse::Configured
    }

    fn activate(&mut self) -> WorkerResponse {
        let instance = match self.instance.as_mut() {
            Some(i) => i,
            None => {
                return WorkerResponse::Error {
                    message: "No plugin loaded".into(),
                };
            }
        };

        if let Err(e) = instance.activate() {
            return WorkerResponse::Error {
                message: format!("Activation failed: {}", e),
            };
        }

        if let Err(e) = instance.start_processing() {
            return WorkerResponse::Error {
                message: format!("Start processing failed: {}", e),
            };
        }

        // Query parameters after activation
        self.params = instance.query_parameters();

        self.activated = true;
        info!("Plugin activated in worker");
        WorkerResponse::Activated
    }

    fn deactivate(&mut self) -> WorkerResponse {
        if let Some(ref mut instance) = self.instance {
            instance.shutdown();
        }
        self.activated = false;
        info!("Plugin deactivated in worker");
        WorkerResponse::Deactivated
    }

    fn process(
        &mut self,
        num_samples: i32,
        events: &[MidiEvent],
        param_changes_in: &[ParamChange],
        transport: &TransportState,
    ) -> WorkerResponse {
        let instance = match self.instance.as_mut() {
            Some(i) => i,
            None => {
                return WorkerResponse::Error {
                    message: "No plugin loaded".into(),
                };
            }
        };

        if instance.is_crashed() {
            return WorkerResponse::Crashed {
                signal: "unknown".into(),
                context: "already crashed".into(),
                backtrace: vec![],
            };
        }

        let buffers = match self.buffers.as_mut() {
            Some(b) => b,
            None => {
                return WorkerResponse::Error {
                    message: "Not configured".into(),
                };
            }
        };

        let shm = match self.shm.as_ref() {
            Some(s) => s,
            None => {
                return WorkerResponse::Error {
                    message: "No shared memory".into(),
                };
            }
        };

        let num_samples = num_samples.min(buffers.max_block_size() as i32);

        // Prepare buffers
        buffers.prepare(num_samples as usize);

        // Copy input from shared memory into process buffers
        for ch in 0..buffers.num_input_channels() {
            if let (Some(input_buf), Some(shm_buf)) = (buffers.input_buffer_mut(ch), unsafe {
                shm.input_channel(ch)
            }) {
                let n = num_samples as usize;
                input_buf[..n].copy_from_slice(&shm_buf[..n]);
            }
        }

        // Set up MIDI events
        unsafe {
            HostEventList::clear(self.event_list);
            for evt in events {
                let vst3_event = match &evt.event_type {
                    MidiEventType::NoteOn { pitch, velocity } => {
                        make_note_on_event(evt.sample_offset, evt.channel, *pitch, *velocity, -1)
                    }
                    MidiEventType::NoteOff { pitch, velocity } => {
                        make_note_off_event(evt.sample_offset, evt.channel, *pitch, *velocity, -1)
                    }
                };
                HostEventList::add(self.event_list, vst3_event);
            }
            buffers.set_input_events(HostEventList::as_ptr(self.event_list) as *mut IEventList);
        }

        // Set up parameter changes
        unsafe {
            HostParameterChanges::clear(self.param_changes);
            for pc in param_changes_in {
                HostParameterChanges::add_change(
                    self.param_changes,
                    pc.id,
                    pc.sample_offset,
                    pc.value,
                );
            }
            buffers.set_input_parameter_changes(
                HostParameterChanges::as_ptr(self.param_changes) as *mut IParameterChanges
            );
        }

        // Set process context
        if let Some(ref mut ctx) = self.process_context {
            ctx.set_playing(transport.playing);
            ctx.set_tempo(transport.tempo);
            ctx.set_time_signature(transport.time_sig_numerator, transport.time_sig_denominator);
            buffers.set_process_context(ctx.as_ptr() as *mut VstProcessContext);
        }

        // Call VST3 process
        let success = unsafe {
            let data = buffers.process_data_ptr();
            instance.process(data)
        };

        // Advance transport
        if let Some(ref mut ctx) = self.process_context {
            ctx.advance(num_samples);
        }

        // Clear pointers
        buffers.set_input_events(std::ptr::null_mut());
        buffers.set_input_parameter_changes(std::ptr::null_mut());
        buffers.set_process_context(std::ptr::null_mut());

        if !success {
            return WorkerResponse::Crashed {
                signal: "unknown".into(),
                context: "process".into(),
                backtrace: vec![],
            };
        }

        // Copy output from process buffers into shared memory
        for ch in 0..buffers.num_output_channels() {
            if let Some(shm_buf) = unsafe { shm.output_channel_mut(ch) } {
                let n = num_samples as usize;
                if let Some(output_buf) = buffers.output_buffer(ch) {
                    shm_buf[..n].copy_from_slice(&output_buf[..n]);
                }
            }
        }

        shm.set_num_samples(num_samples as u32);
        shm.set_ready();

        WorkerResponse::Processed
    }

    fn set_parameter(&mut self, id: u32, value: f64) -> WorkerResponse {
        if let Some(ref mut params) = self.params {
            match params.set_normalized(id, value) {
                Ok(actual_value) => WorkerResponse::ParameterSet {
                    value: actual_value,
                },
                Err(e) => WorkerResponse::Error { message: e },
            }
        } else {
            WorkerResponse::Error {
                message: "No parameter registry".into(),
            }
        }
    }

    fn query_parameters(&self) -> WorkerResponse {
        let params = match self.params.as_ref() {
            Some(p) => p,
            None => {
                return WorkerResponse::Parameters { params: vec![] };
            }
        };

        let param_list: Vec<ParamInfo> = params
            .parameters
            .iter()
            .map(|p| ParamInfo {
                id: p.id,
                title: p.title.clone(),
                short_title: p.short_title.clone(),
                units: p.units.clone(),
                step_count: p.step_count,
                default_normalized: p.default_normalized,
                current_normalized: p.current_normalized,
                can_automate: p.can_automate,
                is_read_only: p.is_read_only,
                is_bypass: p.is_bypass,
            })
            .collect();

        WorkerResponse::Parameters { params: param_list }
    }

    fn get_state(&self) -> WorkerResponse {
        match &self.instance {
            Some(instance) => {
                let component_state = instance.get_component_state();
                WorkerResponse::State {
                    data: component_state,
                }
            }
            None => WorkerResponse::State { data: vec![] },
        }
    }

    fn set_state(&mut self, data: &[u8]) -> WorkerResponse {
        match &mut self.instance {
            Some(instance) => {
                if instance.set_component_state(data) {
                    WorkerResponse::StateLoaded
                } else {
                    WorkerResponse::Error {
                        message: "Failed to restore component state".to_string(),
                    }
                }
            }
            None => WorkerResponse::Error {
                message: "No plugin instance loaded".to_string(),
            },
        }
    }

    fn has_editor(&mut self) -> WorkerResponse {
        let has = self
            .instance
            .as_mut()
            .map(|i| i.has_editor())
            .unwrap_or(false);
        WorkerResponse::EditorAvailable { has_editor: has }
    }

    fn shutdown(&mut self) -> WorkerResponse {
        info!("Worker shutting down");

        // Deactivate if active
        if self.activated {
            if let Some(ref mut instance) = self.instance {
                instance.shutdown();
            }
            self.activated = false;
        }

        // Drop params before instance
        self.params.take();

        // Drop instance (releases COM references)
        self.instance.take();

        // Drop module (unloads library)
        self.module.take();

        // Drop shared memory
        self.shm.take();

        WorkerResponse::ShutdownAck
    }
}

impl Drop for WorkerState {
    fn drop(&mut self) {
        unsafe {
            HostEventList::destroy(self.event_list);
            HostParameterChanges::destroy(self.param_changes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worker_state_creation() {
        let state = WorkerState::new();
        assert!(state.module.is_none());
        assert!(state.instance.is_none());
        assert!(state.buffers.is_none());
        assert!(state.shm.is_none());
        assert!(state.params.is_none());
        assert!(!state.activated);
        assert!(state.config.is_none());
    }

    #[test]
    fn test_worker_ping_pong() {
        let mut state = WorkerState::new();
        let response = state.handle_message(HostMessage::Ping);
        assert!(matches!(response, WorkerResponse::Pong));
    }

    #[test]
    fn test_worker_shutdown_without_plugin() {
        let mut state = WorkerState::new();
        let response = state.handle_message(HostMessage::Shutdown);
        assert!(matches!(response, WorkerResponse::ShutdownAck));
    }

    #[test]
    fn test_worker_activate_without_plugin() {
        let mut state = WorkerState::new();
        let response = state.handle_message(HostMessage::Activate);
        match response {
            WorkerResponse::Error { message } => {
                assert!(message.contains("No plugin loaded"));
            }
            _ => panic!("Expected error for activate without plugin"),
        }
    }

    #[test]
    fn test_worker_configure_without_plugin() {
        let mut state = WorkerState::new();
        let response = state.handle_message(HostMessage::Configure {
            sample_rate: 44100.0,
            max_block_size: 1024,
            output_channels: 2,
            input_arrangement: 0x03,
            output_arrangement: 0x03,
        });
        match response {
            WorkerResponse::Error { message } => {
                assert!(message.contains("No plugin loaded"));
            }
            _ => panic!("Expected error for configure without plugin"),
        }
    }

    #[test]
    fn test_worker_process_without_plugin() {
        let mut state = WorkerState::new();
        let response = state.handle_message(HostMessage::Process {
            num_samples: 512,
            events: vec![],
            param_changes: vec![],
            transport: TransportState::default(),
        });
        match response {
            WorkerResponse::Error { message } => {
                assert!(message.contains("No plugin loaded"));
            }
            _ => panic!("Expected error for process without plugin"),
        }
    }

    #[test]
    fn test_worker_query_params_without_plugin() {
        let state = WorkerState::new();
        let response = state.query_parameters();
        match response {
            WorkerResponse::Parameters { params } => {
                assert!(params.is_empty());
            }
            _ => panic!("Expected empty params"),
        }
    }

    #[test]
    fn test_worker_has_editor_without_plugin() {
        let mut state = WorkerState::new();
        let response = state.has_editor();
        match response {
            WorkerResponse::EditorAvailable { has_editor } => {
                assert!(!has_editor);
            }
            _ => panic!("Expected EditorAvailable"),
        }
    }

    #[test]
    fn test_worker_set_parameter_without_registry() {
        let mut state = WorkerState::new();
        let response = state.set_parameter(1, 0.5);
        match response {
            WorkerResponse::Error { message } => {
                assert!(message.contains("No parameter registry"));
            }
            _ => panic!("Expected error"),
        }
    }

    #[test]
    fn test_worker_get_state() {
        let state = WorkerState::new();
        let response = state.get_state();
        match response {
            WorkerResponse::State { data } => {
                assert!(data.is_empty());
            }
            _ => panic!("Expected State"),
        }
    }

    #[test]
    fn test_worker_set_state() {
        let mut state = WorkerState::new();
        // Without a loaded plugin, set_state should return Error
        let response = state.set_state(&[1, 2, 3]);
        assert!(matches!(response, WorkerResponse::Error { .. }));
    }

    #[test]
    fn test_worker_deactivate_without_plugin() {
        let mut state = WorkerState::new();
        let response = state.deactivate();
        assert!(matches!(response, WorkerResponse::Deactivated));
        assert!(!state.activated);
    }

    #[test]
    fn test_worker_load_nonexistent_plugin() {
        let mut state = WorkerState::new();
        let response = state.load_plugin("/nonexistent/path/Test.vst3", &[0; 16], "NonExistent");
        match response {
            WorkerResponse::Error { message } => {
                assert!(
                    message.contains("Failed to load module"),
                    "Got: {}",
                    message
                );
            }
            _ => panic!("Expected Error, got {:?}", response),
        }
    }
}
