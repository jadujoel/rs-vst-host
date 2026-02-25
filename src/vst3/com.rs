//! VST3 COM interface vtable definitions and IIDs for component/processor access.
//!
//! These are manual FFI definitions matching the Steinberg VST3 SDK C++ ABI.
//! All vtables use `#[repr(C)]` to match C++ virtual function table layout.

use std::ffi::c_void;

// ─── Result codes ─────────────────────────────────────────────────────────

pub const K_RESULT_OK: i32 = 0;
#[allow(dead_code)]
pub const K_RESULT_FALSE: i32 = 1;
pub const K_NOT_IMPLEMENTED: i32 = -1;
#[allow(dead_code)]
pub const K_INVALID_ARGUMENT: i32 = -2;

// ─── Processing constants ─────────────────────────────────────────────────

/// Sample size: 32-bit float.
pub const K_SAMPLE_32: i32 = 0;

/// Process mode: real-time.
pub const K_REALTIME: i32 = 0;

/// Media type: audio.
pub const K_AUDIO: i32 = 0;

/// Bus direction: input.
pub const K_INPUT: i32 = 0;

/// Bus direction: output.
pub const K_OUTPUT: i32 = 1;

/// Speaker arrangement: stereo (L + R).
pub const K_SPEAKER_STEREO: u64 = 0x03;

/// Speaker arrangement: mono (L only).
#[allow(dead_code)]
pub const K_SPEAKER_MONO: u64 = 0x01;

// ─── Interface IIDs ───────────────────────────────────────────────────────

/// IComponent IID: {E831FF31-F2D5-4301-928E-BBEE25697802}
/// Big-endian byte order (macOS/Linux FUID format).
#[cfg(not(target_os = "windows"))]
pub const ICOMPONENT_IID: [u8; 16] = [
    0xE8, 0x31, 0xFF, 0x31, 0xF2, 0xD5, 0x43, 0x01, 0x92, 0x8E, 0xBB, 0xEE, 0x25, 0x69, 0x78,
    0x02,
];

#[cfg(target_os = "windows")]
pub const ICOMPONENT_IID: [u8; 16] = [
    0x31, 0xFF, 0x31, 0xE8, 0x01, 0x43, 0xD5, 0xF2, 0x92, 0x8E, 0xBB, 0xEE, 0x25, 0x69, 0x78,
    0x02,
];

/// IAudioProcessor IID: {42043F99-B7DA-453C-A569-E79D9AAEC33F}
#[cfg(not(target_os = "windows"))]
pub const IAUDIO_PROCESSOR_IID: [u8; 16] = [
    0x42, 0x04, 0x3F, 0x99, 0xB7, 0xDA, 0x45, 0x3C, 0xA5, 0x69, 0xE7, 0x9D, 0x9A, 0xAE, 0xC3,
    0x3F,
];

#[cfg(target_os = "windows")]
pub const IAUDIO_PROCESSOR_IID: [u8; 16] = [
    0x99, 0x3F, 0x04, 0x42, 0x3C, 0x45, 0xDA, 0xB7, 0xA5, 0x69, 0xE7, 0x9D, 0x9A, 0xAE, 0xC3,
    0x3F,
];

/// IHostApplication IID: {58E595CC-DB2D-4969-8B6A-AF8C36A664E5}
#[cfg(not(target_os = "windows"))]
pub const IHOST_APPLICATION_IID: [u8; 16] = [
    0x58, 0xE5, 0x95, 0xCC, 0xDB, 0x2D, 0x49, 0x69, 0x8B, 0x6A, 0xAF, 0x8C, 0x36, 0xA6, 0x64,
    0xE5,
];

#[cfg(target_os = "windows")]
pub const IHOST_APPLICATION_IID: [u8; 16] = [
    0xCC, 0x95, 0xE5, 0x58, 0x69, 0x49, 0x2D, 0xDB, 0x8B, 0x6A, 0xAF, 0x8C, 0x36, 0xA6, 0x64,
    0xE5,
];

/// FUnknown IID: {00000000-0000-0000-C000-000000000046}
pub const FUNKNOWN_IID: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x46,
];

