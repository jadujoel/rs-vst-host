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

// ─── Event constants ──────────────────────────────────────────────────────

/// Event type: Note On.
pub const K_NOTE_ON_EVENT: u16 = 0;

/// Event type: Note Off.
pub const K_NOTE_OFF_EVENT: u16 = 1;

/// Event flags: is live (real-time input).
pub const K_IS_LIVE: u16 = 1;

// ─── Interface IIDs ───────────────────────────────────────────────────────

/// IComponent IID: {E831FF31-F2D5-4301-928E-BBEE25697802}
/// Big-endian byte order (macOS/Linux FUID format).
#[cfg(not(target_os = "windows"))]
pub const ICOMPONENT_IID: [u8; 16] = [
    0xE8, 0x31, 0xFF, 0x31, 0xF2, 0xD5, 0x43, 0x01, 0x92, 0x8E, 0xBB, 0xEE, 0x25, 0x69, 0x78, 0x02,
];

/// IComponent IID in COM-compatible byte order (Windows).
#[cfg(target_os = "windows")]
pub const ICOMPONENT_IID: [u8; 16] = [
    0x31, 0xFF, 0x31, 0xE8, 0xD5, 0xF2, 0x01, 0x43, 0x92, 0x8E, 0xBB, 0xEE, 0x25, 0x69, 0x78, 0x02,
];

/// IAudioProcessor IID: {42043F99-B7DA-453C-A569-E79D9AAEC33D}
#[cfg(not(target_os = "windows"))]
pub const IAUDIO_PROCESSOR_IID: [u8; 16] = [
    0x42, 0x04, 0x3F, 0x99, 0xB7, 0xDA, 0x45, 0x3C, 0xA5, 0x69, 0xE7, 0x9D, 0x9A, 0xAE, 0xC3, 0x3D,
];

/// IAudioProcessor IID in COM-compatible byte order (Windows).
#[cfg(target_os = "windows")]
pub const IAUDIO_PROCESSOR_IID: [u8; 16] = [
    0x99, 0x3F, 0x04, 0x42, 0xDA, 0xB7, 0x3C, 0x45, 0xA5, 0x69, 0xE7, 0x9D, 0x9A, 0xAE, 0xC3, 0x3D,
];

/// IHostApplication IID: {58E595CC-DB2D-4969-8B6A-AF8C36A664E5}
#[cfg(not(target_os = "windows"))]
pub const IHOST_APPLICATION_IID: [u8; 16] = [
    0x58, 0xE5, 0x95, 0xCC, 0xDB, 0x2D, 0x49, 0x69, 0x8B, 0x6A, 0xAF, 0x8C, 0x36, 0xA6, 0x64, 0xE5,
];

/// IHostApplication IID in COM-compatible byte order (Windows).
#[cfg(target_os = "windows")]
pub const IHOST_APPLICATION_IID: [u8; 16] = [
    0xCC, 0x95, 0xE5, 0x58, 0x2D, 0xDB, 0x69, 0x49, 0x8B, 0x6A, 0xAF, 0x8C, 0x36, 0xA6, 0x64, 0xE5,
];

/// FUnknown IID: {00000000-0000-0000-C000-000000000046}
pub const FUNKNOWN_IID: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
];

/// IEditController IID: {DCD7BBE3-7742-448D-A874-AACC979C759E}
#[cfg(not(target_os = "windows"))]
pub const IEDIT_CONTROLLER_IID: [u8; 16] = [
    0xDC, 0xD7, 0xBB, 0xE3, 0x77, 0x42, 0x44, 0x8D, 0xA8, 0x74, 0xAA, 0xCC, 0x97, 0x9C, 0x75, 0x9E,
];

/// IEditController IID in COM-compatible byte order (Windows).
#[cfg(target_os = "windows")]
pub const IEDIT_CONTROLLER_IID: [u8; 16] = [
    0xE3, 0xBB, 0xD7, 0xDC, 0x42, 0x77, 0x8D, 0x44, 0xA8, 0x74, 0xAA, 0xCC, 0x97, 0x9C, 0x75, 0x9E,
];

/// IEventList IID: {3A2C4214-3463-49FE-B2C4-F397B9695A44}
#[cfg(not(target_os = "windows"))]
pub const IEVENT_LIST_IID: [u8; 16] = [
    0x3A, 0x2C, 0x42, 0x14, 0x34, 0x63, 0x49, 0xFE, 0xB2, 0xC4, 0xF3, 0x97, 0xB9, 0x69, 0x5A, 0x44,
];

