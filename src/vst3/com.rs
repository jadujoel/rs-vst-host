//! VST3 COM type re-exports and convenience helpers.
//!
//! This module re-exports types from the `vst3` crate (coupler-rs/vst3-rs)
//! for use throughout the host application. Using a single re-export module
//! keeps import paths short and provides a central place for helper functions.

// ─── Re-exports from vst3 crate ──────────────────────────────────────────

// Core COM types
pub use vst3::Steinberg::{
    FIDString,
    FUnknown,
    // IID constants (as TUID)
    FUnknown_iid,
    FUnknownVtbl,
    // IBStream interface
    IBStream,
    // IBStream seek modes
    IBStream_,
    IBStream_iid,
    IBStreamVtbl,
    IPlugFrame_iid,
    IPlugView_iid,
    IPluginBase_iid,
    // Plugin factory interfaces
    IPluginFactory,
    IPluginFactory_iid,
    IPluginFactory2,
    IPluginFactory2_iid,
    IPluginFactory2Vtbl,
    IPluginFactory3,
    IPluginFactory3_iid,
    IPluginFactory3Vtbl,
    IPluginFactoryVtbl,
    PClassInfo,
    PClassInfo2,
    PClassInfoW,
    // Factory/class info structs
    PFactoryInfo,
    PFactoryInfo_,
    TUID,
    // ViewRect
    ViewRect,
    char8,
    char16,
    int16,
    int32,
    int64,
    kInvalidArgument,
    kNoInterface,
    kNotImplemented,
    kPlatformTypeHWND,
    // Platform type strings
    kPlatformTypeNSView,
    kPlatformTypeX11EmbedWindowID,
    kResultFalse,
    // Result codes
    kResultOk,
    tresult,
    uint8,
    uint32,
    uint64,
};
pub use vst3::com_scrape_types::{Guid, Interface};

// VST-specific types from Steinberg::Vst
pub use vst3::Steinberg::Vst::{
    AudioBusBuffers,
    BusDirection,
    // Enum modules
    BusDirections_,
    BusIndex,
    BusInfo,
    BusType,
    Event,
    Event_,
    IAudioProcessor,
    // IID constants (as TUID)
    IAudioProcessor_iid,
    IAudioProcessorVtbl,
    // Interface types
    IComponent,
    IComponent_iid,
    IComponentHandler,
    IComponentHandler_iid,
    IComponentHandlerVtbl,
    IComponentVtbl,
    IConnectionPoint,
    IConnectionPoint_iid,
    IConnectionPointVtbl,
    IEditController,
    IEditController_iid,
    IEditControllerVtbl,
    IEventList,
    IEventList_iid,
    IEventListVtbl,
    IHostApplication,
    IHostApplication_iid,
    IHostApplicationVtbl,
    IParamValueQueue,
    IParamValueQueueVtbl,
    IParameterChanges,
    IParameterChanges_iid,
    IParameterChangesVtbl,
    IoModes_,
    MediaTypes_,
    NoteOffEvent,
    NoteOnEvent,
    // Type aliases
    ParamID,
    ParamValue,
    ParameterInfo,
    ParameterInfo_,
    ProcessContext,
    ProcessContext_,
    // Struct types
    ProcessData,
    ProcessModes_,
    ProcessSetup,
    RestartFlags_,
    Sample32,
    SampleRate,
    Speaker,
    SpeakerArr,
    SpeakerArrangement,
    String128,
    SymbolicSampleSizes_,
    TChar,
};

// EventTypes_ is nested inside Event_
pub use vst3::Steinberg::Vst::Event_::EventTypes_;

// Note: IPlugFrame exists in both Steinberg and Steinberg::Vst.
// We use the one from Steinberg for host object implementations.
pub use vst3::Steinberg::{IPlugFrame, IPlugFrameVtbl, IPlugView, IPlugViewVtbl};

// ─── Derived IID constants as [u8; 16] (Guid) ────────────────────────────
//
// The vst3 crate stores IIDs as TUID ([i8; 16]). For QueryInterface
// comparisons we need [u8; 16]. These are derived at compile time
// from the Interface::IID associated constant.