// ─── COM base vtable ──────────────────────────────────────────────────────

/// IUnknown/FUnknown vtable.
#[repr(C)]
#[allow(dead_code)]
pub struct FUnknownVtbl {
    pub query_interface:
        unsafe extern "system" fn(this: *mut c_void, iid: *const u8, obj: *mut *mut c_void) -> i32,
    pub add_ref: unsafe extern "system" fn(this: *mut c_void) -> u32,
    pub release: unsafe extern "system" fn(this: *mut c_void) -> u32,
}

// ─── IPluginBase + IComponent vtable ──────────────────────────────────────

/// IComponent vtable (extends IPluginBase extends FUnknown).
///
/// vtable layout:
///   [0-2]  FUnknown: queryInterface, addRef, release
///   [3-4]  IPluginBase: initialize, terminate
///   [5-13] IComponent: getControllerClassId .. getState
#[repr(C)]
pub struct IComponentVtbl {
    // FUnknown
    pub query_interface:
        unsafe extern "system" fn(this: *mut c_void, iid: *const u8, obj: *mut *mut c_void) -> i32,
    pub add_ref: unsafe extern "system" fn(this: *mut c_void) -> u32,
    pub release: unsafe extern "system" fn(this: *mut c_void) -> u32,
    // IPluginBase
    pub initialize: unsafe extern "system" fn(this: *mut c_void, context: *mut c_void) -> i32,
    pub terminate: unsafe extern "system" fn(this: *mut c_void) -> i32,
    // IComponent
    pub get_controller_class_id:
        unsafe extern "system" fn(this: *mut c_void, class_id: *mut [u8; 16]) -> i32,
    pub set_io_mode: unsafe extern "system" fn(this: *mut c_void, mode: i32) -> i32,
    pub get_bus_count:
        unsafe extern "system" fn(this: *mut c_void, media_type: i32, dir: i32) -> i32,
    pub get_bus_info: unsafe extern "system" fn(
        this: *mut c_void,
        media_type: i32,
        dir: i32,
        index: i32,
        bus: *mut BusInfo,
    ) -> i32,
    pub get_routing_info: unsafe extern "system" fn(
        this: *mut c_void,
        in_info: *mut c_void,
        out_info: *mut c_void,
    ) -> i32,
    pub activate_bus: unsafe extern "system" fn(
        this: *mut c_void,
        media_type: i32,
        dir: i32,
        index: i32,
        state: u8,
    ) -> i32,
    pub set_active: unsafe extern "system" fn(this: *mut c_void, state: u8) -> i32,
    pub set_state: unsafe extern "system" fn(this: *mut c_void, state: *mut c_void) -> i32,
    pub get_state: unsafe extern "system" fn(this: *mut c_void, state: *mut c_void) -> i32,
}

/// BusInfo struct matching the VST3 SDK layout.
#[repr(C)]
pub struct BusInfo {
    pub media_type: i32,
    pub direction: i32,
    pub channel_count: i32,
    pub name: [u16; 128],
    pub bus_type: i32,
    pub flags: u32,
}

// ─── IAudioProcessor vtable ───────────────────────────────────────────────

/// IAudioProcessor vtable (extends FUnknown).
///
/// vtable layout:
///   [0-2]  FUnknown: queryInterface, addRef, release
///   [3-10] IAudioProcessor: setBusArrangements .. getTailSamples
#[repr(C)]
pub struct IAudioProcessorVtbl {
    // FUnknown
    pub query_interface:
        unsafe extern "system" fn(this: *mut c_void, iid: *const u8, obj: *mut *mut c_void) -> i32,
    pub add_ref: unsafe extern "system" fn(this: *mut c_void) -> u32,
    pub release: unsafe extern "system" fn(this: *mut c_void) -> u32,
    // IAudioProcessor
    pub set_bus_arrangements: unsafe extern "system" fn(
        this: *mut c_void,
        inputs: *mut u64,
        num_ins: i32,
        outputs: *mut u64,
        num_outs: i32,
    ) -> i32,
    pub get_bus_arrangement: unsafe extern "system" fn(
        this: *mut c_void,
        dir: i32,
        index: i32,
        arr: *mut u64,
    ) -> i32,
    pub can_process_sample_size:
        unsafe extern "system" fn(this: *mut c_void, symbolic_sample_size: i32) -> i32,
    pub get_latency_samples: unsafe extern "system" fn(this: *mut c_void) -> u32,
    pub setup_processing:
        unsafe extern "system" fn(this: *mut c_void, setup: *mut ProcessSetup) -> i32,
    pub set_processing: unsafe extern "system" fn(this: *mut c_void, state: u8) -> i32,
    pub process: unsafe extern "system" fn(this: *mut c_void, data: *mut ProcessData) -> i32,
    pub get_tail_samples: unsafe extern "system" fn(this: *mut c_void) -> u32,
}

