//! Host-side IEventList COM implementation for passing MIDI events to VST3 plugins.
//!
//! This implements the `IEventList` interface that plugins receive in `ProcessData::inputEvents`.
//! Events are added before each process call and cleared between blocks.

use crate::vst3::com::*;
use std::ffi::c_void;

/// Maximum number of events that can be queued per process block.
const MAX_EVENTS_PER_BLOCK: usize = 512;

/// Host-side IEventList COM object.
///
/// Stores events for a single process block. Use `add()` to queue events
/// before calling `process()`, then `clear()` after processing.
#[repr(C)]
pub struct HostEventList {
    /// Pointer to the static vtable.
    vtbl: *const IEventListVtbl,
    /// Stored events for the current block.
    events: Vec<Event>,
}

// Static vtable for IEventList.
static HOST_EVENT_LIST_VTBL: IEventListVtbl = IEventListVtbl {
    base: FUnknownVtbl {
        queryInterface: host_event_list_query_interface,
        addRef: host_event_list_add_ref,
        release: host_event_list_release,
    },
    getEventCount: host_event_list_get_event_count,
    getEvent: host_event_list_get_event,
    addEvent: host_event_list_add_event,
};

impl HostEventList {
    /// Create a new heap-allocated HostEventList.
    pub fn new() -> *mut Self {
        let obj = Box::new(Self {
            vtbl: &HOST_EVENT_LIST_VTBL,
            events: Vec::with_capacity(64),
        });
        Box::into_raw(obj)
    }

    /// Destroy a HostEventList created with `new()`.
    ///
    /// # Safety
    /// The pointer must have been created by `HostEventList::new()` and not yet destroyed.
    pub unsafe fn destroy(ptr: *mut Self) {
        if !ptr.is_null() {
            drop(unsafe { Box::from_raw(ptr) });
        }
    }

    /// Get this as a `*mut c_void` pointer for passing to ProcessData.
    pub fn as_ptr(ptr: *mut Self) -> *mut c_void {
        ptr as *mut c_void
    }

    /// Add an event to the list.
    ///
    /// # Safety
    /// The pointer must be valid.
    pub unsafe fn add(ptr: *mut Self, event: Event) {
        let this = unsafe { &mut *ptr };
        if this.events.len() < MAX_EVENTS_PER_BLOCK {
            this.events.push(event);
        }
    }

    /// Clear all events (call between process blocks).
    ///
    /// # Safety
    /// The pointer must be valid.
    pub unsafe fn clear(ptr: *mut Self) {
        let this = unsafe { &mut *ptr };
        this.events.clear();
    }

    /// Get the number of events in the list.
    ///
    /// # Safety
    /// The pointer must be valid.
    #[allow(dead_code)]
    pub unsafe fn event_count(ptr: *mut Self) -> usize {
        let this = unsafe { &*ptr };
        this.events.len()
    }
}

// --- COM vtable implementations ---

unsafe extern "system" fn host_event_list_query_interface(
    this: *mut FUnknown,
    iid: *const TUID,
    obj: *mut *mut c_void,
) -> tresult {
    if obj.is_null() || iid.is_null() {
        return K_INVALID_ARGUMENT;
    }

    // Direct 16-byte array comparison — avoids fat-pointer slice construction.
    let iid_bytes: [u8; 16] = unsafe { *(iid as *const [u8; 16]) };

    if iid_bytes == IEVENT_LIST_IID || iid_bytes == FUNKNOWN_IID {
        unsafe { *obj = this as *mut c_void };
        K_RESULT_OK
    } else {
        unsafe { *obj = std::ptr::null_mut() };
        K_NOT_IMPLEMENTED
    }
}

unsafe extern "system" fn host_event_list_add_ref(_this: *mut FUnknown) -> uint32 {
    // Static lifetime managed by host, no real ref counting needed
    1
}

unsafe extern "system" fn host_event_list_release(_this: *mut FUnknown) -> uint32 {
    // Destroyed explicitly by host
    1
}

unsafe extern "system" fn host_event_list_get_event_count(this: *mut IEventList) -> int32 {
    let list = this as *const HostEventList;
    unsafe { (*list).events.len() as i32 }
}

unsafe extern "system" fn host_event_list_get_event(
    this: *mut IEventList,
    index: int32,
    event: *mut Event,
) -> tresult {
    if event.is_null() {
        return K_INVALID_ARGUMENT;
    }

    let list = this as *const HostEventList;
    let events = unsafe { &(*list).events };

    if index < 0 || (index as usize) >= events.len() {
        return K_INVALID_ARGUMENT;
    }

    unsafe { *event = events[index as usize] };
    K_RESULT_OK
}