pub const FUNKNOWN_IID: [u8; 16] = <FUnknown as Interface>::IID;
pub const ICOMPONENT_IID: [u8; 16] = <IComponent as Interface>::IID;
pub const IAUDIO_PROCESSOR_IID: [u8; 16] = <IAudioProcessor as Interface>::IID;
pub const IEDIT_CONTROLLER_IID: [u8; 16] = <IEditController as Interface>::IID;
pub const IHOST_APPLICATION_IID: [u8; 16] = <IHostApplication as Interface>::IID;
pub const ICOMPONENT_HANDLER_IID: [u8; 16] = <IComponentHandler as Interface>::IID;
pub const IEVENT_LIST_IID: [u8; 16] = <IEventList as Interface>::IID;
pub const IPARAMETER_CHANGES_IID: [u8; 16] = <IParameterChanges as Interface>::IID;
pub const IPARAM_VALUE_QUEUE_IID: [u8; 16] = <IParamValueQueue as Interface>::IID;
pub const ICONNECTION_POINT_IID: [u8; 16] = <IConnectionPoint as Interface>::IID;
pub const IPLUG_VIEW_IID: [u8; 16] = <IPlugView as Interface>::IID;
pub const IPLUG_FRAME_IID: [u8; 16] = <IPlugFrame as Interface>::IID;
pub const IPLUGIN_FACTORY_IID: [u8; 16] = <IPluginFactory as Interface>::IID;
pub const IPLUGIN_FACTORY2_IID: [u8; 16] = <IPluginFactory2 as Interface>::IID;
pub const IPLUGIN_FACTORY3_IID: [u8; 16] = <IPluginFactory3 as Interface>::IID;
pub const IBSTREAM_IID: [u8; 16] = <IBStream as Interface>::IID;
pub const IPLUGIN_BASE_IID: [u8; 16] = <vst3::Steinberg::IPluginBase as Interface>::IID;

// ─── Backward-compatible constant aliases ─────────────────────────────────

/// Result code: success.
pub const K_RESULT_OK: i32 = kResultOk;

/// Media type: audio.
pub const K_AUDIO: i32 = MediaTypes_::kAudio as i32;

/// Bus direction: input.
pub const K_INPUT: i32 = BusDirections_::kInput as i32;

/// Bus direction: output.
pub const K_OUTPUT: i32 = BusDirections_::kOutput as i32;

/// Sample size: 32-bit float.
pub const K_SAMPLE_32: i32 = SymbolicSampleSizes_::kSample32 as i32;

/// Process mode: real-time.
pub const K_REALTIME: i32 = ProcessModes_::kRealtime as i32;

/// Speaker arrangement: stereo (L + R).
pub const K_SPEAKER_STEREO: u64 = SpeakerArr::kStereo;

/// Speaker arrangement: mono.
#[allow(dead_code)]
pub const K_SPEAKER_MONO: u64 = SpeakerArr::kMono;

/// Event type: Note On.
pub const K_NOTE_ON_EVENT: u16 = EventTypes_::kNoteOnEvent as u16;

/// Event type: Note Off.
pub const K_NOTE_OFF_EVENT: u16 = EventTypes_::kNoteOffEvent as u16;

/// Event flags: is live (real-time input).
pub const K_IS_LIVE: u16 = Event_::EventFlags_::kIsLive as u16;

/// Parameter flag: can automate.
pub const K_CAN_AUTOMATE: i32 = ParameterInfo_::ParameterFlags_::kCanAutomate as i32;

/// Parameter flag: read-only.
#[allow(dead_code)]
pub const K_IS_READ_ONLY: i32 = ParameterInfo_::ParameterFlags_::kIsReadOnly as i32;

/// Parameter flag: is bypass.
#[allow(dead_code)]
pub const K_IS_BYPASS: i32 = ParameterInfo_::ParameterFlags_::kIsBypass as i32;

/// Result code: not implemented.
pub const K_NOT_IMPLEMENTED: i32 = kNotImplemented;

/// Result code: false (no error but operation not performed).
pub const K_RESULT_FALSE: i32 = kResultFalse;

/// Result code: invalid argument.
pub const K_INVALID_ARGUMENT: i32 = kInvalidArgument;

/// Platform type: macOS NSView.
pub const K_PLATFORM_TYPE_NSVIEW: &[u8] = b"NSView\0";

// ─── Convenience helpers ──────────────────────────────────────────────────

