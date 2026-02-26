//! VST3 plugin instance management: component creation, processor setup, and lifecycle.
//!
//! Handles the full VST3 component lifecycle:
//! 1. Factory creates IComponent via `createInstance(cid, IComponent::iid)`
//! 2. `IComponent::initialize(hostContext)`
//! 3. Query IComponent for IAudioProcessor
//! 4. Configure bus arrangements and processing setup
//! 5. Activate, process, deactivate, terminate

use crate::error::Vst3Error;
use crate::vst3::com::*;
use crate::vst3::component_handler::HostComponentHandler;
use crate::vst3::host_context::HostApplication;
use crate::vst3::module::IPluginFactoryVtbl;
use crate::vst3::params::ParameterRegistry;
use crate::vst3::sandbox::{SandboxResult, sandbox_call};
use std::ffi::c_void;
use tracing::{debug, error, info, warn};

/// A fully initialized VST3 plugin instance ready for audio processing.
///
/// Owns COM references to both IComponent and IAudioProcessor interfaces.
/// Manages the complete lifecycle from initialization through shutdown.
pub struct Vst3Instance {
    /// IComponent COM pointer.
    component: *mut ComPtr<IComponentVtbl>,
    /// IAudioProcessor COM pointer (queried from component).
    processor: *mut ComPtr<IAudioProcessorVtbl>,
    /// Host context (owned, destroyed on drop).
    host_context: *mut HostApplication,
    /// IComponentHandler (owned, destroyed on drop).
    component_handler: *mut HostComponentHandler,
    /// Whether the component is currently active.
    active: bool,
    /// Whether processing is currently enabled.
    processing: bool,
    /// Number of audio input channels configured.
    pub input_channels: usize,
    /// Number of audio output channels configured.
    pub output_channels: usize,
    /// Plugin name for logging.
    pub name: String,
    /// Factory COM pointer (AddRef'd for safe use during instance lifetime).
    factory: *mut c_void,
    /// Factory vtable pointer (valid as long as factory is alive).
    factory_vtbl: *const IPluginFactoryVtbl,
    /// Cached IEditController pointer (obtained via QI or separate creation).
    cached_controller: *mut ComPtr<IEditControllerVtbl>,
    /// Whether we own the separate controller (need to terminate + release on drop).
    owns_separate_controller: bool,
    /// Host context for the separate controller (destroyed on drop if non-null).
    controller_host_context: *mut HostApplication,
    /// Whether the plugin has crashed (signals all COM calls should be skipped).
    crashed: bool,
}

// Safety: COM pointers are accessed from the thread that creates the instance
// or from the audio thread after proper handoff. The Mutex in the engine
// ensures exclusive access.
unsafe impl Send for Vst3Instance {}