unsafe extern "system" fn host_event_list_add_event(
    this: *mut IEventList,
    event: *mut Event,
) -> tresult {
    if event.is_null() {
        return K_INVALID_ARGUMENT;
    }

    let list = this as *mut HostEventList;
    let events = unsafe { &mut (*list).events };

    if events.len() >= MAX_EVENTS_PER_BLOCK {
        return K_RESULT_FALSE;
    }

    events.push(unsafe { *event });
    K_RESULT_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_destroy() {
        let list = HostEventList::new();
        assert!(!list.is_null());
        unsafe { HostEventList::destroy(list) };
    }

    #[test]
    fn test_add_and_count_events() {
        let list = HostEventList::new();

        unsafe {
            assert_eq!(HostEventList::event_count(list), 0);

            let note_on = make_note_on_event(0, 0, 60, 0.8, -1);
            HostEventList::add(list, note_on);
            assert_eq!(HostEventList::event_count(list), 1);

            let note_off = make_note_off_event(128, 0, 60, 0.0, -1);
            HostEventList::add(list, note_off);
            assert_eq!(HostEventList::event_count(list), 2);

            HostEventList::destroy(list);
        }
    }

    #[test]
    fn test_clear_events() {
        let list = HostEventList::new();

        unsafe {
            HostEventList::add(list, make_note_on_event(0, 0, 60, 0.8, -1));
            HostEventList::add(list, make_note_on_event(0, 0, 64, 0.8, -1));
            assert_eq!(HostEventList::event_count(list), 2);

            HostEventList::clear(list);
            assert_eq!(HostEventList::event_count(list), 0);

            HostEventList::destroy(list);
        }
    }

    #[test]
    fn test_get_event_via_vtable() {
        let list = HostEventList::new();

        unsafe {
            let note = make_note_on_event(42, 0, 72, 0.9, 5);
            HostEventList::add(list, note);

            // Call through the vtable (as a plugin would)
            let vtbl = &*(*list).vtbl;
            let count = (vtbl.getEventCount)(list as *mut IEventList);
            assert_eq!(count, 1);

            let mut retrieved = std::mem::zeroed::<Event>();
            let result = (vtbl.getEvent)(list as *mut IEventList, 0, &mut retrieved);
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(retrieved.r#type, K_NOTE_ON_EVENT);
            assert_eq!(retrieved.sampleOffset, 42);

            let note_data = event_as_note_on(&retrieved);
            assert_eq!(note_data.pitch, 72);
            assert!((note_data.velocity - 0.9).abs() < 0.001);
            assert_eq!(note_data.noteId, 5);

            HostEventList::destroy(list);
        }
    }

    #[test]
    fn test_get_event_out_of_range() {
        let list = HostEventList::new();

        unsafe {
            let vtbl = &*(*list).vtbl;
            let mut evt = std::mem::zeroed::<Event>();

            let result = (vtbl.getEvent)(list as *mut IEventList, 0, &mut evt);
            assert_ne!(result, K_RESULT_OK);

            let result = (vtbl.getEvent)(list as *mut IEventList, -1, &mut evt);
            assert_ne!(result, K_RESULT_OK);

            HostEventList::destroy(list);
        }
    }

    #[test]
    fn test_query_interface() {
        let list = HostEventList::new();

        unsafe {
            let vtbl = &*(*list).vtbl;
            let mut obj: *mut c_void = std::ptr::null_mut();

            // Should succeed for IEventList IID
            let result = (vtbl.base.queryInterface)(
                list as *mut FUnknown,
                &IEVENT_LIST_IID as *const [u8; 16] as *const TUID,
                &mut obj,
            );
            assert_eq!(result, K_RESULT_OK);
            assert!(!obj.is_null());

            // Should succeed for FUnknown IID
            let result = (vtbl.base.queryInterface)(
                list as *mut FUnknown,
                &FUNKNOWN_IID as *const [u8; 16] as *const TUID,
                &mut obj,
            );
            assert_eq!(result, K_RESULT_OK);

            // Should fail for unrelated IID
            let result = (vtbl.base.queryInterface)(
                list as *mut FUnknown,
                &ICOMPONENT_IID as *const [u8; 16] as *const TUID,
                &mut obj,
            );
            assert_ne!(result, K_RESULT_OK);

            HostEventList::destroy(list);
        }
    }

    #[test]
    fn test_max_events_per_block_overflow() {
        let list = HostEventList::new();

        unsafe {
            // Fill to the max
            for i in 0..MAX_EVENTS_PER_BLOCK {
                HostEventList::add(list, make_note_on_event(i as i32, 0, 60, 0.8, -1));
            }
            assert_eq!(HostEventList::event_count(list), MAX_EVENTS_PER_BLOCK);

            // One more should be silently dropped
            HostEventList::add(list, make_note_on_event(999, 0, 60, 0.8, -1));
            assert_eq!(
                HostEventList::event_count(list),
                MAX_EVENTS_PER_BLOCK,
                "Should not exceed MAX_EVENTS_PER_BLOCK"
            );

            HostEventList::destroy(list);
        }
    }

    #[test]
    fn test_add_event_via_vtable() {
        let list = HostEventList::new();

        unsafe {
            let vtbl = &*(*list).vtbl;
            let mut note = make_note_on_event(10, 0, 64, 0.7, -1);
            let result = (vtbl.addEvent)(list as *mut IEventList, &mut note);
            assert_eq!(result, K_RESULT_OK);

            let count = (vtbl.getEventCount)(list as *mut IEventList);
            assert_eq!(count, 1);

            HostEventList::destroy(list);
        }
    }

    #[test]
    fn test_add_event_via_vtable_overflow() {
        let list = HostEventList::new();

        unsafe {
            let vtbl = &*(*list).vtbl;

            // Fill to the max via vtable
            for i in 0..MAX_EVENTS_PER_BLOCK {
                let mut note = make_note_on_event(i as i32, 0, 60, 0.8, -1);
                let result = (vtbl.addEvent)(list as *mut IEventList, &mut note);
                assert_eq!(result, K_RESULT_OK);
            }

            // One more should return K_RESULT_FALSE
            let mut note = make_note_on_event(999, 0, 60, 0.8, -1);
            let result = (vtbl.addEvent)(list as *mut IEventList, &mut note);
            assert_ne!(result, K_RESULT_OK);

            HostEventList::destroy(list);
        }
    }

    #[test]
    fn test_add_event_null_pointer() {
        let list = HostEventList::new();

        unsafe {
            let vtbl = &*(*list).vtbl;
            let result = (vtbl.addEvent)(list as *mut IEventList, std::ptr::null_mut());
            assert_eq!(result, K_INVALID_ARGUMENT);

            HostEventList::destroy(list);
        }
    }

    #[test]
    fn test_get_event_null_pointer() {
        let list = HostEventList::new();

        unsafe {
            let vtbl = &*(*list).vtbl;
            HostEventList::add(list, make_note_on_event(0, 0, 60, 0.8, -1));

            let result = (vtbl.getEvent)(list as *mut IEventList, 0, std::ptr::null_mut());
            assert_eq!(result, K_INVALID_ARGUMENT);

            HostEventList::destroy(list);
        }
    }

    #[test]
    fn test_query_interface_null_params() {
        let list = HostEventList::new();

        unsafe {
            let vtbl = &*(*list).vtbl;

            // Null obj pointer
            let result = (vtbl.base.queryInterface)(
                list as *mut FUnknown,
                &IEVENT_LIST_IID as *const [u8; 16] as *const TUID,
                std::ptr::null_mut(),
            );
            assert_eq!(result, K_INVALID_ARGUMENT);

            // Null iid pointer
            let mut obj: *mut c_void = std::ptr::null_mut();
            let result =
                (vtbl.base.queryInterface)(list as *mut FUnknown, std::ptr::null(), &mut obj);
            assert_eq!(result, K_INVALID_ARGUMENT);

            HostEventList::destroy(list);
        }
    }

    #[test]
    fn test_add_ref_release() {
        let list = HostEventList::new();

        unsafe {
            let vtbl = &*(*list).vtbl;
            // Static lifetime — add_ref/release should return 1
            let count = (vtbl.base.addRef)(list as *mut FUnknown);
            assert_eq!(count, 1);

            let count = (vtbl.base.release)(list as *mut FUnknown);
            assert_eq!(count, 1);

            HostEventList::destroy(list);
        }
    }

    #[test]
    fn test_as_ptr() {
        let list = HostEventList::new();
        let ptr = HostEventList::as_ptr(list);
        assert!(!ptr.is_null());
        assert_eq!(ptr, list as *mut c_void);
        unsafe { HostEventList::destroy(list) };
    }
}