/// IEventList IID in COM-compatible byte order (Windows).
#[cfg(target_os = "windows")]
pub const IEVENT_LIST_IID: [u8; 16] = [
    0x14, 0x42, 0x2C, 0x3A, 0x63, 0x34, 0xFE, 0x49, 0xB2, 0xC4, 0xF3, 0x97, 0xB9, 0x69, 0x5A, 0x44,
];

/// IParameterChanges IID: {A4779663-0BB6-4A56-B443-84A8466FEB9D}
#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
pub const IPARAMETER_CHANGES_IID: [u8; 16] = [
    0xA4, 0x77, 0x96, 0x63, 0x0B, 0xB6, 0x4A, 0x56, 0xB4, 0x43, 0x84, 0xA8, 0x46, 0x6F, 0xEB, 0x9D,
];

/// IParameterChanges IID in COM-compatible byte order (Windows).
#[cfg(target_os = "windows")]
#[allow(dead_code)]
pub const IPARAMETER_CHANGES_IID: [u8; 16] = [
    0x63, 0x96, 0x77, 0xA4, 0xB6, 0x0B, 0x56, 0x4A, 0xB4, 0x43, 0x84, 0xA8, 0x46, 0x6F, 0xEB, 0x9D,
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
    pub get_bus_arrangement:
        unsafe extern "system" fn(this: *mut c_void, dir: i32, index: i32, arr: *mut u64) -> i32,
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

// ─── Event structures ─────────────────────────────────────────────────────

/// VST3 Event union — the common header + type-specific data.
///
/// In the C++ SDK this is a struct with a union. We represent it as a
/// `#[repr(C)]` struct with the union data as a byte array large enough
/// for the biggest variant (NoteOnEvent = 20 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Event {
    /// Bus index.
    pub bus_index: i32,
    /// Sample offset within current block.
    pub sample_offset: i32,
    /// Position on musical timeline (quarters from start).
    pub ppq_position: f64,
    /// Event flags (e.g. `K_IS_LIVE`).
    pub flags: u16,
    /// Event type (e.g. `K_NOTE_ON_EVENT`, `K_NOTE_OFF_EVENT`).
    pub event_type: u16,
    // 4 bytes of padding to align the union data.
    _pad: [u8; 4],
    /// Union data — interpret based on `event_type`.
    /// Sized for the largest event variant.
    pub data: [u8; 20],
}

impl Event {
    /// Create a Note On event.
    pub fn note_on(
        sample_offset: i32,
        channel: i16,
        pitch: i16,
        velocity: f32,
        note_id: i32,
    ) -> Self {
        let mut event = Self {
            bus_index: 0,
            sample_offset,
            ppq_position: 0.0,
            flags: K_IS_LIVE,
            event_type: K_NOTE_ON_EVENT,
            _pad: [0; 4],
            data: [0; 20],
        };

        // NoteOnEvent layout: channel(i16) + pitch(i16) + tuning(f32) + velocity(f32) + length(i32) + noteId(i32)
        let note_on = NoteOnEvent {
            channel,
            pitch,
            tuning: 0.0,
            velocity,
            length: 0,
            note_id,
        };
        let bytes = unsafe {
            std::slice::from_raw_parts(
                &note_on as *const NoteOnEvent as *const u8,
                std::mem::size_of::<NoteOnEvent>(),
            )
        };
        event.data[..bytes.len()].copy_from_slice(bytes);
        event
    }

    /// Create a Note Off event.
    pub fn note_off(
        sample_offset: i32,
        channel: i16,
        pitch: i16,
        velocity: f32,
        note_id: i32,
    ) -> Self {
        let mut event = Self {
            bus_index: 0,
            sample_offset,
            ppq_position: 0.0,
            flags: K_IS_LIVE,
            event_type: K_NOTE_OFF_EVENT,
            _pad: [0; 4],
            data: [0; 20],
        };

        // NoteOffEvent has the same layout as NoteOnEvent
        let note_off = NoteOffEvent {
            channel,
            pitch,
            tuning: 0.0,
            velocity,
            length: 0,
            note_id,
        };
        let bytes = unsafe {
            std::slice::from_raw_parts(
                &note_off as *const NoteOffEvent as *const u8,
                std::mem::size_of::<NoteOffEvent>(),
            )
        };
        event.data[..bytes.len()].copy_from_slice(bytes);
        event
    }
}

