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
use std::ffi::c_void;
use tracing::{debug, info, warn};

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
            })
        }
    }

    /// Verify the plugin supports 32-bit float processing.
    pub fn can_process_f32(&self) -> bool {
        unsafe {
            let proc_vtbl = &*(*self.processor).vtbl;
            let result =
                (proc_vtbl.can_process_sample_size)(self.processor as *mut c_void, K_SAMPLE_32);
            result == K_RESULT_OK
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
        unsafe {
            let proc_vtbl = &*(*self.processor).vtbl;
            let mut inputs = [input_arr];
            let mut outputs = [output_arr];

            let num_ins = if self.input_channels > 0 { 1 } else { 0 };
            let num_outs = 1i32;

            let result = (proc_vtbl.set_bus_arrangements)(
                self.processor as *mut c_void,
                inputs.as_mut_ptr(),
                num_ins,
                outputs.as_mut_ptr(),
                num_outs,
            );

            if result != K_RESULT_OK {
                warn!(
                    plugin = %self.name,
                    result,
                    "setBusArrangements returned non-OK (may still work)"
                );
                // Many plugins return kResultFalse but still work with defaults
            }

            // Activate the audio buses
            let comp_vtbl = &*(*self.component).vtbl;
            if self.input_channels > 0 {
                (comp_vtbl.activate_bus)(self.component as *mut c_void, K_AUDIO, K_INPUT, 0, 1);
            }
            (comp_vtbl.activate_bus)(self.component as *mut c_void, K_AUDIO, K_OUTPUT, 0, 1);

            debug!(plugin = %self.name, "Bus arrangements configured");
            Ok(())
        }
    }

    /// Configure the processing setup (sample rate, block size, etc.).
    pub fn setup_processing(
        &mut self,
        sample_rate: f64,
        max_block_size: i32,
    ) -> Result<(), Vst3Error> {
        unsafe {
            let proc_vtbl = &*(*self.processor).vtbl;
            let mut setup = ProcessSetup {
                process_mode: K_REALTIME,
                symbolic_sample_size: K_SAMPLE_32,
                max_samples_per_block: max_block_size,
                sample_rate,
            };

            let result = (proc_vtbl.setup_processing)(self.processor as *mut c_void, &mut setup);

            if result != K_RESULT_OK {
                return Err(Vst3Error::Factory(format!(
                    "setupProcessing failed for '{}' (result: {})",
                    self.name, result
                )));
            }

            info!(
                plugin = %self.name,
                sample_rate,
                max_block_size,
                "Processing setup complete"
            );
            Ok(())
        }
    }

    /// Activate the component for processing.
    pub fn activate(&mut self) -> Result<(), Vst3Error> {
        if self.active {
            return Ok(());
        }

        unsafe {
            let comp_vtbl = &*(*self.component).vtbl;
            let result = (comp_vtbl.set_active)(self.component as *mut c_void, 1);

            if result != K_RESULT_OK {
                return Err(Vst3Error::Factory(format!(
                    "setActive(true) failed for '{}' (result: {})",
                    self.name, result
                )));
            }

            self.active = true;
            debug!(plugin = %self.name, "Component activated");
        }
        Ok(())
    }

    /// Start processing.
    pub fn start_processing(&mut self) -> Result<(), Vst3Error> {
        if self.processing {
            return Ok(());
        }

        unsafe {
            let proc_vtbl = &*(*self.processor).vtbl;
            let result = (proc_vtbl.set_processing)(self.processor as *mut c_void, 1);

            if result != K_RESULT_OK {
                return Err(Vst3Error::Factory(format!(
                    "setProcessing(true) failed for '{}' (result: {})",
                    self.name, result
                )));
            }

            self.processing = true;
            info!(plugin = %self.name, "Processing started");
        }
        Ok(())
    }

    /// Call the plugin's process function with prepared buffers.
    ///
    /// # Safety
    /// The `data` must point to a valid, fully initialized `ProcessData` with
    /// stable buffer pointers for the duration of the call.
    pub unsafe fn process(&self, data: *mut ProcessData) -> i32 {
        unsafe {
            let proc_vtbl = &*(*self.processor).vtbl;
            (proc_vtbl.process)(self.processor as *mut c_void, data)
        }
    }

    /// Get the plugin's latency in samples.
    pub fn latency_samples(&self) -> u32 {
        unsafe {
            let proc_vtbl = &*(*self.processor).vtbl;
            (proc_vtbl.get_latency_samples)(self.processor as *mut c_void)
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

    /// Disconnect component and controller IConnectionPoint (best-effort, called on drop).
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
        let controller = match self.get_controller() {
            Some(c) => c,
            None => {
                debug!(plugin = %self.name, "No IEditController available for component handler");
                return false;
            }
        };

        unsafe {
            let ctrl_vtbl = &*(*controller).vtbl;

            // Create and install the handler
            let handler = HostComponentHandler::new();
            let result = (ctrl_vtbl.set_component_handler)(
                controller as *mut c_void,
                HostComponentHandler::as_ptr(handler),
            );

            if result == K_RESULT_OK {
                self.component_handler = handler;
                info!(plugin = %self.name, "IComponentHandler installed");
                true
            } else {
                HostComponentHandler::destroy(handler);
                warn!(plugin = %self.name, result, "setComponentHandler failed");
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

    /// Stop processing and deactivate the component.
    pub fn shutdown(&mut self) {
        unsafe {
            if self.processing {
                let proc_vtbl = &*(*self.processor).vtbl;
                (proc_vtbl.set_processing)(self.processor as *mut c_void, 0);
                self.processing = false;
                debug!(plugin = %self.name, "Processing stopped");
            }

            if self.active {
                let comp_vtbl = &*(*self.component).vtbl;
                (comp_vtbl.set_active)(self.component as *mut c_void, 0);
                self.active = false;
                debug!(plugin = %self.name, "Component deactivated");
            }
        }
    }
}

impl Drop for Vst3Instance {
    fn drop(&mut self) {
        // Ensure processing is stopped and component is deactivated
        self.shutdown();

        unsafe {
            // Disconnect IConnectionPoint before releasing the controller
            self.disconnect_component_controller();

            // Release the cached controller
            if self.owns_separate_controller && !self.cached_controller.is_null() {
                // Terminate and release the separately created controller
                let ctrl_vtbl = &*(*self.cached_controller).vtbl;
                (ctrl_vtbl.terminate)(self.cached_controller as *mut c_void);
                (ctrl_vtbl.release)(self.cached_controller as *mut c_void);
                debug!(plugin = %self.name, "Separate IEditController terminated and released");

                // Destroy controller's host context
                if !self.controller_host_context.is_null() {
                    HostApplication::destroy(self.controller_host_context);
                }
            } else if !self.cached_controller.is_null() {
                // Release QI'd controller reference (one Release for the QI AddRef)
                let ctrl_vtbl = &*(*self.cached_controller).vtbl;
                (ctrl_vtbl.release)(self.cached_controller as *mut c_void);
            }

            // Terminate the component
            let comp_vtbl = &*(*self.component).vtbl;
            (comp_vtbl.terminate)(self.component as *mut c_void);
            debug!(plugin = %self.name, "Component terminated");

            // Release COM references
            let proc_vtbl = &*(*self.processor).vtbl;
            (proc_vtbl.release)(self.processor as *mut c_void);
            (comp_vtbl.release)(self.component as *mut c_void);

            // Release factory reference (balances the AddRef in create())
            if !self.factory_vtbl.is_null() {
                let fvtbl = &*self.factory_vtbl;
                (fvtbl.base.release)(self.factory);
            }

            // Destroy host context
            HostApplication::destroy(self.host_context);

            // Destroy component handler
            if !self.component_handler.is_null() {
                HostComponentHandler::destroy(self.component_handler);
            }

            info!(plugin = %self.name, "VST3 instance destroyed");
        }
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
}