impl Vst3Instance {
    /// Create a new VST3 instance from a factory and class ID.
    ///
    /// This performs:
    /// 1. Factory `createInstance` to get IComponent
    /// 2. `IComponent::initialize` with host context
    /// 3. QueryInterface for IAudioProcessor
    pub unsafe fn create(
        factory: *mut c_void,
        factory_vtbl: &crate::vst3::module::IPluginFactoryVtbl,
        cid: &[u8; 16],
        name: &str,
    ) -> Result<Self, Vst3Error> {
        unsafe {
            // Create host context
            let host_context = HostApplication::new();

            // Create component instance
            let mut component_ptr: *mut c_void = std::ptr::null_mut();
            let result = (factory_vtbl.create_instance)(
                factory,
                cid.as_ptr(),
                ICOMPONENT_IID.as_ptr(),
                &mut component_ptr,
            );

            if result != K_RESULT_OK || component_ptr.is_null() {
                HostApplication::destroy(host_context);
                return Err(Vst3Error::Factory(format!(
                    "createInstance failed for '{}' (result: {})",
                    name, result
                )));
            }

            let component = component_ptr as *mut ComPtr<IComponentVtbl>;
            debug!(plugin = %name, "Created IComponent instance");

            // Initialize the component with host context
            let comp_vtbl = &*(*component).vtbl;
            let init_result =
                (comp_vtbl.initialize)(component_ptr, HostApplication::as_unknown(host_context));

            if init_result != K_RESULT_OK {
                (comp_vtbl.release)(component_ptr);
                HostApplication::destroy(host_context);
                return Err(Vst3Error::Factory(format!(
                    "IComponent::initialize failed for '{}' (result: {})",
                    name, init_result
                )));
            }

            debug!(plugin = %name, "Initialized IComponent");

            // Query for IAudioProcessor
            let mut processor_ptr: *mut c_void = std::ptr::null_mut();
            let qi_result = (comp_vtbl.query_interface)(
                component_ptr,
                IAUDIO_PROCESSOR_IID.as_ptr(),
                &mut processor_ptr,
            );

            if qi_result != K_RESULT_OK || processor_ptr.is_null() {
                (comp_vtbl.terminate)(component_ptr);
                (comp_vtbl.release)(component_ptr);
                HostApplication::destroy(host_context);
                return Err(Vst3Error::Factory(format!(
                    "QueryInterface for IAudioProcessor failed for '{}' (result: {})",
                    name, qi_result
                )));
            }

            let processor = processor_ptr as *mut ComPtr<IAudioProcessorVtbl>;
            debug!(plugin = %name, "Obtained IAudioProcessor interface");

            // Query bus configuration
            let input_bus_count = (comp_vtbl.get_bus_count)(component_ptr, K_AUDIO, K_INPUT);
            let output_bus_count = (comp_vtbl.get_bus_count)(component_ptr, K_AUDIO, K_OUTPUT);
            debug!(plugin = %name, input_buses = input_bus_count, output_buses = output_bus_count, "Bus counts");

            // Get channel counts from bus info
            let input_channels = if input_bus_count > 0 {
                let mut bus_info: BusInfo = std::mem::zeroed();
                if (comp_vtbl.get_bus_info)(component_ptr, K_AUDIO, K_INPUT, 0, &mut bus_info)
                    == K_RESULT_OK
                {
                    debug!(channels = bus_info.channel_count, "Input bus 0");
                    bus_info.channel_count.max(0) as usize
                } else {
                    2 // Default to stereo
                }
            } else {
                0
            };

            let output_channels = if output_bus_count > 0 {
                let mut bus_info: BusInfo = std::mem::zeroed();
                if (comp_vtbl.get_bus_info)(component_ptr, K_AUDIO, K_OUTPUT, 0, &mut bus_info)
                    == K_RESULT_OK
                {
                    debug!(channels = bus_info.channel_count, "Output bus 0");
                    bus_info.channel_count.max(0) as usize
                } else {
                    2
                }
            } else {
                2
            };

            info!(
                plugin = %name,
                input_channels,
                output_channels,
                "VST3 instance created"
            );

            // AddRef the factory so we can use it later for controller creation
            (factory_vtbl.base.add_ref)(factory);

            Ok(Self {
                component,
                processor,
                host_context,
                component_handler: std::ptr::null_mut(),
                active: false,
                processing: false,
                input_channels,
                output_channels,
                name: name.to_string(),
                factory,
                factory_vtbl: factory_vtbl as *const _,
                cached_controller: std::ptr::null_mut(),
                owns_separate_controller: false,
                controller_host_context: std::ptr::null_mut(),
                crashed: false,
            })
        }
    }

    /// Verify the plugin supports 32-bit float processing.
    pub fn can_process_f32(&self) -> bool {
        if self.crashed {
            return false;
        }
        let proc = self.processor as usize;
        let result = sandbox_call("can_process_f32", move || unsafe {
            let processor = proc as *mut ComPtr<IAudioProcessorVtbl>;
            let proc_vtbl = &*(*processor).vtbl;
            (proc_vtbl.can_process_sample_size)(processor as *mut c_void, K_SAMPLE_32)
        });
        match result {
            SandboxResult::Ok(K_RESULT_OK) => true,
            _ => false,
        }
    }

    /// Set bus arrangements (speaker configurations).
    ///
    /// Typically called before `setup_processing` with stereo in/out.
    pub fn set_bus_arrangements(
        &mut self,
        input_arr: u64,
        output_arr: u64,
    ) -> Result<(), Vst3Error> {
        if self.crashed {
            return Err(Vst3Error::Factory("Plugin has crashed".to_string()));
        }

        let proc = self.processor as usize;
        let comp = self.component as usize;
        let in_ch = self.input_channels;

        let result = sandbox_call("set_bus_arrangements", move || unsafe {
            let processor = proc as *mut ComPtr<IAudioProcessorVtbl>;
            let component = comp as *mut ComPtr<IComponentVtbl>;
            let proc_vtbl = &*(*processor).vtbl;
            let comp_vtbl = &*(*component).vtbl;

            let mut inputs = [input_arr];
            let mut outputs = [output_arr];
            let num_ins = if in_ch > 0 { 1 } else { 0 };
            let num_outs = 1i32;

            let _result = (proc_vtbl.set_bus_arrangements)(
                processor as *mut c_void,
                inputs.as_mut_ptr(),
                num_ins,
                outputs.as_mut_ptr(),
                num_outs,
            );
            // Many plugins return kResultFalse but still work with defaults

            // Activate the audio buses
            if in_ch > 0 {
                (comp_vtbl.activate_bus)(component as *mut c_void, K_AUDIO, K_INPUT, 0, 1);
            }
            (comp_vtbl.activate_bus)(component as *mut c_void, K_AUDIO, K_OUTPUT, 0, 1);
        });

        match result {
            SandboxResult::Ok(()) => {
                debug!(plugin = %self.name, "Bus arrangements configured");
                Ok(())
            }
            SandboxResult::Crashed(crash) => {
                self.crashed = true;
                Err(Vst3Error::Factory(format!(
                    "Plugin '{}' crashed during bus arrangement ({})",
                    self.name, crash.signal_name
                )))
            }
            SandboxResult::Panicked(msg) => {
                self.crashed = true;
                Err(Vst3Error::Factory(format!(
                    "Plugin '{}' panicked during bus arrangement: {}",
                    self.name, msg
                )))
            }
        }
    }