/// NoteOnEvent data layout.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NoteOnEvent {
    pub channel: i16,
    pub pitch: i16,
    pub tuning: f32,
    pub velocity: f32,
    pub length: i32,
    pub note_id: i32,
}

/// NoteOffEvent data layout.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NoteOffEvent {
    pub channel: i16,
    pub pitch: i16,
    pub tuning: f32,
    pub velocity: f32,
    pub length: i32,
    pub note_id: i32,
}

// ─── IEventList vtable ────────────────────────────────────────────────────

/// IEventList vtable (extends FUnknown).
///
/// vtable layout:
///   [0-2]  FUnknown: queryInterface, addRef, release
///   [3-4]  IEventList: getEventCount, getEvent, addEvent
#[repr(C)]
pub struct IEventListVtbl {
    // FUnknown
    pub query_interface:
        unsafe extern "system" fn(this: *mut c_void, iid: *const u8, obj: *mut *mut c_void) -> i32,
    pub add_ref: unsafe extern "system" fn(this: *mut c_void) -> u32,
    pub release: unsafe extern "system" fn(this: *mut c_void) -> u32,
    // IEventList
    pub get_event_count: unsafe extern "system" fn(this: *mut c_void) -> i32,
    pub get_event:
        unsafe extern "system" fn(this: *mut c_void, index: i32, event: *mut Event) -> i32,
    pub add_event: unsafe extern "system" fn(this: *mut c_void, event: *const Event) -> i32,
}

// ─── IEditController vtable ───────────────────────────────────────────────

/// IEditController vtable (extends IPluginBase extends FUnknown).
///
/// vtable layout:
///   [0-2]  FUnknown: queryInterface, addRef, release
///   [3-4]  IPluginBase: initialize, terminate
///   [5-16] IEditController methods
#[repr(C)]
pub struct IEditControllerVtbl {
    // FUnknown
    pub query_interface:
        unsafe extern "system" fn(this: *mut c_void, iid: *const u8, obj: *mut *mut c_void) -> i32,
    pub add_ref: unsafe extern "system" fn(this: *mut c_void) -> u32,
    pub release: unsafe extern "system" fn(this: *mut c_void) -> u32,
    // IPluginBase
    pub initialize: unsafe extern "system" fn(this: *mut c_void, context: *mut c_void) -> i32,
    pub terminate: unsafe extern "system" fn(this: *mut c_void) -> i32,
    // IEditController
    pub set_component_state:
        unsafe extern "system" fn(this: *mut c_void, state: *mut c_void) -> i32,
    pub set_state: unsafe extern "system" fn(this: *mut c_void, state: *mut c_void) -> i32,
    pub get_state: unsafe extern "system" fn(this: *mut c_void, state: *mut c_void) -> i32,
    pub get_parameter_count: unsafe extern "system" fn(this: *mut c_void) -> i32,
    pub get_parameter_info: unsafe extern "system" fn(
        this: *mut c_void,
        param_index: i32,
        info: *mut ParameterInfo,
    ) -> i32,
    pub get_param_string_by_value: unsafe extern "system" fn(
        this: *mut c_void,
        id: u32,
        value_normalized: f64,
        string: *mut u16,
    ) -> i32,
    pub get_param_value_by_string: unsafe extern "system" fn(
        this: *mut c_void,
        id: u32,
        string: *const u16,
        value_normalized: *mut f64,
    ) -> i32,
    pub normalized_param_to_plain:
        unsafe extern "system" fn(this: *mut c_void, id: u32, value_normalized: f64) -> f64,
    pub plain_param_to_normalized:
        unsafe extern "system" fn(this: *mut c_void, id: u32, plain_value: f64) -> f64,
    pub get_param_normalized: unsafe extern "system" fn(this: *mut c_void, id: u32) -> f64,
    pub set_param_normalized:
        unsafe extern "system" fn(this: *mut c_void, id: u32, value: f64) -> i32,
    pub set_component_handler:
        unsafe extern "system" fn(this: *mut c_void, handler: *mut c_void) -> i32,
    pub create_view: unsafe extern "system" fn(this: *mut c_void, name: *const u8) -> *mut c_void,
}

