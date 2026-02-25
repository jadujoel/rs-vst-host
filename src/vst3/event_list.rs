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
    query_interface: host_event_list_query_interface,
    add_ref: host_event_list_add_ref,
    release: host_event_list_release,
    get_event_count: host_event_list_get_event_count,
    get_event: host_event_list_get_event,
    add_event: host_event_list_add_event,
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
    this: *mut c_void,
    iid: *const u8,
    obj: *mut *mut c_void,
) -> i32 {
    if obj.is_null() || iid.is_null() {
        return K_INVALID_ARGUMENT;
    }

    let iid_slice = unsafe { std::slice::from_raw_parts(iid, 16) };

    if iid_slice == IEVENT_LIST_IID || iid_slice == FUNKNOWN_IID {
        unsafe { *obj = this };
        K_RESULT_OK
    } else {
        unsafe { *obj = std::ptr::null_mut() };
        K_NOT_IMPLEMENTED
    }
}

unsafe extern "system" fn host_event_list_add_ref(_this: *mut c_void) -> u32 {
    // Static lifetime managed by host, no real ref counting needed
    1
}

unsafe extern "system" fn host_event_list_release(_this: *mut c_void) -> u32 {
    // Destroyed explicitly by host
    1
}

unsafe extern "system" fn host_event_list_get_event_count(this: *mut c_void) -> i32 {
    let list = this as *const HostEventList;
    unsafe { (*list).events.len() as i32 }
}

unsafe extern "system" fn host_event_list_get_event(
    this: *mut c_void,
    index: i32,
    event: *mut Event,
) -> i32 {
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

unsafe extern "system" fn host_event_list_add_event(this: *mut c_void, event: *const Event) -> i32 {
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

            let note_on = Event::note_on(0, 0, 60, 0.8, -1);
            HostEventList::add(list, note_on);
            assert_eq!(HostEventList::event_count(list), 1);

            let note_off = Event::note_off(128, 0, 60, 0.0, -1);
            HostEventList::add(list, note_off);
            assert_eq!(HostEventList::event_count(list), 2);

            HostEventList::destroy(list);
        }
    }

    #[test]
    fn test_clear_events() {
        let list = HostEventList::new();

        unsafe {
            HostEventList::add(list, Event::note_on(0, 0, 60, 0.8, -1));
            HostEventList::add(list, Event::note_on(0, 0, 64, 0.8, -1));
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
            let note = Event::note_on(42, 0, 72, 0.9, 5);
            HostEventList::add(list, note);

            // Call through the vtable (as a plugin would)
            let vtbl = &*(*list).vtbl;
            let count = (vtbl.get_event_count)(list as *mut c_void);
            assert_eq!(count, 1);

            let mut retrieved = std::mem::zeroed::<Event>();
            let result = (vtbl.get_event)(list as *mut c_void, 0, &mut retrieved);
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(retrieved.event_type, K_NOTE_ON_EVENT);
            assert_eq!(retrieved.sample_offset, 42);

            let note_data: &NoteOnEvent = &*(retrieved.data.as_ptr() as *const NoteOnEvent);
            assert_eq!(note_data.pitch, 72);
            assert!((note_data.velocity - 0.9).abs() < 0.001);
            assert_eq!(note_data.note_id, 5);

            HostEventList::destroy(list);
        }
    }

    #[test]
    fn test_get_event_out_of_range() {
        let list = HostEventList::new();

        unsafe {
            let vtbl = &*(*list).vtbl;
            let mut evt = std::mem::zeroed::<Event>();

            let result = (vtbl.get_event)(list as *mut c_void, 0, &mut evt);
            assert_ne!(result, K_RESULT_OK);

            let result = (vtbl.get_event)(list as *mut c_void, -1, &mut evt);
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
            let result = (vtbl.query_interface)(
                list as *mut c_void,
                IEVENT_LIST_IID.as_ptr(),
                &mut obj,
            );
            assert_eq!(result, K_RESULT_OK);
            assert!(!obj.is_null());

            // Should succeed for FUnknown IID
            let result = (vtbl.query_interface)(
                list as *mut c_void,
                FUNKNOWN_IID.as_ptr(),
                &mut obj,
            );
            assert_eq!(result, K_RESULT_OK);

            // Should fail for unrelated IID
            let result = (vtbl.query_interface)(
                list as *mut c_void,
                ICOMPONENT_IID.as_ptr(),
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
                HostEventList::add(list, Event::note_on(i as i32, 0, 60, 0.8, -1));
            }
            assert_eq!(HostEventList::event_count(list), MAX_EVENTS_PER_BLOCK);

            // One more should be silently dropped
            HostEventList::add(list, Event::note_on(999, 0, 60, 0.8, -1));
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
            let note = Event::note_on(10, 0, 64, 0.7, -1);
            let result = (vtbl.add_event)(list as *mut c_void, &note);
            assert_eq!(result, K_RESULT_OK);

            let count = (vtbl.get_event_count)(list as *mut c_void);
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
                let note = Event::note_on(i as i32, 0, 60, 0.8, -1);
                let result = (vtbl.add_event)(list as *mut c_void, &note);
                assert_eq!(result, K_RESULT_OK);
            }

            // One more should return K_RESULT_FALSE
            let note = Event::note_on(999, 0, 60, 0.8, -1);
            let result = (vtbl.add_event)(list as *mut c_void, &note);
            assert_ne!(result, K_RESULT_OK);

            HostEventList::destroy(list);
        }
    }

    #[test]
    fn test_add_event_null_pointer() {
        let list = HostEventList::new();

        unsafe {
            let vtbl = &*(*list).vtbl;
            let result = (vtbl.add_event)(list as *mut c_void, std::ptr::null());
            assert_eq!(result, K_INVALID_ARGUMENT);

            HostEventList::destroy(list);
        }
    }

    #[test]
    fn test_get_event_null_pointer() {
        let list = HostEventList::new();

        unsafe {
            let vtbl = &*(*list).vtbl;
            HostEventList::add(list, Event::note_on(0, 0, 60, 0.8, -1));

            let result = (vtbl.get_event)(list as *mut c_void, 0, std::ptr::null_mut());
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
            let result = (vtbl.query_interface)(
                list as *mut c_void,
                IEVENT_LIST_IID.as_ptr(),
                std::ptr::null_mut(),
            );
            assert_eq!(result, K_INVALID_ARGUMENT);

            // Null iid pointer
            let mut obj: *mut c_void = std::ptr::null_mut();
            let result = (vtbl.query_interface)(
                list as *mut c_void,
                std::ptr::null(),
                &mut obj,
            );
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
            let count = (vtbl.add_ref)(list as *mut c_void);
            assert_eq!(count, 1);

            let count = (vtbl.release)(list as *mut c_void);
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