    /// Configure the processing setup (sample rate, block size, etc.).
    pub fn setup_processing(
        &mut self,
        sample_rate: f64,
        max_block_size: i32,
    ) -> Result<(), Vst3Error> {
        if self.crashed {
            return Err(Vst3Error::Factory("Plugin has crashed".to_string()));
        }

        let proc = self.processor as usize;
        let result = sandbox_call("setup_processing", move || unsafe {
            let processor = proc as *mut ComPtr<IAudioProcessorVtbl>;
            let proc_vtbl = &*(*processor).vtbl;
            let mut setup = ProcessSetup {
                process_mode: K_REALTIME,
                symbolic_sample_size: K_SAMPLE_32,
                max_samples_per_block: max_block_size,
                sample_rate,
            };
            (proc_vtbl.setup_processing)(processor as *mut c_void, &mut setup)
        });

        match result {
            SandboxResult::Ok(K_RESULT_OK) => {
                info!(
                    plugin = %self.name,
                    sample_rate,
                    max_block_size,
                    "Processing setup complete"
                );
                Ok(())
            }
            SandboxResult::Ok(r) => Err(Vst3Error::Factory(format!(
                "setupProcessing failed for '{}' (result: {})",
                self.name, r
            ))),
            SandboxResult::Crashed(crash) => {
                self.crashed = true;
                Err(Vst3Error::Factory(format!(
                    "Plugin '{}' crashed during setupProcessing ({})",
                    self.name, crash.signal_name
                )))
            }
            SandboxResult::Panicked(msg) => {
                self.crashed = true;
                Err(Vst3Error::Factory(format!(
                    "Plugin '{}' panicked during setupProcessing: {}",
                    self.name, msg
                )))
            }
        }
    }

    /// Activate the component for processing.
    pub fn activate(&mut self) -> Result<(), Vst3Error> {
        if self.active {
            return Ok(());
        }
        if self.crashed {
            return Err(Vst3Error::Factory("Plugin has crashed".to_string()));
        }

        let comp = self.component as usize;
        let result = sandbox_call("activate", move || unsafe {
            let component = comp as *mut ComPtr<IComponentVtbl>;
            let comp_vtbl = &*(*component).vtbl;
            (comp_vtbl.set_active)(component as *mut c_void, 1)
        });

        match result {
            SandboxResult::Ok(K_RESULT_OK) => {
                self.active = true;
                debug!(plugin = %self.name, "Component activated");
                Ok(())
            }
            SandboxResult::Ok(r) => Err(Vst3Error::Factory(format!(
                "setActive(true) failed for '{}' (result: {})",
                self.name, r
            ))),
            SandboxResult::Crashed(crash) => {
                self.crashed = true;
                Err(Vst3Error::Factory(format!(
                    "Plugin '{}' crashed during activation ({})",
                    self.name, crash.signal_name
                )))
            }
            SandboxResult::Panicked(msg) => {
                self.crashed = true;
                Err(Vst3Error::Factory(format!(
                    "Plugin '{}' panicked during activation: {}",
                    self.name, msg
                )))
            }
        }
    }

    /// Start processing.
    pub fn start_processing(&mut self) -> Result<(), Vst3Error> {
        if self.processing {
            return Ok(());
        }
        if self.crashed {
            return Err(Vst3Error::Factory("Plugin has crashed".to_string()));
        }

        let proc = self.processor as usize;
        let result = sandbox_call("start_processing", move || unsafe {
            let processor = proc as *mut ComPtr<IAudioProcessorVtbl>;
            let proc_vtbl = &*(*processor).vtbl;
            (proc_vtbl.set_processing)(processor as *mut c_void, 1)
        });

        match result {
            SandboxResult::Ok(K_RESULT_OK) => {
                self.processing = true;
                info!(plugin = %self.name, "Processing started");
                Ok(())
            }
            SandboxResult::Ok(r) => Err(Vst3Error::Factory(format!(
                "setProcessing(true) failed for '{}' (result: {})",
                self.name, r
            ))),
            SandboxResult::Crashed(crash) => {
                self.crashed = true;
                Err(Vst3Error::Factory(format!(
                    "Plugin '{}' crashed during start_processing ({})",
                    self.name, crash.signal_name
                )))
            }
            SandboxResult::Panicked(msg) => {
                self.crashed = true;
                Err(Vst3Error::Factory(format!(
                    "Plugin '{}' panicked during start_processing: {}",
                    self.name, msg
                )))
            }
        }
    }