/// ParameterInfo — returned by IEditController::getParameterInfo.
#[repr(C)]
pub struct ParameterInfo {
    /// Parameter ID.
    pub id: u32,
    /// Parameter title (UTF-16, 128 chars max).
    pub title: [u16; 128],
    /// Short title (UTF-16, 128 chars max).
    pub short_title: [u16; 128],
    /// Units label (UTF-16, 128 chars max).
    pub units: [u16; 128],
    /// Number of discrete steps (0 = continuous).
    pub step_count: i32,
    /// Default normalized value [0..1].
    pub default_normalized_value: f64,
    /// Unit ID for grouping.
    pub unit_id: i32,
    /// Parameter flags.
    pub flags: i32,
}

/// Parameter flags for ParameterInfo.
pub const K_CAN_AUTOMATE: i32 = 1;
#[allow(dead_code)]
pub const K_IS_READ_ONLY: i32 = 1 << 1;
#[allow(dead_code)]
pub const K_IS_WRAP_AROUND: i32 = 1 << 2;
#[allow(dead_code)]
pub const K_IS_LIST: i32 = 1 << 3;
#[allow(dead_code)]
pub const K_IS_PROGRAM_CHANGE: i32 = 1 << 4;
#[allow(dead_code)]
pub const K_IS_BYPASS: i32 = 1 << 5;

// ─── IConnectionPoint vtable ──────────────────────────────────────────────

/// IConnectionPoint IID: {22888DDB-156E-45AE-8358-B34808190625}
#[cfg(not(target_os = "windows"))]
pub const ICONNECTION_POINT_IID: [u8; 16] = [
    0x22, 0x88, 0x8D, 0xDB, 0x15, 0x6E, 0x45, 0xAE, 0x83, 0x58, 0xB3, 0x48, 0x08, 0x19, 0x06, 0x25,
];

/// IConnectionPoint IID in COM-compatible byte order (Windows).
#[cfg(target_os = "windows")]
pub const ICONNECTION_POINT_IID: [u8; 16] = [
    0xDB, 0x8D, 0x88, 0x22, 0x6E, 0x15, 0xAE, 0x45, 0x83, 0x58, 0xB3, 0x48, 0x08, 0x19, 0x06, 0x25,
];

/// IConnectionPoint vtable (extends FUnknown).
///
/// vtable layout:
///   [0-2]  FUnknown: queryInterface, addRef, release
///   [3-4]  IConnectionPoint: connect, disconnect
#[repr(C)]
pub struct IConnectionPointVtbl {
    // FUnknown
    pub query_interface:
        unsafe extern "system" fn(this: *mut c_void, iid: *const u8, obj: *mut *mut c_void) -> i32,
    pub add_ref: unsafe extern "system" fn(this: *mut c_void) -> u32,
    pub release: unsafe extern "system" fn(this: *mut c_void) -> u32,
    // IConnectionPoint
    pub connect: unsafe extern "system" fn(this: *mut c_void, other: *mut c_void) -> i32,
    pub disconnect: unsafe extern "system" fn(this: *mut c_void, other: *mut c_void) -> i32,
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