/// Convert a null-terminated char8 (i8) buffer to a Rust String.
pub fn char8_to_string(buf: &[char8]) -> String {
    let bytes: &[u8] = unsafe { &*(buf as *const [char8] as *const [u8]) };
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// Convert a null-terminated char16 (u16) buffer to a Rust String.
pub fn char16_to_string(buf: &[char16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Convert a String128 (TChar = char16) to a Rust String.
pub fn string128_to_string(s: &String128) -> String {
    char16_to_string(s)
}

/// Write a Rust string into a String128 buffer (UTF-16, null-terminated).
pub fn write_string128(dst: &mut String128, src: &str) {
    let mut i = 0;
    for c in src.encode_utf16() {
        if i >= 127 {
            break;
        }
        dst[i] = c;
        i += 1;
    }
    dst[i] = 0;
}

/// Convert a TUID ([i8; 16]) to a [u8; 16] for comparison.
pub fn tuid_to_bytes(tuid: &TUID) -> [u8; 16] {
    unsafe { *(tuid as *const TUID as *const [u8; 16]) }
}

/// Get the width of a ViewRect.
pub fn view_rect_width(rect: &ViewRect) -> i32 {
    rect.right - rect.left
}

/// Get the height of a ViewRect.
pub fn view_rect_height(rect: &ViewRect) -> i32 {
    rect.bottom - rect.top
}

/// Create a NoteOnEvent.
pub fn make_note_on_event(
    sample_offset: i32,
    channel: i16,
    pitch: i16,
    velocity: f32,
    note_id: i32,
) -> Event {
    let mut event: Event = unsafe { std::mem::zeroed() };
    event.busIndex = 0;
    event.sampleOffset = sample_offset;
    event.ppqPosition = 0.0;
    event.flags = K_IS_LIVE;
    event.r#type = K_NOTE_ON_EVENT;
    unsafe {
        let note = &mut event.__field0.noteOn;
        note.channel = channel;
        note.pitch = pitch;
        note.tuning = 0.0;
        note.velocity = velocity;
        note.length = 0;
        note.noteId = note_id;
    }
    event
}

/// Create a NoteOffEvent.
pub fn make_note_off_event(
    sample_offset: i32,
    channel: i16,
    pitch: i16,
    velocity: f32,
    note_id: i32,
) -> Event {
    let mut event: Event = unsafe { std::mem::zeroed() };
    event.busIndex = 0;
    event.sampleOffset = sample_offset;
    event.ppqPosition = 0.0;
    event.flags = K_IS_LIVE;
    event.r#type = K_NOTE_OFF_EVENT;
    unsafe {
        let note = &mut event.__field0.noteOff;
        note.channel = channel;
        note.pitch = pitch;
        note.tuning = 0.0;
        note.velocity = velocity;
        note.noteId = note_id;
    }
    event
}

/// Read a NoteOnEvent from an Event (unchecked).
pub unsafe fn event_as_note_on(event: &Event) -> &NoteOnEvent {
    unsafe { &event.__field0.noteOn }
}

/// Read a NoteOffEvent from an Event (unchecked).
pub unsafe fn event_as_note_off(event: &Event) -> &NoteOffEvent {
    unsafe { &event.__field0.noteOff }
}

/// Return None for empty strings, Some(s) otherwise.
pub fn non_empty(s: String) -> Option<String> {
    if s.is_empty() { None } else { Some(s) }
}

/// Cast a [u8; 16] IID to *const TUID for QueryInterface calls.
#[inline]
pub fn iid_as_tuid_ptr(iid: &[u8; 16]) -> *const TUID {
    iid as *const [u8; 16] as *const TUID
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uuid_to_big_endian(uuid: &str) -> [u8; 16] {
        let hex: String = uuid.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        assert_eq!(hex.len(), 32);
        let mut bytes = [0u8; 16];
        for i in 0..16 {
            bytes[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
        }
        bytes
    }

    #[test]
    fn test_icomponent_iid() {
        let expected = uuid_to_big_endian("E831FF31-F2D5-4301-928E-BBEE25697802");
        #[cfg(not(target_os = "windows"))]
        assert_eq!(ICOMPONENT_IID, expected);
    }

    #[test]
    fn test_iaudio_processor_iid() {
        let expected = uuid_to_big_endian("42043F99-B7DA-453C-A569-E79D9AAEC33D");
        #[cfg(not(target_os = "windows"))]
        assert_eq!(IAUDIO_PROCESSOR_IID, expected);
    }

    #[test]
    fn test_funknown_iid() {
        let expected = uuid_to_big_endian("00000000-0000-0000-C000-000000000046");
        assert_eq!(FUNKNOWN_IID, expected);
    }

    #[test]
    fn test_iedit_controller_iid() {
        let expected = uuid_to_big_endian("DCD7BBE3-7742-448D-A874-AACC979C759E");
        #[cfg(not(target_os = "windows"))]
        assert_eq!(IEDIT_CONTROLLER_IID, expected);
    }

    #[test]
    fn test_iplugin_factory_iids() {
        let f2 = uuid_to_big_endian("0007B650-F24B-4C0B-A464-EDB9F00B2ABB");
        let f3 = uuid_to_big_endian("4555A2AB-C123-4E57-9B12-291036878931");
        #[cfg(not(target_os = "windows"))]
        {
            assert_eq!(IPLUGIN_FACTORY2_IID, f2);
            assert_eq!(IPLUGIN_FACTORY3_IID, f3);
        }
    }

    #[test]
    fn test_char8_to_string() {
        let buf: [char8; 6] = [b'H' as char8, b'i' as char8, 0, 0, 0, 0];
        assert_eq!(char8_to_string(&buf), "Hi");
    }

    #[test]
    fn test_char16_to_string() {
        let buf: [char16; 4] = [b'O' as u16, b'K' as u16, 0, 0];
        assert_eq!(char16_to_string(&buf), "OK");
    }

    #[test]
    fn test_write_string128() {
        let mut s: String128 = [0; 128];
        write_string128(&mut s, "Test");
        assert_eq!(string128_to_string(&s), "Test");
    }

    #[test]
    fn test_view_rect_dimensions() {
        let rect = ViewRect {
            left: 10,
            top: 20,
            right: 810,
            bottom: 620,
        };
        assert_eq!(view_rect_width(&rect), 800);
        assert_eq!(view_rect_height(&rect), 600);
    }

    #[test]
    fn test_backward_compat_constants() {
        assert_eq!(K_RESULT_OK, 0);
        assert_eq!(K_AUDIO, 0);
        assert_eq!(K_INPUT, 0);
        assert_eq!(K_OUTPUT, 1);
        assert_eq!(K_SAMPLE_32, 0);
        assert_eq!(K_REALTIME, 0);
        assert_eq!(K_SPEAKER_STEREO, 3);
        assert_eq!(K_NOTE_ON_EVENT, 0);
        assert_eq!(K_NOTE_OFF_EVENT, 1);
        assert_eq!(K_IS_LIVE, 1);
    }

    #[test]
    fn test_make_note_on_event() {
        let event = make_note_on_event(128, 0, 60, 0.8, -1);
        assert_eq!(event.r#type, K_NOTE_ON_EVENT);
        assert_eq!(event.sampleOffset, 128);
        let note = unsafe { event_as_note_on(&event) };
        assert_eq!(note.pitch, 60);
        assert!((note.velocity - 0.8).abs() < 0.001);
        assert_eq!(note.noteId, -1);
    }

    #[test]
    fn test_make_note_off_event() {
        let event = make_note_off_event(256, 0, 60, 0.0, -1);
        assert_eq!(event.r#type, K_NOTE_OFF_EVENT);
        let note = unsafe { event_as_note_off(&event) };
        assert_eq!(note.pitch, 60);
    }

    #[test]
    fn test_iid_lengths() {
        assert_eq!(FUNKNOWN_IID.len(), 16);
        assert_eq!(ICOMPONENT_IID.len(), 16);
        assert_eq!(IAUDIO_PROCESSOR_IID.len(), 16);
        assert_eq!(IEDIT_CONTROLLER_IID.len(), 16);
        assert_eq!(IHOST_APPLICATION_IID.len(), 16);
        assert_eq!(ICOMPONENT_HANDLER_IID.len(), 16);
        assert_eq!(IEVENT_LIST_IID.len(), 16);
        assert_eq!(IPARAMETER_CHANGES_IID.len(), 16);
        assert_eq!(IPARAM_VALUE_QUEUE_IID.len(), 16);
        assert_eq!(ICONNECTION_POINT_IID.len(), 16);
        assert_eq!(IPLUG_VIEW_IID.len(), 16);
        assert_eq!(IPLUG_FRAME_IID.len(), 16);
    }

    #[test]
    fn test_non_empty() {
        assert_eq!(non_empty(String::new()), None);
        assert_eq!(non_empty("hello".into()), Some("hello".into()));
    }
}