    /// Call the plugin's process function with crash protection.
    ///
    /// Returns `true` if processing succeeded. Returns `false` if the plugin
    /// crashed (the instance is then marked as crashed and all subsequent
    /// COM calls will be skipped).
    ///
    /// # Safety
    /// The `data` must point to a valid, fully initialized `ProcessData` with
    /// stable buffer pointers for the duration of the call.
    pub unsafe fn process(&mut self, data: *mut ProcessData) -> bool {
        if self.crashed {
            return false;
        }

        let proc = self.processor;
        let result = sandbox_call("audio_process", move || unsafe {
            let proc_vtbl = &*(*proc).vtbl;
            (proc_vtbl.process)(proc as *mut c_void, data)
        });

        match result {
            SandboxResult::Ok(_) => true,
            SandboxResult::Crashed(crash) => {
                self.crashed = true;
                error!(
                    plugin = %self.name,
                    signal = %crash.signal_name,
                    "Plugin crashed during audio processing — instance marked as crashed"
                );
                false
            }
            SandboxResult::Panicked(msg) => {
                self.crashed = true;
                error!(
                    plugin = %self.name,
                    panic = %msg,
                    "Plugin panicked during audio processing — instance marked as crashed"
                );
                false
            }
        }
    }

    /// Whether this plugin instance has crashed and should not be used.
    pub fn is_crashed(&self) -> bool {
        self.crashed
    }

    /// Get the plugin's latency in samples.
    pub fn latency_samples(&self) -> u32 {
        if self.crashed {
            return 0;
        }
        let proc = self.processor as usize;
        let result = sandbox_call("get_latency_samples", move || unsafe {
            let processor = proc as *mut ComPtr<IAudioProcessorVtbl>;
            let proc_vtbl = &*(*processor).vtbl;
            (proc_vtbl.get_latency_samples)(processor as *mut c_void)
        });
        match result {
            SandboxResult::Ok(v) => v,
            _ => 0,
        }
    }

    /// Get or create the IEditController for this plugin.
    ///
    /// Tries in order:
    /// 1. Return cached controller if already obtained
    /// 2. QueryInterface on the component (single-component plugins)
    /// 3. Create a separate controller via the factory (split component/controller plugins)
    ///
    /// The returned pointer is cached and owned by the instance.
    fn get_controller(&mut self) -> Option<*mut ComPtr<IEditControllerVtbl>> {
        if !self.cached_controller.is_null() {
            return Some(self.cached_controller);
        }

        unsafe {
            let comp_vtbl = &*(*self.component).vtbl;

            // Try 1: QueryInterface for IEditController directly on the component
            let mut controller_ptr: *mut c_void = std::ptr::null_mut();
            let qi_result = (comp_vtbl.query_interface)(
                self.component as *mut c_void,
                IEDIT_CONTROLLER_IID.as_ptr(),
                &mut controller_ptr,
            );

            if qi_result == K_RESULT_OK && !controller_ptr.is_null() {
                debug!(plugin = %self.name, "IEditController obtained via QueryInterface");
                self.cached_controller = controller_ptr as *mut ComPtr<IEditControllerVtbl>;
                self.owns_separate_controller = false;
                return Some(self.cached_controller);
            }

            // Try 2: Get controller class ID and create a separate controller
            let mut controller_cid = [0u8; 16];
            let result = (comp_vtbl.get_controller_class_id)(
                self.component as *mut c_void,
                &mut controller_cid,
            );

            if result != K_RESULT_OK || controller_cid == [0u8; 16] {
                debug!(plugin = %self.name, "No controller class ID available");
                return None;
            }

            debug!(
                plugin = %self.name,
                controller_cid = ?controller_cid,
                "Creating separate IEditController via factory"
            );

            // Create the controller using the factory's createInstance
            let factory_vtbl = &*self.factory_vtbl;
            let mut ec_ptr: *mut c_void = std::ptr::null_mut();
            let create_result = (factory_vtbl.create_instance)(
                self.factory,
                controller_cid.as_ptr(),
                IEDIT_CONTROLLER_IID.as_ptr(),
                &mut ec_ptr,
            );

            if create_result != K_RESULT_OK || ec_ptr.is_null() {
                warn!(
                    plugin = %self.name,
                    result = create_result,
                    "Factory createInstance failed for separate IEditController"
                );
                return None;
            }

            let controller = ec_ptr as *mut ComPtr<IEditControllerVtbl>;

            // Initialize the controller with a host context
            let host_ctx = HostApplication::new();
            let ctrl_vtbl = &*(*controller).vtbl;
            let init_result = (ctrl_vtbl.initialize)(ec_ptr, HostApplication::as_unknown(host_ctx));

            if init_result != K_RESULT_OK {
                warn!(
                    plugin = %self.name,
                    result = init_result,
                    "Separate IEditController::initialize failed"
                );
                (ctrl_vtbl.release)(ec_ptr);
                HostApplication::destroy(host_ctx);
                return None;
            }

            // Connect component ↔ controller via IConnectionPoint (best-effort)
            self.connect_component_controller(controller);

            self.cached_controller = controller;
            self.owns_separate_controller = true;
            self.controller_host_context = host_ctx;

            info!(
                plugin = %self.name,
                "Separate IEditController created and initialized"
            );

            Some(self.cached_controller)
        }
    }

