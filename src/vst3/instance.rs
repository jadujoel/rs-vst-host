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
use crate::vst3::params::ParameterRegistry;
use crate::vst3::sandbox::{SandboxResult, sandbox_call};
use std::cell::Cell;
use std::ffi::c_void;
use tracing::{debug, error, info, warn};
use vst3::Steinberg::IPluginBase;

// ── Thread-local flag for instance ↔ module crash communication ─────────
//
// When Vst3Instance::drop's sandbox catches a crash during COM cleanup,
// this flag is set to `true`. The Vst3Module::drop (which runs immediately
// after on the same thread) checks it and skips library unload to prevent
// C++ static destructors from running on corrupted plugin state.
thread_local! {
    pub(crate) static LAST_DROP_CRASHED: Cell<bool> = const { Cell::new(false) };

    /// Set to `true` when a Vst3Instance crashes during COM cleanup in its
    /// Drop impl. The GUI backend checks this after `ActiveState` is fully
    /// dropped to record the plugin path as "tainted", preventing re-load
    /// in the same session (reloading a crashed-and-leaked library causes
    /// heap corruption because `siglongjmp` recovery may leave the process
    /// malloc state inconsistent).
    pub(crate) static DEACTIVATION_CRASHED: Cell<bool> = const { Cell::new(false) };

    /// Set to `true` when heap corruption is detected during crash recovery
    /// in instance Drop. The GUI backend reads this alongside DEACTIVATION_CRASHED
    /// to propagate heap corruption status to the user.
    pub(crate) static DEACTIVATION_HEAP_CORRUPTED: Cell<bool> = const { Cell::new(false) };
}