// ─── Process data structures ──────────────────────────────────────────────

/// ProcessSetup — sent to IAudioProcessor::setupProcessing.
#[repr(C)]
pub struct ProcessSetup {
    pub process_mode: i32,
    pub symbolic_sample_size: i32,
    pub max_samples_per_block: i32,
    // Note: 4 bytes of padding here due to f64 alignment
    pub sample_rate: f64,
}

/// ProcessData — passed to IAudioProcessor::process each block.
#[repr(C)]
pub struct ProcessData {
    pub process_mode: i32,
    pub symbolic_sample_size: i32,
    pub num_samples: i32,
    pub num_inputs: i32,
    pub num_outputs: i32,
    // Note: padding before pointers on 64-bit
    pub inputs: *mut AudioBusBuffers,
    pub outputs: *mut AudioBusBuffers,
    pub input_parameter_changes: *mut c_void,
    pub output_parameter_changes: *mut c_void,
    pub input_events: *mut c_void,
    pub output_events: *mut c_void,
    pub process_context: *mut c_void,
}

/// AudioBusBuffers — per-bus buffer pointers for process().
#[repr(C)]
pub struct AudioBusBuffers {
    pub num_channels: i32,
    // Note: padding before u64 on 32-bit
    pub silence_flags: u64,
    pub channel_buffers_32: *mut *mut f32,
}

// ─── COM pointer wrapper ──────────────────────────────────────────────────

/// Generic COM object: pointer-to-vtable-pointer layout.
#[repr(C)]
pub struct ComPtr<V> {
    pub vtbl: *const V,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem;

    #[test]
    fn test_process_setup_layout() {
        // ProcessSetup should be 24 bytes on all platforms:
        // 3 × i32 (12) + 4 padding + f64 (8) = 24
        assert_eq!(mem::size_of::<ProcessSetup>(), 24);
    }

    #[test]
    fn test_audio_bus_buffers_layout() {
        // AudioBusBuffers: i32 (4) + 4 padding + u64 (8) + ptr (8) = 24
        assert_eq!(mem::size_of::<AudioBusBuffers>(), 24);
    }

    #[test]
    fn test_bus_info_has_name_field() {
        let bi: BusInfo = unsafe { mem::zeroed() };
        assert_eq!(bi.name.len(), 128);
        assert_eq!(bi.channel_count, 0);
    }

    #[test]
    fn test_iid_lengths() {
        assert_eq!(ICOMPONENT_IID.len(), 16);
        assert_eq!(IAUDIO_PROCESSOR_IID.len(), 16);
        assert_eq!(IHOST_APPLICATION_IID.len(), 16);
        assert_eq!(FUNKNOWN_IID.len(), 16);
    }

    #[test]
    fn test_process_data_layout() {
        // ProcessData should be 80 bytes on 64-bit:
        // 5 × i32 (20) + 4 padding + 7 × ptr (56) = 80
        #[cfg(target_pointer_width = "64")]
        assert_eq!(mem::size_of::<ProcessData>(), 80);
    }

    #[test]
    fn test_speaker_arrangements() {
        assert_eq!(K_SPEAKER_STEREO, 3);
        assert_eq!(K_SPEAKER_MONO, 1);
    }
}