    /// Connect component and controller via IConnectionPoint (if both support it).
    ///
    /// This enables bidirectional communication between the component (processor)
    /// and the controller (parameter/UI side) in split-architecture plugins.
    fn connect_component_controller(&self, controller: *mut ComPtr<IEditControllerVtbl>) {
        unsafe {
            let comp_vtbl = &*(*self.component).vtbl;
            let ctrl_vtbl = &*(*controller).vtbl;

            // Query IConnectionPoint on the component
            let mut comp_cp: *mut c_void = std::ptr::null_mut();
            let qi1 = (comp_vtbl.query_interface)(
                self.component as *mut c_void,
                ICONNECTION_POINT_IID.as_ptr(),
                &mut comp_cp,
            );

            if qi1 != K_RESULT_OK || comp_cp.is_null() {
                debug!(plugin = %self.name, "Component does not support IConnectionPoint");
                return;
            }

            // Query IConnectionPoint on the controller
            let mut ctrl_cp: *mut c_void = std::ptr::null_mut();
            let qi2 = (ctrl_vtbl.query_interface)(
                controller as *mut c_void,
                ICONNECTION_POINT_IID.as_ptr(),
                &mut ctrl_cp,
            );

            if qi2 != K_RESULT_OK || ctrl_cp.is_null() {
                debug!(plugin = %self.name, "Controller does not support IConnectionPoint");
                let cp_vtbl = &*(*(comp_cp as *mut ComPtr<IConnectionPointVtbl>)).vtbl;
                (cp_vtbl.release)(comp_cp);
                return;
            }

            // Connect both directions
            let comp_cp_vtbl = &*(*(comp_cp as *mut ComPtr<IConnectionPointVtbl>)).vtbl;
            let ctrl_cp_vtbl = &*(*(ctrl_cp as *mut ComPtr<IConnectionPointVtbl>)).vtbl;

            let r1 = (comp_cp_vtbl.connect)(comp_cp, ctrl_cp);
            let r2 = (ctrl_cp_vtbl.connect)(ctrl_cp, comp_cp);

            if r1 == K_RESULT_OK && r2 == K_RESULT_OK {
                debug!(plugin = %self.name, "Component ↔ Controller connected via IConnectionPoint");
            } else {
                debug!(
                    plugin = %self.name,
                    comp_result = r1,
                    ctrl_result = r2,
                    "IConnectionPoint::connect partial or failed"
                );
            }

            // Release QI'd IConnectionPoint references
            (comp_cp_vtbl.release)(comp_cp);
            (ctrl_cp_vtbl.release)(ctrl_cp);
        }
    }

    /// Disconnect component and controller IConnectionPoint (best-effort).
    ///
    /// Note: The Drop impl inlines this logic inside a sandbox_call.
    /// This method is kept for potential use in non-drop code paths.
    #[allow(dead_code)]
    fn disconnect_component_controller(&self) {
        if !self.owns_separate_controller || self.cached_controller.is_null() {
            return;
        }

        unsafe {
            let comp_vtbl = &*(*self.component).vtbl;
            let ctrl_vtbl = &*(*self.cached_controller).vtbl;

            let mut comp_cp: *mut c_void = std::ptr::null_mut();
            let qi1 = (comp_vtbl.query_interface)(
                self.component as *mut c_void,
                ICONNECTION_POINT_IID.as_ptr(),
                &mut comp_cp,
            );

            let mut ctrl_cp: *mut c_void = std::ptr::null_mut();
            let qi2 = (ctrl_vtbl.query_interface)(
                self.cached_controller as *mut c_void,
                ICONNECTION_POINT_IID.as_ptr(),
                &mut ctrl_cp,
            );

            if qi1 == K_RESULT_OK && !comp_cp.is_null() && qi2 == K_RESULT_OK && !ctrl_cp.is_null()
            {
                let comp_cp_vtbl = &*(*(comp_cp as *mut ComPtr<IConnectionPointVtbl>)).vtbl;
                let ctrl_cp_vtbl = &*(*(ctrl_cp as *mut ComPtr<IConnectionPointVtbl>)).vtbl;

                (comp_cp_vtbl.disconnect)(comp_cp, ctrl_cp);
                (ctrl_cp_vtbl.disconnect)(ctrl_cp, comp_cp);

                (comp_cp_vtbl.release)(comp_cp);
                (ctrl_cp_vtbl.release)(ctrl_cp);

                debug!(plugin = %self.name, "Component ↔ Controller disconnected");
            } else {
                // Release any QI'd refs that succeeded
                if qi1 == K_RESULT_OK && !comp_cp.is_null() {
                    let vtbl = &*(*(comp_cp as *mut ComPtr<IConnectionPointVtbl>)).vtbl;
                    (vtbl.release)(comp_cp);
                }
                if qi2 == K_RESULT_OK && !ctrl_cp.is_null() {
                    let vtbl = &*(*(ctrl_cp as *mut ComPtr<IConnectionPointVtbl>)).vtbl;
                    (vtbl.release)(ctrl_cp);
                }
            }
        }
    }

