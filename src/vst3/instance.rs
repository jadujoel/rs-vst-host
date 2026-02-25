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
use crate::vst3::host_context::HostApplication;
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

            Ok(Self {
                component,
                processor,
                host_context,
                active: false,
                processing: false,
                input_channels,
                output_channels,
                name: name.to_string(),
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

            let result =
                (proc_vtbl.setup_processing)(self.processor as *mut c_void, &mut setup);

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
            // Terminate the component
            let comp_vtbl = &*(*self.component).vtbl;
            (comp_vtbl.terminate)(self.component as *mut c_void);
            debug!(plugin = %self.name, "Component terminated");

            // Release COM references
            let proc_vtbl = &*(*self.processor).vtbl;
            (proc_vtbl.release)(self.processor as *mut c_void);
            (comp_vtbl.release)(self.component as *mut c_void);

            // Destroy host context
            HostApplication::destroy(self.host_context);

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
    fn test_process_setup_constants() {
        assert_eq!(K_SAMPLE_32, 0);
        assert_eq!(K_REALTIME, 0);
        assert_eq!(K_AUDIO, 0);
        assert_eq!(K_INPUT, 0);
        assert_eq!(K_OUTPUT, 1);
    }
}