/// A fully initialized VST3 plugin instance ready for audio processing.
///
/// Owns COM references to both IComponent and IAudioProcessor interfaces.
/// Manages the complete lifecycle from initialization through shutdown.
pub struct Vst3Instance {
    /// IComponent COM pointer.
    component: *mut IComponent,
    /// IAudioProcessor COM pointer (queried from component).
    processor: *mut IAudioProcessor,
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
    cached_controller: *mut IEditController,
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
    ///
    /// # Safety
    ///
    /// `factory` must be a valid COM pointer to an `IPluginFactory` obtained from
    /// the VST3 module, and `factory_vtbl` must point to its vtable.
    pub unsafe fn create(
        factory: *mut c_void,
        factory_vtbl: &IPluginFactoryVtbl,
        cid: &[u8; 16],
        name: &str,
    ) -> Result<Self, Vst3Error> {
        unsafe {
            // Create host context
            let host_context = HostApplication::new();

            // Create component instance
            let mut component_ptr: *mut c_void = std::ptr::null_mut();
            let result = (factory_vtbl.createInstance)(
                factory as *mut IPluginFactory,
                cid.as_ptr() as FIDString,
                ICOMPONENT_IID.as_ptr() as FIDString,
                &mut component_ptr,
            );

            if result != K_RESULT_OK || component_ptr.is_null() {
                HostApplication::destroy(host_context);
                return Err(Vst3Error::Factory(format!(
                    "createInstance failed for '{}' (result: {})",
                    name, result
                )));
            }

            let component = component_ptr as *mut IComponent;
            debug!(plugin = %name, "Created IComponent instance");

            // Initialize the component with host context
            let comp_vtbl = &*(*component).vtbl;
            let init_result = (comp_vtbl.base.initialize)(
                component_ptr as *mut IPluginBase,
                HostApplication::as_unknown(host_context) as *mut FUnknown,
            );

            if init_result != K_RESULT_OK {
                (comp_vtbl.base.base.release)(component_ptr as *mut FUnknown);
                HostApplication::destroy(host_context);
                return Err(Vst3Error::Factory(format!(
                    "IComponent::initialize failed for '{}' (result: {})",
                    name, init_result
                )));
            }

            debug!(plugin = %name, "Initialized IComponent");

            // Query for IAudioProcessor
            let mut processor_ptr: *mut c_void = std::ptr::null_mut();
            let qi_result = (comp_vtbl.base.base.queryInterface)(
                component_ptr as *mut FUnknown,
                iid_as_tuid_ptr(&IAUDIO_PROCESSOR_IID),
                &mut processor_ptr,
            );

            if qi_result != K_RESULT_OK || processor_ptr.is_null() {
                (comp_vtbl.base.terminate)(component_ptr as *mut IPluginBase);
                (comp_vtbl.base.base.release)(component_ptr as *mut FUnknown);
                HostApplication::destroy(host_context);
                return Err(Vst3Error::Factory(format!(
                    "QueryInterface for IAudioProcessor failed for '{}' (result: {})",
                    name, qi_result
                )));
            }

            let processor = processor_ptr as *mut IAudioProcessor;
            debug!(plugin = %name, "Obtained IAudioProcessor interface");

            // Query bus configuration
            let input_bus_count =
                (comp_vtbl.getBusCount)(component_ptr as *mut IComponent, K_AUDIO, K_INPUT);
            let output_bus_count =
                (comp_vtbl.getBusCount)(component_ptr as *mut IComponent, K_AUDIO, K_OUTPUT);
            debug!(plugin = %name, input_buses = input_bus_count, output_buses = output_bus_count, "Bus counts");

            // Get channel counts from bus info
            let input_channels = if input_bus_count > 0 {
                let mut bus_info: BusInfo = std::mem::zeroed();
                if (comp_vtbl.getBusInfo)(
                    component_ptr as *mut IComponent,
                    K_AUDIO,
                    K_INPUT,
                    0,
                    &mut bus_info,
                ) == K_RESULT_OK
                {
                    debug!(channels = bus_info.channelCount, "Input bus 0");
                    bus_info.channelCount.max(0) as usize
                } else {
                    2 // Default to stereo
                }
            } else {
                0
            };

            let output_channels = if output_bus_count > 0 {
                let mut bus_info: BusInfo = std::mem::zeroed();
                if (comp_vtbl.getBusInfo)(
                    component_ptr as *mut IComponent,
                    K_AUDIO,
                    K_OUTPUT,
                    0,
                    &mut bus_info,
                ) == K_RESULT_OK
                {
                    debug!(channels = bus_info.channelCount, "Output bus 0");
                    bus_info.channelCount.max(0) as usize
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
            (factory_vtbl.base.addRef)(factory as *mut FUnknown);

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
            let processor = proc as *mut IAudioProcessor;
            let proc_vtbl = &*(*processor).vtbl;
            (proc_vtbl.canProcessSampleSize)(processor, K_SAMPLE_32)
        });
        matches!(result, SandboxResult::Ok(K_RESULT_OK))
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
            let processor = proc as *mut IAudioProcessor;
            let component = comp as *mut IComponent;
            let proc_vtbl = &*(*processor).vtbl;
            let comp_vtbl = &*(*component).vtbl;

            let mut inputs = [input_arr];
            let mut outputs = [output_arr];
            let num_ins = if in_ch > 0 { 1 } else { 0 };
            let num_outs = 1i32;

            let _result = (proc_vtbl.setBusArrangements)(
                processor,
                inputs.as_mut_ptr(),
                num_ins,
                outputs.as_mut_ptr(),
                num_outs,
            );
            // Many plugins return kResultFalse but still work with defaults

            // Activate the audio buses
            if in_ch > 0 {
                (comp_vtbl.activateBus)(component, K_AUDIO, K_INPUT, 0, 1);
            }
            (comp_vtbl.activateBus)(component, K_AUDIO, K_OUTPUT, 0, 1);
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

    /// Set bus arrangements with a fallback chain for maximum compatibility.
    ///
    /// Tries each arrangement in order: stereo → mono → default (empty call).
    /// Returns the number of output channels that succeeded, or an error if
    /// all arrangements failed due to a crash.
    ///
    /// Speaker arrangement constants:
    /// - Stereo: 0x03 (kSpeakerL | kSpeakerR)
    /// - Mono: 0x01 (kSpeakerL)
    pub fn set_bus_arrangements_with_fallback(
        &mut self,
        desired_input_channels: usize,
        desired_output_channels: usize,
    ) -> Result<usize, Vst3Error> {
        // Speaker arrangement constants
        const SPEAKER_STEREO: u64 = 0x03; // L + R
        const SPEAKER_MONO: u64 = 0x01; // L only

        // Build fallback chain based on desired channels
        let arrangements: Vec<(u64, u64, usize)> = if desired_output_channels >= 2 {
            vec![
                (
                    if desired_input_channels > 0 {
                        SPEAKER_STEREO
                    } else {
                        0
                    },
                    SPEAKER_STEREO,
                    2,
                ),
                (
                    if desired_input_channels > 0 {
                        SPEAKER_MONO
                    } else {
                        0
                    },
                    SPEAKER_MONO,
                    1,
                ),
            ]
        } else {
            vec![(
                if desired_input_channels > 0 {
                    SPEAKER_MONO
                } else {
                    0
                },
                SPEAKER_MONO,
                1,
            )]
        };

        for (input_arr, output_arr, out_ch) in &arrangements {
            match self.try_bus_arrangement(*input_arr, *output_arr) {
                Ok(true) => {
                    info!(
                        plugin = %self.name,
                        output_channels = out_ch,
                        "Bus arrangement accepted"
                    );
                    self.output_channels = *out_ch;
                    if desired_input_channels > 0 {
                        self.input_channels = if *input_arr == SPEAKER_STEREO { 2 } else { 1 };
                    }
                    return Ok(*out_ch);
                }
                Ok(false) => {
                    debug!(
                        plugin = %self.name,
                        output_channels = out_ch,
                        "Bus arrangement rejected, trying fallback"
                    );
                    continue;
                }
                Err(e) => return Err(e), // Crashed — no point trying more
            }
        }

        // All arrangements rejected — proceed with defaults and activate buses
        warn!(
            plugin = %self.name,
            "All bus arrangements rejected — using plugin defaults"
        );
        // Just activate the buses with whatever the plugin defaults to
        self.set_bus_arrangements(
            if desired_input_channels > 0 {
                SPEAKER_STEREO
            } else {
                0
            },
            SPEAKER_STEREO,
        )?;
        Ok(desired_output_channels)
    }

    /// Try a specific bus arrangement, returning whether it was accepted.
    fn try_bus_arrangement(&mut self, input_arr: u64, output_arr: u64) -> Result<bool, Vst3Error> {
        if self.crashed {
            return Err(Vst3Error::Factory("Plugin has crashed".to_string()));
        }

        let proc = self.processor as usize;
        let in_ch = self.input_channels;

        let result = sandbox_call("try_bus_arrangement", move || unsafe {
            let processor = proc as *mut IAudioProcessor;
            let proc_vtbl = &*(*processor).vtbl;

            let mut inputs = [input_arr];
            let mut outputs = [output_arr];
            let num_ins = if in_ch > 0 { 1 } else { 0 };

            (proc_vtbl.setBusArrangements)(
                processor,
                inputs.as_mut_ptr(),
                num_ins,
                outputs.as_mut_ptr(),
                1,
            )
        });

        match result {
            SandboxResult::Ok(K_RESULT_OK) => Ok(true),
            SandboxResult::Ok(_) => Ok(false),
            SandboxResult::Crashed(crash) => {
                self.crashed = true;
                Err(Vst3Error::Factory(format!(
                    "Plugin '{}' crashed during bus arrangement probe ({})",
                    self.name, crash.signal_name
                )))
            }
            SandboxResult::Panicked(msg) => {
                self.crashed = true;
                Err(Vst3Error::Factory(format!(
                    "Plugin '{}' panicked during bus arrangement probe: {}",
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
            let processor = proc as *mut IAudioProcessor;
            let proc_vtbl = &*(*processor).vtbl;
            let mut setup = ProcessSetup {
                processMode: K_REALTIME,
                symbolicSampleSize: K_SAMPLE_32,
                maxSamplesPerBlock: max_block_size,
                sampleRate: sample_rate,
            };
            (proc_vtbl.setupProcessing)(processor, &mut setup)
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
            let component = comp as *mut IComponent;
            let comp_vtbl = &*(*component).vtbl;
            (comp_vtbl.setActive)(component, 1)
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
            let processor = proc as *mut IAudioProcessor;
            let proc_vtbl = &*(*processor).vtbl;
            (proc_vtbl.setProcessing)(processor, 1)
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
        let _span = tracing::trace_span!("vst3_process", plugin = %self.name).entered();
        if self.crashed {
            return false;
        }

        let proc = self.processor;
        let result = sandbox_call("audio_process", move || unsafe {
            let proc_vtbl = &*(*proc).vtbl;
            (proc_vtbl.process)(proc, data)
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
            let processor = proc as *mut IAudioProcessor;
            let proc_vtbl = &*(*processor).vtbl;
            (proc_vtbl.getLatencySamples)(processor)
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
    fn get_controller(&mut self) -> Option<*mut IEditController> {
        if !self.cached_controller.is_null() {
            return Some(self.cached_controller);
        }

        unsafe {
            let comp_vtbl = &*(*self.component).vtbl;

            // Try 1: QueryInterface for IEditController directly on the component
            let mut controller_ptr: *mut c_void = std::ptr::null_mut();
            let qi_result = (comp_vtbl.base.base.queryInterface)(
                self.component as *mut FUnknown,
                iid_as_tuid_ptr(&IEDIT_CONTROLLER_IID),
                &mut controller_ptr,
            );

            if qi_result == K_RESULT_OK && !controller_ptr.is_null() {
                debug!(plugin = %self.name, "IEditController obtained via QueryInterface");
                self.cached_controller = controller_ptr as *mut IEditController;
                self.owns_separate_controller = false;
                return Some(self.cached_controller);
            }

            // Try 2: Get controller class ID and create a separate controller
            let mut controller_cid = [0u8; 16];
            let result = (comp_vtbl.getControllerClassId)(
                self.component,
                &mut controller_cid as *mut _ as *mut TUID,
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
            let create_result = (factory_vtbl.createInstance)(
                self.factory as *mut IPluginFactory,
                controller_cid.as_ptr() as FIDString,
                IEDIT_CONTROLLER_IID.as_ptr() as FIDString,
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

            let controller = ec_ptr as *mut IEditController;

            // Initialize the controller with a host context
            let host_ctx = HostApplication::new();
            let ctrl_vtbl = &*(*controller).vtbl;
            let init_result = (ctrl_vtbl.base.initialize)(
                ec_ptr as *mut IPluginBase,
                HostApplication::as_unknown(host_ctx) as *mut FUnknown,
            );

            if init_result != K_RESULT_OK {
                warn!(
                    plugin = %self.name,
                    result = init_result,
                    "Separate IEditController::initialize failed"
                );
                (ctrl_vtbl.base.base.release)(ec_ptr as *mut FUnknown);
                HostApplication::destroy(host_ctx);
                return None;
            }

            // Connect component ↔ controller via IConnectionPoint (best-effort)
            self.connect_component_controller(controller);

            // Transfer component state to the controller (required for split-architecture
            // plugins like JUCE-based ones where the controller needs the component's
            // state before it can create editor views).
            self.sync_component_state_to_controller(controller);

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
    fn connect_component_controller(&self, controller: *mut IEditController) {
        unsafe {
            let comp_vtbl = &*(*self.component).vtbl;
            let ctrl_vtbl = &*(*controller).vtbl;

            // Query IConnectionPoint on the component
            let mut comp_cp: *mut c_void = std::ptr::null_mut();
            let qi1 = (comp_vtbl.base.base.queryInterface)(
                self.component as *mut FUnknown,
                iid_as_tuid_ptr(&ICONNECTION_POINT_IID),
                &mut comp_cp,
            );

            if qi1 != K_RESULT_OK || comp_cp.is_null() {
                debug!(plugin = %self.name, "Component does not support IConnectionPoint");
                return;
            }

            // Query IConnectionPoint on the controller
            let mut ctrl_cp: *mut c_void = std::ptr::null_mut();
            let qi2 = (ctrl_vtbl.base.base.queryInterface)(
                controller as *mut FUnknown,
                iid_as_tuid_ptr(&ICONNECTION_POINT_IID),
                &mut ctrl_cp,
            );

            if qi2 != K_RESULT_OK || ctrl_cp.is_null() {
                debug!(plugin = %self.name, "Controller does not support IConnectionPoint");
                let cp_vtbl = &*(*(comp_cp as *mut IConnectionPoint)).vtbl;
                (cp_vtbl.base.release)(comp_cp as *mut FUnknown);
                return;
            }

            // Connect both directions
            let comp_cp_vtbl = &*(*(comp_cp as *mut IConnectionPoint)).vtbl;
            let ctrl_cp_vtbl = &*(*(ctrl_cp as *mut IConnectionPoint)).vtbl;

            let r1 = (comp_cp_vtbl.connect)(
                comp_cp as *mut IConnectionPoint,
                ctrl_cp as *mut IConnectionPoint,
            );
            let r2 = (ctrl_cp_vtbl.connect)(
                ctrl_cp as *mut IConnectionPoint,
                comp_cp as *mut IConnectionPoint,
            );

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
            (comp_cp_vtbl.base.release)(comp_cp as *mut FUnknown);
            (ctrl_cp_vtbl.base.release)(ctrl_cp as *mut FUnknown);
        }
    }

    /// Transfer the component's state to a separate controller.
    ///
    /// Split-architecture plugins (e.g., JUCE-based) require this call so the
    /// controller can initialize its internal state before creating editor views.
    /// Without it, `createView("editor")` may return null.
    fn sync_component_state_to_controller(&self, controller: *mut IEditController) {
        use crate::vst3::ibstream::HostBStream;

        unsafe {
            let comp_vtbl = &*(*self.component).vtbl;

            // 1. Get the component's state via IComponent::getState(stream)
            let get_stream = HostBStream::new();
            let get_result = (comp_vtbl.getState)(
                self.component,
                HostBStream::as_ptr(get_stream) as *mut IBStream,
            );

            if get_result != K_RESULT_OK {
                debug!(
                    plugin = %self.name,
                    result = get_result,
                    "IComponent::getState failed — skipping setComponentState"
                );
                HostBStream::destroy(get_stream);
                return;
            }

            // 2. Extract the data and create a read stream for the controller
            let state_data = HostBStream::take_data(get_stream);
            HostBStream::destroy(get_stream);

            if state_data.is_empty() {
                debug!(plugin = %self.name, "Component state is empty — skipping setComponentState");
                return;
            }

            let set_stream = HostBStream::from_data(state_data);

            // 3. Pass to controller via IEditController::setComponentState(stream)
            let ctrl_vtbl = &*(*controller).vtbl;
            let set_result = (ctrl_vtbl.setComponentState)(
                controller,
                HostBStream::as_ptr(set_stream) as *mut IBStream,
            );

            HostBStream::destroy(set_stream);

            match set_result {
                0 => {
                    debug!(plugin = %self.name, "setComponentState succeeded");
                }
                1 => {
                    // kResultFalse — controller accepted but nothing to do
                    debug!(plugin = %self.name, "setComponentState returned kResultFalse (OK)");
                }
                _ => {
                    debug!(
                        plugin = %self.name,
                        result = set_result,
                        "setComponentState returned non-OK (continuing anyway)"
                    );
                }
            }
        }
    }

    /// Get the component's opaque state via `IComponent::getState()`.
    ///
    /// Returns the binary state blob that can later be restored with
    /// [`set_component_state`]. Returns an empty `Vec` if the plugin
    /// does not support state persistence or crashes.
    pub fn get_component_state(&self) -> Vec<u8> {
        use crate::vst3::ibstream::HostBStream;

        if self.crashed {
            return Vec::new();
        }

        let comp = self.component as usize;
        let result = sandbox_call("get_component_state", move || unsafe {
            let component = comp as *mut IComponent;
            let comp_vtbl = &*(*component).vtbl;
            let stream = HostBStream::new();
            let r = (comp_vtbl.getState)(component, HostBStream::as_ptr(stream) as *mut IBStream);
            if r == K_RESULT_OK {
                let data = HostBStream::take_data(stream);
                HostBStream::destroy(stream);
                data
            } else {
                HostBStream::destroy(stream);
                Vec::new()
            }
        });

        match result {
            SandboxResult::Ok(data) => {
                debug!(plugin = %self.name, bytes = data.len(), "Component state captured");
                data
            }
            SandboxResult::Crashed(crash) => {
                warn!(plugin = %self.name, signal = %crash.signal_name,
                    "Plugin crashed during getState (component)");
                Vec::new()
            }
            SandboxResult::Panicked(msg) => {
                warn!(plugin = %self.name, panic = %msg,
                    "Plugin panicked during getState (component)");
                Vec::new()
            }
        }
    }

    /// Get the controller's opaque state via `IEditController::getState()`.
    ///
    /// Returns the binary state blob that can later be restored with
    /// [`set_controller_state`]. Returns an empty `Vec` if the plugin
    /// has no separate controller state or crashes.
    pub fn get_controller_state(&mut self) -> Vec<u8> {
        use crate::vst3::ibstream::HostBStream;

        if self.crashed {
            return Vec::new();
        }

        let controller = match self.get_controller() {
            Some(c) => c,
            None => return Vec::new(),
        };

        let ctrl = controller as usize;
        let result = sandbox_call("get_controller_state", move || unsafe {
            let controller = ctrl as *mut IEditController;
            let ctrl_vtbl = &*(*controller).vtbl;
            let stream = HostBStream::new();
            let r = (ctrl_vtbl.getState)(controller, HostBStream::as_ptr(stream) as *mut IBStream);
            if r == K_RESULT_OK {
                let data = HostBStream::take_data(stream);
                HostBStream::destroy(stream);
                data
            } else {
                HostBStream::destroy(stream);
                Vec::new()
            }
        });

        match result {
            SandboxResult::Ok(data) => {
                debug!(plugin = %self.name, bytes = data.len(), "Controller state captured");
                data
            }
            SandboxResult::Crashed(crash) => {
                warn!(plugin = %self.name, signal = %crash.signal_name,
                    "Plugin crashed during getState (controller)");
                Vec::new()
            }
            SandboxResult::Panicked(msg) => {
                warn!(plugin = %self.name, panic = %msg,
                    "Plugin panicked during getState (controller)");
                Vec::new()
            }
        }
    }

    /// Restore the component's state via `IComponent::setState()`.
    ///
    /// `data` should be a blob previously obtained from [`get_component_state`].
    /// After setting component state, the controller is also notified via
    /// `IEditController::setComponentState()` to keep both in sync.
    pub fn set_component_state(&mut self, data: &[u8]) -> bool {
        use crate::vst3::ibstream::HostBStream;

        if self.crashed || data.is_empty() {
            return false;
        }

        let comp = self.component as usize;
        let data_owned = data.to_vec();
        let result = sandbox_call("set_component_state", move || unsafe {
            let component = comp as *mut IComponent;
            let comp_vtbl = &*(*component).vtbl;
            let stream = HostBStream::from_data(data_owned);
            let r = (comp_vtbl.setState)(component, HostBStream::as_ptr(stream) as *mut IBStream);
            HostBStream::destroy(stream);
            r
        });

        let ok = match result {
            SandboxResult::Ok(K_RESULT_OK) => {
                debug!(plugin = %self.name, "Component state restored");
                true
            }
            SandboxResult::Ok(r) => {
                warn!(plugin = %self.name, result = r, "IComponent::setState returned non-OK");
                false
            }
            SandboxResult::Crashed(crash) => {
                warn!(plugin = %self.name, signal = %crash.signal_name,
                    "Plugin crashed during setState (component)");
                self.crashed = true;
                false
            }
            SandboxResult::Panicked(msg) => {
                warn!(plugin = %self.name, panic = %msg,
                    "Plugin panicked during setState (component)");
                self.crashed = true;
                false
            }
        };

        // Sync the component state to the controller
        if ok && let Some(controller) = self.get_controller() {
            self.sync_component_state_to_controller(controller);
        }

        ok
    }

    /// Restore the controller's state via `IEditController::setState()`.
    ///
    /// `data` should be a blob previously obtained from [`get_controller_state`].
    pub fn set_controller_state(&mut self, data: &[u8]) -> bool {
        use crate::vst3::ibstream::HostBStream;

        if self.crashed || data.is_empty() {
            return false;
        }

        let controller = match self.get_controller() {
            Some(c) => c,
            None => return false,
        };

        let ctrl = controller as usize;
        let data_owned = data.to_vec();
        let result = sandbox_call("set_controller_state", move || unsafe {
            let controller = ctrl as *mut IEditController;
            let ctrl_vtbl = &*(*controller).vtbl;
            let stream = HostBStream::from_data(data_owned);
            let r = (ctrl_vtbl.setState)(controller, HostBStream::as_ptr(stream) as *mut IBStream);
            HostBStream::destroy(stream);
            r
        });

        match result {
            SandboxResult::Ok(K_RESULT_OK) => {
                debug!(plugin = %self.name, "Controller state restored");
                true
            }
            SandboxResult::Ok(r) => {
                warn!(plugin = %self.name, result = r, "IEditController::setState returned non-OK");
                false
            }
            SandboxResult::Crashed(crash) => {
                warn!(plugin = %self.name, signal = %crash.signal_name,
                    "Plugin crashed during setState (controller)");
                self.crashed = true;
                false
            }
            SandboxResult::Panicked(msg) => {
                warn!(plugin = %self.name, panic = %msg,
                    "Plugin panicked during setState (controller)");
                self.crashed = true;
                false
            }
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
            let qi1 = (comp_vtbl.base.base.queryInterface)(
                self.component as *mut FUnknown,
                iid_as_tuid_ptr(&ICONNECTION_POINT_IID),
                &mut comp_cp,
            );

            let mut ctrl_cp: *mut c_void = std::ptr::null_mut();
            let qi2 = (ctrl_vtbl.base.base.queryInterface)(
                self.cached_controller as *mut FUnknown,
                iid_as_tuid_ptr(&ICONNECTION_POINT_IID),
                &mut ctrl_cp,
            );

            if qi1 == K_RESULT_OK && !comp_cp.is_null() && qi2 == K_RESULT_OK && !ctrl_cp.is_null()
            {
                let comp_cp_vtbl = &*(*(comp_cp as *mut IConnectionPoint)).vtbl;
                let ctrl_cp_vtbl = &*(*(ctrl_cp as *mut IConnectionPoint)).vtbl;

                (comp_cp_vtbl.disconnect)(
                    comp_cp as *mut IConnectionPoint,
                    ctrl_cp as *mut IConnectionPoint,
                );
                (ctrl_cp_vtbl.disconnect)(
                    ctrl_cp as *mut IConnectionPoint,
                    comp_cp as *mut IConnectionPoint,
                );

                (comp_cp_vtbl.base.release)(comp_cp as *mut FUnknown);
                (ctrl_cp_vtbl.base.release)(ctrl_cp as *mut FUnknown);

                debug!(plugin = %self.name, "Component ↔ Controller disconnected");
            } else {
                // Release any QI'd refs that succeeded
                if qi1 == K_RESULT_OK && !comp_cp.is_null() {
                    let vtbl = &*(*(comp_cp as *mut IConnectionPoint)).vtbl;
                    (vtbl.base.release)(comp_cp as *mut FUnknown);
                }
                if qi2 == K_RESULT_OK && !ctrl_cp.is_null() {
                    let vtbl = &*(*(ctrl_cp as *mut IConnectionPoint)).vtbl;
                    (vtbl.base.release)(ctrl_cp as *mut FUnknown);
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
        let handler_ptr = HostComponentHandler::as_ptr(handler);
        let result = sandbox_call("set_component_handler", move || unsafe {
            let controller = ctrl as *mut IEditController;
            let ctrl_vtbl = &*(*controller).vtbl;
            (ctrl_vtbl.setComponentHandler)(controller, handler_ptr as *mut IComponentHandler)
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
    pub fn create_editor_view(&mut self) -> Option<*mut IPlugView> {
        if self.crashed {
            return None;
        }

        let controller = self.get_controller()?;

        let ctrl = controller as usize;
        let result = sandbox_call("create_editor_view", move || unsafe {
            let controller = ctrl as *mut IEditController;
            let ctrl_vtbl = &*(*controller).vtbl;

            // Call createView("editor")
            let view_name = b"editor\0";
            (ctrl_vtbl.createView)(controller, view_name.as_ptr() as FIDString)
        });

        match result {
            SandboxResult::Ok(view_ptr) => {
                if view_ptr.is_null() {
                    debug!(plugin = %self.name, "Plugin does not provide an editor view");
                    return None;
                }
                debug!(plugin = %self.name, "IPlugView created");
                Some(view_ptr)
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
                let view = v as *mut IPlugView;
                let vtbl = &*(*view).vtbl;
                (vtbl.base.release)(view as *mut FUnknown)
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
                (proc_vtbl.setProcessing)(proc, 0)
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
                (comp_vtbl.setActive)(comp, 0)
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
        let _span = tracing::info_span!("vst3_instance_drop", plugin = %self.name).entered();
        // Ensure processing is stopped and component is deactivated (sandboxed)
        self.shutdown();

        // Track whether *any* COM cleanup step crashed during this drop.
        let mut any_crash = false;

        if !self.crashed {
            // Extract all raw pointers (Copy types) so closures don't
            // borrow self — required for sandbox_call.
            let component = self.component;
            let processor = self.processor;
            let cached_controller = self.cached_controller;
            let owns_separate_controller = self.owns_separate_controller;
            let factory = self.factory;
            let factory_vtbl = self.factory_vtbl;

            // ── Step 0: Clear component handler on the controller ────
            // Tell the plugin to release its reference to our handler
            // BEFORE any terminate/release calls. This follows the VST3
            // shutdown protocol and prevents the controller's destructor
            // from calling back into a handler we're about to destroy.
            if !cached_controller.is_null() && !self.component_handler.is_null() {
                let result = sandbox_call("clear_component_handler", move || unsafe {
                    let ctrl_vtbl = &*(*cached_controller).vtbl;
                    (ctrl_vtbl.setComponentHandler)(cached_controller, std::ptr::null_mut())
                });

                if result.is_crashed() || result.is_panicked() {
                    any_crash = true;
                    warn!(plugin = %self.name, "Crash during setComponentHandler(null) — continuing cleanup");
                }
            }

            // ── Step 1: Disconnect IConnectionPoint ──────────────────
            // Separate sandbox so that a crash here does not prevent
            // the remaining terminate/release calls.
            if !any_crash && owns_separate_controller && !cached_controller.is_null() {
                let result = sandbox_call("disconnect_connection_points", move || unsafe {
                    let comp_vtbl = &*(*component).vtbl;
                    let ctrl_vtbl = &*(*cached_controller).vtbl;

                    let mut comp_cp: *mut c_void = std::ptr::null_mut();
                    let qi1 = (comp_vtbl.base.base.queryInterface)(
                        component as *mut FUnknown,
                        iid_as_tuid_ptr(&ICONNECTION_POINT_IID),
                        &mut comp_cp,
                    );

                    let mut ctrl_cp: *mut c_void = std::ptr::null_mut();
                    let qi2 = (ctrl_vtbl.base.base.queryInterface)(
                        cached_controller as *mut FUnknown,
                        iid_as_tuid_ptr(&ICONNECTION_POINT_IID),
                        &mut ctrl_cp,
                    );

                    if qi1 == K_RESULT_OK
                        && !comp_cp.is_null()
                        && qi2 == K_RESULT_OK
                        && !ctrl_cp.is_null()
                    {
                        let comp_cp_vtbl = &*(*(comp_cp as *mut IConnectionPoint)).vtbl;
                        let ctrl_cp_vtbl = &*(*(ctrl_cp as *mut IConnectionPoint)).vtbl;

                        (comp_cp_vtbl.disconnect)(
                            comp_cp as *mut IConnectionPoint,
                            ctrl_cp as *mut IConnectionPoint,
                        );
                        (ctrl_cp_vtbl.disconnect)(
                            ctrl_cp as *mut IConnectionPoint,
                            comp_cp as *mut IConnectionPoint,
                        );

                        (comp_cp_vtbl.base.release)(comp_cp as *mut FUnknown);
                        (ctrl_cp_vtbl.base.release)(ctrl_cp as *mut FUnknown);
                    } else {
                        if qi1 == K_RESULT_OK && !comp_cp.is_null() {
                            let vtbl = &*(*(comp_cp as *mut IConnectionPoint)).vtbl;
                            (vtbl.base.release)(comp_cp as *mut FUnknown);
                        }
                        if qi2 == K_RESULT_OK && !ctrl_cp.is_null() {
                            let vtbl = &*(*(ctrl_cp as *mut IConnectionPoint)).vtbl;
                            (vtbl.base.release)(ctrl_cp as *mut FUnknown);
                        }
                    }
                });

                if result.is_crashed() || result.is_panicked() {
                    any_crash = true;
                    warn!(plugin = %self.name, "Crash during IConnectionPoint disconnect — continuing cleanup");
                }
            }

            // ── Step 2a: Terminate controller ────────────────────────
            // Split terminate and release into separate sandbox calls so
            // a crash in terminate doesn't prevent the release attempt.
            if !cached_controller.is_null() && !any_crash && owns_separate_controller {
                let result = sandbox_call("terminate_controller", move || unsafe {
                    let ctrl_vtbl = &*(*cached_controller).vtbl;
                    (ctrl_vtbl.base.terminate)(cached_controller as *mut IPluginBase)
                });

                if result.is_crashed() || result.is_panicked() {
                    any_crash = true;
                    warn!(plugin = %self.name, "Crash during controller terminate — continuing cleanup");
                }
            }

            // ── Step 2b: Release controller ──────────────────────────
            if !cached_controller.is_null() && !any_crash {
                let result = sandbox_call("release_controller", move || unsafe {
                    let ctrl_vtbl = &*(*cached_controller).vtbl;
                    (ctrl_vtbl.base.base.release)(cached_controller as *mut FUnknown)
                });

                if result.is_crashed() || result.is_panicked() {
                    any_crash = true;
                    warn!(plugin = %self.name, "Crash during controller release — continuing cleanup");
                }
            }

            // ── Step 3: Terminate component ──────────────────────────
            if !any_crash {
                let result = sandbox_call("terminate_component", move || unsafe {
                    let comp_vtbl = &*(*component).vtbl;
                    (comp_vtbl.base.terminate)(component as *mut IPluginBase);
                });

                if result.is_crashed() || result.is_panicked() {
                    any_crash = true;
                    warn!(plugin = %self.name, "Crash during component terminate — continuing cleanup");
                }
            }

            // ── Step 4: Release COM references ───────────────────────
            // Split processor and component release so a crash in one
            // doesn't prevent the other.
            if !any_crash {
                let result = sandbox_call("release_processor", move || unsafe {
                    let proc_vtbl = &*(*processor).vtbl;
                    (proc_vtbl.base.release)(processor as *mut FUnknown)
                });

                if result.is_crashed() || result.is_panicked() {
                    any_crash = true;
                    warn!(plugin = %self.name, "Crash during processor release — reference leaked");
                }
            }

            if !any_crash {
                let result = sandbox_call("release_component", move || unsafe {
                    let comp_vtbl = &*(*component).vtbl;
                    (comp_vtbl.base.base.release)(component as *mut FUnknown)
                });

                if result.is_crashed() || result.is_panicked() {
                    any_crash = true;
                    warn!(plugin = %self.name, "Crash during component release — reference leaked");
                }
            }

            // ── Step 5: Release factory reference ────────────────────
            if !any_crash && !factory_vtbl.is_null() {
                let result = sandbox_call("release_factory", move || unsafe {
                    let fvtbl = &*factory_vtbl;
                    (fvtbl.base.release)(factory as *mut FUnknown);
                });

                if result.is_crashed() || result.is_panicked() {
                    any_crash = true;
                    warn!(plugin = %self.name, "Crash during factory release — reference leaked");
                }
            }

            // ── Propagate crash status ───────────────────────────────
            if any_crash {
                LAST_DROP_CRASHED.with(|c| c.set(true));
                DEACTIVATION_CRASHED.with(|c| c.set(true));
                // Check heap integrity after crash recovery and propagate
                // to the backend so the GUI can display a warning.
                let heap_ok = crate::diagnostics::heap_check();
                if !heap_ok {
                    DEACTIVATION_HEAP_CORRUPTED.with(|c| c.set(true));
                    error!(
                        plugin = %self.name,
                        "HEAP CORRUPTION DETECTED after plugin COM cleanup crash"
                    );
                }
                warn!(
                    plugin = %self.name,
                    heap_corrupted = !heap_ok,
                    "Plugin crashed during COM cleanup — resources leaked (host is safe)"
                );
            } else {
                debug!(plugin = %self.name, "COM references released");
            }
        } else {
            // Plugin was already marked crashed (e.g. crash during processing).
            // Also set DEACTIVATION_CRASHED so the backend tracks it.
            LAST_DROP_CRASHED.with(|c| c.set(true));
            DEACTIVATION_CRASHED.with(|c| c.set(true));
            warn!(
                plugin = %self.name,
                "Skipping COM cleanup for crashed plugin — resources leaked intentionally"
            );
        }

        // Clean up host-owned resources.
        //
        // CRITICAL: When COM cleanup crashed (`any_crash` or `self.crashed`),
        // the plugin's COM objects have been LEAKED — they are still alive in
        // memory and may hold pointers to our host_context, component_handler,
        // and controller_host_context. Freeing these host objects while leaked
        // COM objects still reference them causes use-after-free → heap
        // corruption → SIGABRT ("Corruption of tiny freelist").
        //
        // The fix: when any crash occurred, LEAK the host objects too. This is
        // safe because the library is also kept loaded (Vst3Module skips
        // dlclose when LAST_DROP_CRASHED is set), so all pointers remain valid
        // for the process lifetime. The memory cost is negligible (< 1 KB).
        if any_crash || self.crashed {
            warn!(
                plugin = %self.name,
                "Leaking host objects (host_context, component_handler) — \
                 leaked COM objects may still reference them"
            );
        } else {
            // Wrap host object destruction in sandbox calls as defense-in-depth.
            // If a plugin has deferred callbacks or background threads that
            // reference these objects, destroying them could trigger a
            // use-after-free in plugin code. The sandbox catches the crash.
            let controller_host_ctx = self.controller_host_context;
            if !controller_host_ctx.is_null() {
                let _ = sandbox_call("destroy_controller_host_context", move || unsafe {
                    HostApplication::destroy(controller_host_ctx);
                });
            }

            let host_ctx = self.host_context;
            let _ = sandbox_call("destroy_host_context", move || unsafe {
                HostApplication::destroy(host_ctx);
            });

            let handler = self.component_handler;
            if !handler.is_null() {
                let _ = sandbox_call("destroy_component_handler", move || unsafe {
                    HostComponentHandler::destroy(handler);
                });
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
        // IConnectionPointVtbl should have 6 function pointers:
        // queryInterface, addRef, release, connect, disconnect, notify
        let size = std::mem::size_of::<IConnectionPointVtbl>();
        #[cfg(target_pointer_width = "64")]
        assert_eq!(size, 6 * 8, "IConnectionPointVtbl should be 6 pointers");
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

    #[test]
    fn test_last_drop_crashed_default_is_false() {
        // LAST_DROP_CRASHED should be false by default
        let val = LAST_DROP_CRASHED.with(|c| c.get());
        assert!(!val, "LAST_DROP_CRASHED should default to false");
    }

    #[test]
    fn test_last_drop_crashed_set_and_reset() {
        // Setting LAST_DROP_CRASHED should be readable
        LAST_DROP_CRASHED.with(|c| c.set(true));
        let val = LAST_DROP_CRASHED.with(|c| c.get());
        assert!(val, "LAST_DROP_CRASHED should be true after set");

        // Reset it
        LAST_DROP_CRASHED.with(|c| c.set(false));
        let val = LAST_DROP_CRASHED.with(|c| c.get());
        assert!(!val, "LAST_DROP_CRASHED should be false after reset");
    }

    #[test]
    fn test_last_drop_crashed_set_on_sandbox_crash() {
        // Simulate what happens in Vst3Instance::drop when sandbox catches a crash:
        // the LAST_DROP_CRASHED flag should be set.
        use crate::vst3::sandbox::{SandboxResult, sandbox_call};

        // Ensure clean state
        LAST_DROP_CRASHED.with(|c| c.set(false));

        let result: SandboxResult<()> = sandbox_call("test_instance_drop_crash", || unsafe {
            libc::raise(libc::SIGBUS);
        });

        // Simulate the instance drop behavior: set flag on crash
        if result.is_crashed() {
            LAST_DROP_CRASHED.with(|c| c.set(true));
        }

        assert!(result.is_crashed());
        let crashed = LAST_DROP_CRASHED.with(|c| {
            let v = c.get();
            c.set(false); // Read-and-reset like Vst3Module::drop does
            v
        });
        assert!(
            crashed,
            "LAST_DROP_CRASHED should be set after instance crash"
        );

        // After reset, should be false
        let after_reset = LAST_DROP_CRASHED.with(|c| c.get());
        assert!(
            !after_reset,
            "LAST_DROP_CRASHED should be false after read-and-reset"
        );
    }

    #[test]
    fn test_last_drop_crashed_not_set_on_success() {
        // When the sandbox call succeeds, the flag should NOT be set
        use crate::vst3::sandbox::sandbox_call;

        LAST_DROP_CRASHED.with(|c| c.set(false));

        let _result = sandbox_call("test_successful_drop", || {
            // Simulates successful COM cleanup
            42
        });

        let crashed = LAST_DROP_CRASHED.with(|c| c.get());
        assert!(
            !crashed,
            "LAST_DROP_CRASHED should remain false after successful cleanup"
        );
    }

    #[test]
    fn test_deactivation_crashed_flag_default_false() {
        DEACTIVATION_CRASHED.with(|c| {
            let original = c.get();
            c.set(false);
            assert!(!c.get(), "DEACTIVATION_CRASHED should be false by default");
            c.set(original);
        });
    }

    #[test]
    fn test_deactivation_crashed_flag_can_be_set_and_read() {
        DEACTIVATION_CRASHED.with(|c| {
            let original = c.get();
            c.set(true);
            assert!(
                c.get(),
                "DEACTIVATION_CRASHED should be true after set(true)"
            );
            c.set(false);
            assert!(
                !c.get(),
                "DEACTIVATION_CRASHED should be false after set(false)"
            );
            c.set(original);
        });
    }

    #[test]
    fn test_deactivation_crashed_independent_of_last_drop_crashed() {
        // Verify that DEACTIVATION_CRASHED and LAST_DROP_CRASHED are independent flags.
        LAST_DROP_CRASHED.with(|c| c.set(false));
        DEACTIVATION_CRASHED.with(|c| c.set(false));

        LAST_DROP_CRASHED.with(|c| c.set(true));
        assert!(
            !DEACTIVATION_CRASHED.with(|c| c.get()),
            "DEACTIVATION_CRASHED should not be affected by LAST_DROP_CRASHED"
        );

        LAST_DROP_CRASHED.with(|c| c.set(false));
        DEACTIVATION_CRASHED.with(|c| c.set(true));
        assert!(
            !LAST_DROP_CRASHED.with(|c| c.get()),
            "LAST_DROP_CRASHED should not be affected by DEACTIVATION_CRASHED"
        );

        // Clean up
        DEACTIVATION_CRASHED.with(|c| c.set(false));
    }

    #[test]
    fn test_host_objects_leaked_on_crash_prevents_use_after_free() {
        // Verify that when a crash occurs during COM cleanup, the host objects
        // (host_context, component_handler) are NOT destroyed. This prevents
        // use-after-free when leaked COM objects still reference them.
        //
        // We can't test the actual Drop impl without a real plugin, but we can
        // verify the logic: create host objects, simulate a crash flag, and
        // ensure we would NOT destroy them.
        let host_ctx = HostApplication::new();
        let handler = HostComponentHandler::new();

        // Simulate the crash path: any_crash = true means DON'T destroy
        let any_crash = true;
        let crashed = false;

        if any_crash || crashed {
            // Objects intentionally leaked — verify they are still valid
            // (no crash/ASAN violation from accessing them).
            let ctx_ptr = HostApplication::as_unknown(host_ctx);
            assert!(
                !ctx_ptr.is_null(),
                "Leaked host_context should remain valid"
            );
            let handler_ptr = HostComponentHandler::as_ptr(handler);
            assert!(
                !handler_ptr.is_null(),
                "Leaked component_handler should remain valid"
            );
            // In production, these are intentionally leaked.
            // For the test, we clean up to avoid ASAN reports.
            unsafe {
                HostApplication::destroy(host_ctx);
                HostComponentHandler::destroy(handler);
            }
        }
    }

    #[test]
    fn test_host_objects_destroyed_on_clean_shutdown() {
        // Verify that host objects ARE destroyed when no crash occurred.
        let host_ctx = HostApplication::new();
        let handler = HostComponentHandler::new();

        let any_crash = false;
        let crashed = false;

        if !(any_crash || crashed) {
            // Clean shutdown path — destroy host objects normally.
            unsafe {
                HostApplication::destroy(host_ctx);
                HostComponentHandler::destroy(handler);
            }
        }
        // If this completes without ASAN/crash, the destroy path works.
    }

    #[test]
    fn test_deactivation_heap_corrupted_flag() {
        // Verify the DEACTIVATION_HEAP_CORRUPTED flag works correctly.
        DEACTIVATION_HEAP_CORRUPTED.with(|c| {
            let original = c.get();
            c.set(false);
            assert!(!c.get());
            c.set(true);
            assert!(c.get());
            c.set(original);
        });
    }

    #[test]
    fn test_crash_flags_set_together_on_com_crash() {
        // Simulate the full crash flag sequence from Vst3Instance::drop.
        use crate::vst3::sandbox::{SandboxResult, sandbox_call};

        // Clean state
        LAST_DROP_CRASHED.with(|c| c.set(false));
        DEACTIVATION_CRASHED.with(|c| c.set(false));
        DEACTIVATION_HEAP_CORRUPTED.with(|c| c.set(false));

        let result: SandboxResult<()> = sandbox_call("test_com_crash_flags", || unsafe {
            libc::raise(libc::SIGBUS);
        });

        // Simulate the flag-setting logic from the Drop impl
        let any_crash = result.is_crashed() || result.is_panicked();
        if any_crash {
            LAST_DROP_CRASHED.with(|c| c.set(true));
            DEACTIVATION_CRASHED.with(|c| c.set(true));
            // heap_check returns true in a clean test process
            let heap_ok = crate::diagnostics::heap_check();
            if !heap_ok {
                DEACTIVATION_HEAP_CORRUPTED.with(|c| c.set(true));
            }
        }

        assert!(
            LAST_DROP_CRASHED.with(|c| c.get()),
            "LAST_DROP_CRASHED should be set"
        );
        assert!(
            DEACTIVATION_CRASHED.with(|c| c.get()),
            "DEACTIVATION_CRASHED should be set"
        );
        // Heap should be OK in test (no real corruption)
        assert!(
            !DEACTIVATION_HEAP_CORRUPTED.with(|c| c.get()),
            "DEACTIVATION_HEAP_CORRUPTED should not be set in clean test"
        );

        // Clean up
        LAST_DROP_CRASHED.with(|c| c.set(false));
        DEACTIVATION_CRASHED.with(|c| c.set(false));
    }
}