    /// Query the component for an IEditController interface.
    ///
    /// Returns a `ParameterRegistry` with all enumerated parameters, or None
    /// if the component does not support IEditController.
    pub fn query_parameters(&mut self) -> Option<ParameterRegistry> {
        let controller = self.get_controller()?;
        // The instance owns the controller — ParameterRegistry borrows it
        unsafe { Some(ParameterRegistry::from_controller(controller, false)) }
    }

    /// Install a component handler on the IEditController.
    ///
    /// Creates a `HostComponentHandler`, calls `setComponentHandler()` on the controller,
    /// and stores the handler for later polling.
    pub fn install_component_handler(&mut self) -> bool {
        if self.crashed {
            return false;
        }

        let controller = match self.get_controller() {
            Some(c) => c,
            None => {
                debug!(plugin = %self.name, "No IEditController available for component handler");
                return false;
            }
        };

        let handler = HostComponentHandler::new();
        let ctrl = controller as usize;
        // Safety: HostPlugFrame::as_ptr returns the raw COM pointer.
        let handler_ptr = unsafe { HostComponentHandler::as_ptr(handler) };
        let result = sandbox_call("set_component_handler", move || unsafe {
            let controller = ctrl as *mut ComPtr<IEditControllerVtbl>;
            let ctrl_vtbl = &*(*controller).vtbl;
            (ctrl_vtbl.set_component_handler)(controller as *mut c_void, handler_ptr)
        });

        match result {
            SandboxResult::Ok(K_RESULT_OK) => {
                self.component_handler = handler;
                info!(plugin = %self.name, "IComponentHandler installed");
                true
            }
            SandboxResult::Ok(r) => {
                // Safety: handler is our own Rust struct, never crashes.
                unsafe { HostComponentHandler::destroy(handler) };
                warn!(plugin = %self.name, result = r, "setComponentHandler failed");
                false
            }
            SandboxResult::Crashed(crash) => {
                // Handler was never accepted by the plugin, destroy it
                // Safety: handler is our own Rust struct, never crashes.
                unsafe { HostComponentHandler::destroy(handler) };
                self.crashed = true;
                warn!(
                    plugin = %self.name,
                    signal = %crash.signal_name,
                    "Plugin crashed during setComponentHandler"
                );
                false
            }
            SandboxResult::Panicked(msg) => {
                // Safety: handler is our own Rust struct, never crashes.
                unsafe { HostComponentHandler::destroy(handler) };
                self.crashed = true;
                warn!(
                    plugin = %self.name,
                    panic = %msg,
                    "Plugin panicked during setComponentHandler"
                );
                false
            }
        }
    }

    /// Get the component handler pointer (if installed).
    ///
    /// Used by the command layer to poll for parameter changes from the plugin.
    pub fn component_handler(&self) -> *mut HostComponentHandler {
        self.component_handler
    }

    /// Create a plugin editor view (IPlugView).
    ///
    /// Calls `IEditController::createView("editor")` and returns the IPlugView
    /// pointer if the plugin supports an editor UI. The caller is responsible
    /// for managing the view lifecycle (attached, removed, release).
    ///
    /// Returns `None` if the plugin has no editor, no controller, or crashes.
    pub fn create_editor_view(&mut self) -> Option<*mut ComPtr<IPlugViewVtbl>> {
        if self.crashed {
            return None;
        }

        let controller = self.get_controller()?;

        let ctrl = controller as usize;
        let result = sandbox_call("create_editor_view", move || unsafe {
            let controller = ctrl as *mut ComPtr<IEditControllerVtbl>;
            let ctrl_vtbl = &*(*controller).vtbl;

            // Call createView("editor")
            let view_name = b"editor\0";
            let view_ptr = (ctrl_vtbl.create_view)(controller as *mut c_void, view_name.as_ptr());

            view_ptr
        });

        match result {
            SandboxResult::Ok(view_ptr) => {
                if view_ptr.is_null() {
                    debug!(plugin = %self.name, "Plugin does not provide an editor view");
                    return None;
                }
                let view = view_ptr as *mut ComPtr<IPlugViewVtbl>;
                debug!(plugin = %self.name, "IPlugView created");
                Some(view)
            }
            SandboxResult::Crashed(crash) => {
                warn!(
                    plugin = %self.name,
                    signal = %crash.signal_name,
                    "Plugin crashed during createView"
                );
                self.crashed = true;
                None
            }
            SandboxResult::Panicked(msg) => {
                warn!(
                    plugin = %self.name,
                    panic = %msg,
                    "Plugin panicked during createView"
                );
                self.crashed = true;
                None
            }
        }
    }

    /// Check if the plugin provides an editor UI.
    ///
    /// Creates a temporary IPlugView and immediately releases it.
    pub fn has_editor(&mut self) -> bool {
        if self.crashed {
            return false;
        }

        if let Some(view) = self.create_editor_view() {
            let v = view as usize;
            let _ = sandbox_call("has_editor_release", move || unsafe {
                let view = v as *mut ComPtr<IPlugViewVtbl>;
                let vtbl = &*(*view).vtbl;
                (vtbl.release)(view as *mut c_void)
            });
            true
        } else {
            false
        }
    }