    /// Convert a UUID string like "E831FF31-F2D5-4301-928E-BBEE25697802" to
    /// the expected big-endian (macOS/Linux FUID) byte array.
    fn uuid_to_big_endian(uuid: &str) -> [u8; 16] {
        let hex: String = uuid.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        assert_eq!(hex.len(), 32, "UUID must have 32 hex digits");
        let mut bytes = [0u8; 16];
        for i in 0..16 {
            bytes[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
        }
        bytes
    }

    /// Convert a UUID string to COM-compatible (Windows) byte order.
    /// l1 (bytes 0-3): little-endian u32
    /// l2 (bytes 4-7): two little-endian u16s
    /// l3-l4 (bytes 8-15): big-endian (unchanged)
    fn uuid_to_com(uuid: &str) -> [u8; 16] {
        let be = uuid_to_big_endian(uuid);
        [
            be[3], be[2], be[1], be[0], // l1 as LE u32
            be[5], be[4], be[7], be[6], // l2 as two LE u16s
            be[8], be[9], be[10], be[11], be[12], be[13], be[14], be[15],
        ]
    }

    // ─── IID correctness tests ────────────────────────────────────────────

    #[test]
    fn test_icomponent_iid_matches_uuid() {
        let expected = uuid_to_big_endian("E831FF31-F2D5-4301-928E-BBEE25697802");
        #[cfg(not(target_os = "windows"))]
        assert_eq!(ICOMPONENT_IID, expected, "IComponent IID mismatch");
        #[cfg(target_os = "windows")]
        assert_eq!(
            ICOMPONENT_IID,
            uuid_to_com("E831FF31-F2D5-4301-928E-BBEE25697802"),
            "IComponent IID COM mismatch"
        );
        let _ = expected;
    }

    #[test]
    fn test_iaudio_processor_iid_matches_uuid() {
        let expected = uuid_to_big_endian("42043F99-B7DA-453C-A569-E79D9AAEC33D");
        #[cfg(not(target_os = "windows"))]
        assert_eq!(
            IAUDIO_PROCESSOR_IID, expected,
            "IAudioProcessor IID mismatch"
        );
        #[cfg(target_os = "windows")]
        assert_eq!(
            IAUDIO_PROCESSOR_IID,
            uuid_to_com("42043F99-B7DA-453C-A569-E79D9AAEC33D"),
            "IAudioProcessor IID COM mismatch"
        );
        let _ = expected;
    }

    #[test]
    fn test_ihost_application_iid_matches_uuid() {
        let expected = uuid_to_big_endian("58E595CC-DB2D-4969-8B6A-AF8C36A664E5");
        #[cfg(not(target_os = "windows"))]
        assert_eq!(
            IHOST_APPLICATION_IID, expected,
            "IHostApplication IID mismatch"
        );
        #[cfg(target_os = "windows")]
        assert_eq!(
            IHOST_APPLICATION_IID,
            uuid_to_com("58E595CC-DB2D-4969-8B6A-AF8C36A664E5"),
            "IHostApplication IID COM mismatch"
        );
        let _ = expected;
    }

    #[test]
    fn test_funknown_iid_matches_uuid() {
        // FUnknown: {00000000-0000-0000-C000-000000000046}
        // All zeros for l1/l2, so LE and BE are the same for the first 8 bytes
        let expected = uuid_to_big_endian("00000000-0000-0000-C000-000000000046");
        assert_eq!(FUNKNOWN_IID, expected, "FUnknown IID mismatch");
    }

    #[test]
    fn test_iedit_controller_iid_matches_uuid() {
        let expected = uuid_to_big_endian("DCD7BBE3-7742-448D-A874-AACC979C759E");
        #[cfg(not(target_os = "windows"))]
        assert_eq!(
            IEDIT_CONTROLLER_IID, expected,
            "IEditController IID mismatch"
        );
        #[cfg(target_os = "windows")]
        assert_eq!(
            IEDIT_CONTROLLER_IID,
            uuid_to_com("DCD7BBE3-7742-448D-A874-AACC979C759E"),
            "IEditController IID COM mismatch"
        );
        let _ = expected;
    }

    #[test]
    fn test_ievent_list_iid_matches_uuid() {
        let expected = uuid_to_big_endian("3A2C4214-3463-49FE-B2C4-F397B9695A44");
        #[cfg(not(target_os = "windows"))]
        assert_eq!(IEVENT_LIST_IID, expected, "IEventList IID mismatch");
        #[cfg(target_os = "windows")]
        assert_eq!(
            IEVENT_LIST_IID,
            uuid_to_com("3A2C4214-3463-49FE-B2C4-F397B9695A44"),
            "IEventList IID COM mismatch"
        );
        let _ = expected;
    }

    #[test]
    fn test_iparameter_changes_iid_matches_uuid() {
        let expected = uuid_to_big_endian("A4779663-0BB6-4A56-B443-84A8466FEB9D");
        #[cfg(not(target_os = "windows"))]
        assert_eq!(
            IPARAMETER_CHANGES_IID, expected,
            "IParameterChanges IID mismatch"
        );
        #[cfg(target_os = "windows")]
        assert_eq!(
            IPARAMETER_CHANGES_IID,
            uuid_to_com("A4779663-0BB6-4A56-B443-84A8466FEB9D"),
            "IParameterChanges IID COM mismatch"
        );
        let _ = expected;
    }

    #[test]
    fn test_iconnection_point_iid_matches_uuid() {
        let expected = uuid_to_big_endian("22888DDB-156E-45AE-8358-B34808190625");
        #[cfg(not(target_os = "windows"))]
        assert_eq!(
            ICONNECTION_POINT_IID, expected,
            "IConnectionPoint IID mismatch"
        );
        #[cfg(target_os = "windows")]
        assert_eq!(
            ICONNECTION_POINT_IID,
            uuid_to_com("22888DDB-156E-45AE-8358-B34808190625"),
            "IConnectionPoint IID COM mismatch"
        );
        let _ = expected;
    }

    // ─── UUID helper tests ────────────────────────────────────────────────

    #[test]
    fn test_uuid_to_big_endian_helper() {
        let bytes = uuid_to_big_endian("E831FF31-F2D5-4301-928E-BBEE25697802");
        assert_eq!(bytes[0], 0xE8);
        assert_eq!(bytes[4], 0xF2);
        assert_eq!(bytes[15], 0x02);
    }

    #[test]
    fn test_uuid_to_com_helper() {
        let com = uuid_to_com("E831FF31-F2D5-4301-928E-BBEE25697802");
        // l1: 0xE831FF31 → LE: [0x31, 0xFF, 0x31, 0xE8]
        assert_eq!(&com[0..4], &[0x31, 0xFF, 0x31, 0xE8]);
        // l2: 0xF2D54301 → two LE u16: [0xD5, 0xF2, 0x01, 0x43]
        assert_eq!(&com[4..8], &[0xD5, 0xF2, 0x01, 0x43]);
        // l3/l4: big-endian unchanged
        assert_eq!(
            &com[8..16],
            &[0x92, 0x8E, 0xBB, 0xEE, 0x25, 0x69, 0x78, 0x02]
        );
    }

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
        assert_eq!(IEDIT_CONTROLLER_IID.len(), 16);
        assert_eq!(IEVENT_LIST_IID.len(), 16);
        assert_eq!(IPARAMETER_CHANGES_IID.len(), 16);
        assert_eq!(ICONNECTION_POINT_IID.len(), 16);
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

    #[test]
    fn test_event_size() {
        // Event struct should be 48 bytes:
        // bus_index(4) + sample_offset(4) + ppq_position(8) + flags(2) + type(2) + pad(4) + data(20) = 44
        // But with alignment the struct may have trailing padding
        let size = mem::size_of::<Event>();
        assert!(
            size >= 44,
            "Event size should be at least 44 bytes, got {}",
            size
        );
        assert!(
            size <= 48,
            "Event size should not exceed 48 bytes, got {}",
            size
        );
    }

    #[test]
    fn test_note_on_event_layout() {
        assert_eq!(mem::size_of::<NoteOnEvent>(), 20);
        assert_eq!(mem::size_of::<NoteOffEvent>(), 20);
    }

    #[test]
    fn test_event_note_on_creation() {
        let event = Event::note_on(128, 0, 60, 0.8, -1);
        assert_eq!(event.event_type, K_NOTE_ON_EVENT);
        assert_eq!(event.sample_offset, 128);
        assert_eq!(event.flags, K_IS_LIVE);
        // Verify note data
        let note: &NoteOnEvent = unsafe { &*(event.data.as_ptr() as *const NoteOnEvent) };
        assert_eq!(note.channel, 0);
        assert_eq!(note.pitch, 60);
        assert!((note.velocity - 0.8).abs() < 0.001);
        assert_eq!(note.note_id, -1);
    }

    #[test]
    fn test_event_note_off_creation() {
        let event = Event::note_off(256, 0, 60, 0.0, -1);
        assert_eq!(event.event_type, K_NOTE_OFF_EVENT);
        assert_eq!(event.sample_offset, 256);
        let note: &NoteOffEvent = unsafe { &*(event.data.as_ptr() as *const NoteOffEvent) };
        assert_eq!(note.pitch, 60);
        assert_eq!(note.velocity, 0.0);
    }

    #[test]
    fn test_parameter_info_has_title_field() {
        let pi: ParameterInfo = unsafe { mem::zeroed() };
        assert_eq!(pi.title.len(), 128);
        assert_eq!(pi.short_title.len(), 128);
        assert_eq!(pi.units.len(), 128);
    }

    #[test]
    fn test_parameter_flags() {
        assert_eq!(K_CAN_AUTOMATE, 1);
        assert_eq!(K_IS_READ_ONLY, 2);
        assert_eq!(K_IS_LIST, 8);
    }
}