    /// Stop processing and deactivate the component (with crash protection).
    ///
    /// Each COM call is sandboxed so that a plugin crash during shutdown
    /// does not terminate the host. If a crash is detected, the instance
    /// is marked as crashed and remaining COM calls are skipped.
    pub fn shutdown(&mut self) {
        if self.crashed {
            debug!(plugin = %self.name, "Skipping shutdown for crashed plugin");
            return;
        }

        if self.processing {
            let proc = self.processor;
            let result = sandbox_call("set_processing_off", move || unsafe {
                let proc_vtbl = &*(*proc).vtbl;
                (proc_vtbl.set_processing)(proc as *mut c_void, 0)
            });
            match result {
                SandboxResult::Ok(_) => {
                    self.processing = false;
                    debug!(plugin = %self.name, "Processing stopped");
                }
                _ => {
                    self.crashed = true;
                    warn!(plugin = %self.name, "Plugin crashed during set_processing(0) — skipping remaining shutdown");
                    return;
                }
            }
        }

        if self.active {
            let comp = self.component;
            let result = sandbox_call("set_active_off", move || unsafe {
                let comp_vtbl = &*(*comp).vtbl;
                (comp_vtbl.set_active)(comp as *mut c_void, 0)
            });
            match result {
                SandboxResult::Ok(_) => {
                    self.active = false;
                    debug!(plugin = %self.name, "Component deactivated");
                }
                _ => {
                    self.crashed = true;
                    warn!(plugin = %self.name, "Plugin crashed during set_active(0) — skipping remaining shutdown");
                }
            }
        }
    }
}

impl Drop for Vst3Instance {
    fn drop(&mut self) {
        // Ensure processing is stopped and component is deactivated (sandboxed)
        self.shutdown();

        if !self.crashed {
            // Extract all raw pointers (Copy types) so the closure doesn't
            // borrow self — required for sandbox_call.
            let component = self.component;
            let processor = self.processor;
            let cached_controller = self.cached_controller;
            let owns_separate_controller = self.owns_separate_controller;
            let factory = self.factory;
            let factory_vtbl = self.factory_vtbl;

            // Wrap ALL plugin-facing COM cleanup in a single sandbox.
            // If any COM call crashes, we catch the signal and skip the
            // rest of the cleanup (intentionally leaking COM objects).
            let result = sandbox_call("instance_drop", move || unsafe {
                // 1. Disconnect IConnectionPoint between component and controller
                if owns_separate_controller && !cached_controller.is_null() {
                    let comp_vtbl = &*(*component).vtbl;
                    let ctrl_vtbl = &*(*cached_controller).vtbl;

                    let mut comp_cp: *mut c_void = std::ptr::null_mut();
                    let qi1 = (comp_vtbl.query_interface)(
                        component as *mut c_void,
                        ICONNECTION_POINT_IID.as_ptr(),
                        &mut comp_cp,
                    );

                    let mut ctrl_cp: *mut c_void = std::ptr::null_mut();
                    let qi2 = (ctrl_vtbl.query_interface)(
                        cached_controller as *mut c_void,
                        ICONNECTION_POINT_IID.as_ptr(),
                        &mut ctrl_cp,
                    );

                    if qi1 == K_RESULT_OK
                        && !comp_cp.is_null()
                        && qi2 == K_RESULT_OK
                        && !ctrl_cp.is_null()
                    {
                        let comp_cp_vtbl = &*(*(comp_cp as *mut ComPtr<IConnectionPointVtbl>)).vtbl;
                        let ctrl_cp_vtbl = &*(*(ctrl_cp as *mut ComPtr<IConnectionPointVtbl>)).vtbl;

                        (comp_cp_vtbl.disconnect)(comp_cp, ctrl_cp);
                        (ctrl_cp_vtbl.disconnect)(ctrl_cp, comp_cp);

                        (comp_cp_vtbl.release)(comp_cp);
                        (ctrl_cp_vtbl.release)(ctrl_cp);
                    } else {
                        if qi1 == K_RESULT_OK && !comp_cp.is_null() {
                            let vtbl = &*(*(comp_cp as *mut ComPtr<IConnectionPointVtbl>)).vtbl;
                            (vtbl.release)(comp_cp);
                        }
                        if qi2 == K_RESULT_OK && !ctrl_cp.is_null() {
                            let vtbl = &*(*(ctrl_cp as *mut ComPtr<IConnectionPointVtbl>)).vtbl;
                            (vtbl.release)(ctrl_cp);
                        }
                    }
                }

                // 2. Release the cached controller
                if owns_separate_controller && !cached_controller.is_null() {
                    let ctrl_vtbl = &*(*cached_controller).vtbl;
                    (ctrl_vtbl.terminate)(cached_controller as *mut c_void);
                    (ctrl_vtbl.release)(cached_controller as *mut c_void);
                } else if !cached_controller.is_null() {
                    let ctrl_vtbl = &*(*cached_controller).vtbl;
                    (ctrl_vtbl.release)(cached_controller as *mut c_void);
                }

                // 3. Terminate the component
                let comp_vtbl = &*(*component).vtbl;
                (comp_vtbl.terminate)(component as *mut c_void);

                // 4. Release COM references
                let proc_vtbl = &*(*processor).vtbl;
                (proc_vtbl.release)(processor as *mut c_void);
                (comp_vtbl.release)(component as *mut c_void);

                // 5. Release factory reference (balances AddRef in create())
                if !factory_vtbl.is_null() {
                    let fvtbl = &*factory_vtbl;
                    (fvtbl.base.release)(factory);
                }
            });

            match &result {
                SandboxResult::Crashed(crash) => {
                    warn!(
                        plugin = %self.name,
                        signal = %crash.signal_name,
                        "Plugin crashed during COM cleanup — resources leaked (host is safe)"
                    );
                }
                SandboxResult::Panicked(msg) => {
                    warn!(
                        plugin = %self.name,
                        panic = %msg,
                        "Plugin panicked during COM cleanup"
                    );
                }
                SandboxResult::Ok(()) => {
                    debug!(plugin = %self.name, "COM references released");
                }
            }
        } else {
            warn!(
                plugin = %self.name,
                "Skipping COM cleanup for crashed plugin — resources leaked intentionally"
            );
        }

        // Always clean up our own resources (these are pure Rust, never crash)
        unsafe {
            if !self.controller_host_context.is_null() {
                HostApplication::destroy(self.controller_host_context);
            }
            HostApplication::destroy(self.host_context);
            if !self.component_handler.is_null() {
                HostComponentHandler::destroy(self.component_handler);
            }
        }

        info!(plugin = %self.name, "VST3 instance destroyed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_icomponent_iid_is_16_bytes() {
        assert_eq!(ICOMPONENT_IID.len(), 16);
    }

    #[test]
    fn test_iaudio_processor_iid_is_16_bytes() {
        assert_eq!(IAUDIO_PROCESSOR_IID.len(), 16);
    }

    #[test]
    fn test_iedit_controller_iid_is_16_bytes() {
        assert_eq!(IEDIT_CONTROLLER_IID.len(), 16);
    }

    #[test]
    fn test_iconnection_point_iid_is_16_bytes() {
        assert_eq!(ICONNECTION_POINT_IID.len(), 16);
    }

    #[test]
    fn test_process_setup_constants() {
        assert_eq!(K_SAMPLE_32, 0);
        assert_eq!(K_REALTIME, 0);
        assert_eq!(K_AUDIO, 0);
        assert_eq!(K_INPUT, 0);
        assert_eq!(K_OUTPUT, 1);
    }

    #[test]
    fn test_iconnection_point_vtbl_has_correct_layout() {
        // IConnectionPointVtbl should have 5 function pointers:
        // queryInterface, addRef, release, connect, disconnect
        let size = std::mem::size_of::<IConnectionPointVtbl>();
        #[cfg(target_pointer_width = "64")]
        assert_eq!(size, 5 * 8, "IConnectionPointVtbl should be 5 pointers");
    }

    #[test]
    fn test_factory_vtbl_has_create_instance() {
        // IPluginFactoryVtbl should have base (3 fns) + 4 factory fns = 7 pointers
        let size = std::mem::size_of::<IPluginFactoryVtbl>();
        #[cfg(target_pointer_width = "64")]
        assert_eq!(size, 7 * 8, "IPluginFactoryVtbl should be 7 pointers");
    }

    #[test]
    fn test_sandbox_used_in_lifecycle_methods() {
        // Verify sandbox_call is importable and usable from the instance module
        use crate::vst3::sandbox::sandbox_call;

        let result = sandbox_call("instance_test", || 42);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_sandbox_crash_recovery_in_instance_context() {
        // Simulate the kind of crash that happens during plugin deactivation
        use crate::vst3::sandbox::{SandboxResult, sandbox_call};

        let result: SandboxResult<()> = sandbox_call("simulate_deactivation_crash", || unsafe {
            libc::raise(libc::SIGBUS);
        });
        assert!(result.is_crashed());

        // The host should be able to continue after the crash
        let normal = sandbox_call("post_crash_normal", || "recovered");
        assert!(normal.is_ok());
        assert_eq!(normal.unwrap(), "recovered");
    }

    #[test]
    fn test_sandbox_catches_abort_during_cleanup() {
        // Simulate malloc abort (like the report.txt crash) during cleanup
        use crate::vst3::sandbox::{SandboxResult, sandbox_call};

        let result: SandboxResult<()> = sandbox_call("simulate_abort_crash", || unsafe {
            libc::raise(libc::SIGABRT);
        });
        assert!(result.is_crashed());

        if let SandboxResult::Crashed(crash) = result {
            assert_eq!(crash.signal, libc::SIGABRT);
        }
    }
}
