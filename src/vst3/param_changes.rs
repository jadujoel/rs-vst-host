//! Host-side IParameterChanges and IParamValueQueue COM implementations.
//!
//! Used to send parameter value changes from the host to the plugin during
//! `IAudioProcessor::process()`. Each parameter has a queue of (sampleOffset, value) points.

use crate::vst3::com::*;
use std::ffi::c_void;

/// Maximum number of parameters that can change in a single block.
const MAX_PARAM_QUEUES: usize = 64;
/// Maximum number of value points per parameter per block.
const MAX_POINTS_PER_PARAM: usize = 16;

/// A single value point in a parameter change queue.
#[derive(Clone, Copy, Default)]
struct ValuePoint {
    /// Sample offset within the block.
    sample_offset: i32,
    /// Normalized parameter value [0..1].
    value: f64,
}

// ═══════════════════════════════════════════════════════════════════════════
// IParamValueQueue
// ═══════════════════════════════════════════════════════════════════════════

/// IParamValueQueue vtable.
///
/// vtable layout:
///   [0-2]  FUnknown: queryInterface, addRef, release
///   [3]    getParameterId() -> ParamID
///   [4]    getPointCount() -> i32
///   [5]    getPoint(index, sampleOffset*, value*) -> tresult
///   [6]    addPoint(sampleOffset, value, index*) -> tresult
#[repr(C)]
struct IParamValueQueueVtbl {
    // FUnknown
    query_interface:
        unsafe extern "system" fn(this: *mut c_void, iid: *const u8, obj: *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(this: *mut c_void) -> u32,
    release: unsafe extern "system" fn(this: *mut c_void) -> u32,
    // IParamValueQueue
    get_parameter_id: unsafe extern "system" fn(this: *mut c_void) -> u32,
    get_point_count: unsafe extern "system" fn(this: *mut c_void) -> i32,
    get_point: unsafe extern "system" fn(
        this: *mut c_void,
        index: i32,
        sample_offset: *mut i32,
        value: *mut f64,
    ) -> i32,
    add_point: unsafe extern "system" fn(
        this: *mut c_void,
        sample_offset: i32,
        value: f64,
        index: *mut i32,
    ) -> i32,
}

/// Static vtable for IParamValueQueue.
static PARAM_VALUE_QUEUE_VTBL: IParamValueQueueVtbl = IParamValueQueueVtbl {
    query_interface: pvq_query_interface,
    add_ref: pvq_add_ref,
    release: pvq_release,
    get_parameter_id: pvq_get_parameter_id,
    get_point_count: pvq_get_point_count,
    get_point: pvq_get_point,
    add_point: pvq_add_point,
};

/// IParamValueQueue IID: {01263A18-ED07-4F6F-98C9-D3564686F9BA}
#[cfg(not(target_os = "windows"))]
const IPARAM_VALUE_QUEUE_IID: [u8; 16] = [
    0x01, 0x26, 0x3A, 0x18, 0xED, 0x07, 0x4F, 0x6F, 0x98, 0xC9, 0xD3, 0x56, 0x46, 0x86, 0xF9, 0xBA,
];

#[cfg(target_os = "windows")]
const IPARAM_VALUE_QUEUE_IID: [u8; 16] = [
    0x18, 0x3A, 0x26, 0x01, 0x6F, 0x4F, 0x07, 0xED, 0x98, 0xC9, 0xD3, 0x56, 0x46, 0x86, 0xF9, 0xBA,
];

/// Host-side IParamValueQueue COM object.
///
/// Stores change points for a single parameter.
/// Embedded inline within `HostParameterChanges`.
#[repr(C)]
struct HostParamValueQueue {
    /// Pointer to the static vtable.
    vtbl: *const IParamValueQueueVtbl,
    /// Parameter ID this queue is for.
    param_id: u32,
    /// Value change points.
    points: [ValuePoint; MAX_POINTS_PER_PARAM],
    /// Number of active points.
    point_count: i32,
}

impl HostParamValueQueue {
    /// Initialize the queue for a parameter.
    fn init(&mut self, param_id: u32) {
        self.vtbl = &PARAM_VALUE_QUEUE_VTBL;
        self.param_id = param_id;
        self.point_count = 0;
    }

    /// Clear all points.
    fn clear(&mut self) {
        self.point_count = 0;
    }

    /// Add a value point. Returns the index, or -1 if full.
    fn add(&mut self, sample_offset: i32, value: f64) -> i32 {
        if (self.point_count as usize) >= MAX_POINTS_PER_PARAM {
            return -1;
        }
        let idx = self.point_count;
        self.points[idx as usize] = ValuePoint {
            sample_offset,
            value,
        };
        self.point_count += 1;
        idx
    }
}

// ─── IParamValueQueue COM method implementations ──────────────────────────

unsafe extern "system" fn pvq_query_interface(
    this: *mut c_void,
    iid: *const u8,
    obj: *mut *mut c_void,
) -> i32 {
    if iid.is_null() || obj.is_null() {
        return K_NOT_IMPLEMENTED;
    }

    let iid_bytes: [u8; 16] = unsafe { *(iid as *const [u8; 16]) };
    if iid_bytes == IPARAM_VALUE_QUEUE_IID || iid_bytes == FUNKNOWN_IID {
        unsafe { *obj = this };
        return K_RESULT_OK;
    }

    unsafe { *obj = std::ptr::null_mut() };
    K_NOT_IMPLEMENTED
}

unsafe extern "system" fn pvq_add_ref(_this: *mut c_void) -> u32 {
    1 // Embedded object, lifetime managed by parent
}

unsafe extern "system" fn pvq_release(_this: *mut c_void) -> u32 {
    1
}

unsafe extern "system" fn pvq_get_parameter_id(this: *mut c_void) -> u32 {
    let queue = this as *const HostParamValueQueue;
    unsafe { (*queue).param_id }
}

unsafe extern "system" fn pvq_get_point_count(this: *mut c_void) -> i32 {
    let queue = this as *const HostParamValueQueue;
    unsafe { (*queue).point_count }
}

unsafe extern "system" fn pvq_get_point(
    this: *mut c_void,
    index: i32,
    sample_offset: *mut i32,
    value: *mut f64,
) -> i32 {
    if sample_offset.is_null() || value.is_null() {
        return K_INVALID_ARGUMENT;
    }

    let queue = this as *const HostParamValueQueue;
    unsafe {
        if index < 0 || index >= (*queue).point_count {
            return K_INVALID_ARGUMENT;
        }
        let pt = &(*queue).points[index as usize];
        *sample_offset = pt.sample_offset;
        *value = pt.value;
    }
    K_RESULT_OK
}

unsafe extern "system" fn pvq_add_point(
    this: *mut c_void,
    sample_offset: i32,
    value: f64,
    index: *mut i32,
) -> i32 {
    let queue = this as *mut HostParamValueQueue;
    unsafe {
        let idx = (*queue).add(sample_offset, value);
        if idx < 0 {
            return K_RESULT_FALSE;
        }
        if !index.is_null() {
            *index = idx;
        }
    }
    K_RESULT_OK
}

// ═══════════════════════════════════════════════════════════════════════════
// IParameterChanges
// ═══════════════════════════════════════════════════════════════════════════

/// IParameterChanges vtable.
///
/// vtable layout:
///   [0-2]  FUnknown: queryInterface, addRef, release
///   [3]    getParameterCount() -> i32
///   [4]    getParameterData(index) -> IParamValueQueue*
///   [5]    addParameterData(id, index*) -> IParamValueQueue*
#[repr(C)]
struct IParameterChangesVtbl {
    // FUnknown
    query_interface:
        unsafe extern "system" fn(this: *mut c_void, iid: *const u8, obj: *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(this: *mut c_void) -> u32,
    release: unsafe extern "system" fn(this: *mut c_void) -> u32,
    // IParameterChanges
    get_parameter_count: unsafe extern "system" fn(this: *mut c_void) -> i32,
    get_parameter_data: unsafe extern "system" fn(this: *mut c_void, index: i32) -> *mut c_void,
    add_parameter_data: unsafe extern "system" fn(
        this: *mut c_void,
        id: *const u32,
        index: *mut i32,
    ) -> *mut c_void,
}

/// Static vtable for IParameterChanges.
static PARAMETER_CHANGES_VTBL: IParameterChangesVtbl = IParameterChangesVtbl {
    query_interface: pc_query_interface,
    add_ref: pc_add_ref,
    release: pc_release,
    get_parameter_count: pc_get_parameter_count,
    get_parameter_data: pc_get_parameter_data,
    add_parameter_data: pc_add_parameter_data,
};

/// Host-side IParameterChanges COM object.
///
/// Contains a fixed pool of `IParamValueQueue` objects. Each block, the host
/// adds parameter changes using `add_change()`, then passes this object to
/// the plugin via `ProcessData::inputParameterChanges`.
#[repr(C)]
pub struct HostParameterChanges {
    /// Pointer to the static vtable.
    vtbl: *const IParameterChangesVtbl,
    /// Pool of value queues (pre-allocated, reused each block).
    queues: [HostParamValueQueue; MAX_PARAM_QUEUES],
    /// Number of active queues this block.
    queue_count: i32,
}

impl HostParameterChanges {
    /// Create a new heap-allocated parameter changes object.
    pub fn new() -> *mut Self {
        let mut obj = Box::new(Self {
            vtbl: &PARAMETER_CHANGES_VTBL,
            queues: unsafe { std::mem::zeroed() },
            queue_count: 0,
        });
        // Initialize vtable pointers for all queues
        for queue in &mut obj.queues {
            queue.vtbl = &PARAM_VALUE_QUEUE_VTBL;
        }
        Box::into_raw(obj)
    }

    /// Destroy a parameter changes object.
    ///
    /// # Safety
    /// The pointer must have been created by `HostParameterChanges::new()`.
    pub unsafe fn destroy(ptr: *mut Self) {
        if !ptr.is_null() {
            drop(unsafe { Box::from_raw(ptr) });
        }
    }

    /// Get this as a `*mut c_void` for setting on `ProcessData::inputParameterChanges`.
    pub fn as_ptr(ptr: *mut Self) -> *mut c_void {
        ptr as *mut c_void
    }

    /// Clear all queues (call between process blocks).
    ///
    /// # Safety
    /// The pointer must be valid.
    pub unsafe fn clear(ptr: *mut Self) {
        let this = unsafe { &mut *ptr };
        for i in 0..this.queue_count as usize {
            this.queues[i].clear();
        }
        this.queue_count = 0;
    }

    /// Add a parameter change for the current block.
    ///
    /// `sample_offset` is the sample position within the block (0 for "immediately").
    ///
    /// # Safety
    /// The pointer must be valid.
    pub unsafe fn add_change(
        ptr: *mut Self,
        param_id: u32,
        sample_offset: i32,
        value: f64,
    ) -> bool {
        let this = unsafe { &mut *ptr };

        // Find existing queue for this parameter
        for i in 0..this.queue_count as usize {
            if this.queues[i].param_id == param_id {
                return this.queues[i].add(sample_offset, value) >= 0;
            }
        }

        // Create new queue
        if (this.queue_count as usize) >= MAX_PARAM_QUEUES {
            return false;
        }

        let idx = this.queue_count as usize;
        this.queues[idx].init(param_id);
        this.queues[idx].add(sample_offset, value);
        this.queue_count += 1;
        true
    }

    /// Get the number of parameters with changes this block.
    ///
    /// # Safety
    /// The pointer must be valid.
    #[allow(dead_code)]
    pub unsafe fn change_count(ptr: *mut Self) -> i32 {
        let this = unsafe { &*ptr };
        this.queue_count
    }
}

// ─── IParameterChanges COM method implementations ─────────────────────────

unsafe extern "system" fn pc_query_interface(
    this: *mut c_void,
    iid: *const u8,
    obj: *mut *mut c_void,
) -> i32 {
    if iid.is_null() || obj.is_null() {
        return K_NOT_IMPLEMENTED;
    }

    let iid_bytes: [u8; 16] = unsafe { *(iid as *const [u8; 16]) };
    if iid_bytes == IPARAMETER_CHANGES_IID || iid_bytes == FUNKNOWN_IID {
        unsafe { *obj = this };
        return K_RESULT_OK;
    }

    unsafe { *obj = std::ptr::null_mut() };
    K_NOT_IMPLEMENTED
}

unsafe extern "system" fn pc_add_ref(_this: *mut c_void) -> u32 {
    1 // Managed by host
}

unsafe extern "system" fn pc_release(_this: *mut c_void) -> u32 {
    1
}

unsafe extern "system" fn pc_get_parameter_count(this: *mut c_void) -> i32 {
    let changes = this as *const HostParameterChanges;
    unsafe { (*changes).queue_count }
}

unsafe extern "system" fn pc_get_parameter_data(this: *mut c_void, index: i32) -> *mut c_void {
    let changes = this as *mut HostParameterChanges;
    unsafe {
        if index < 0 || index >= (*changes).queue_count {
            return std::ptr::null_mut();
        }
        &mut (*changes).queues[index as usize] as *mut HostParamValueQueue as *mut c_void
    }
}

unsafe extern "system" fn pc_add_parameter_data(
    this: *mut c_void,
    id: *const u32,
    index: *mut i32,
) -> *mut c_void {
    if id.is_null() {
        return std::ptr::null_mut();
    }

    let changes = this as *mut HostParameterChanges;
    let param_id = unsafe { *id };

    unsafe {
        // Check if queue already exists for this parameter
        for i in 0..(*changes).queue_count as usize {
            if (*changes).queues[i].param_id == param_id {
                if !index.is_null() {
                    *index = i as i32;
                }
                return &mut (*changes).queues[i] as *mut HostParamValueQueue as *mut c_void;
            }
        }

        // Create new queue
        if ((*changes).queue_count as usize) >= MAX_PARAM_QUEUES {
            return std::ptr::null_mut();
        }

        let idx = (*changes).queue_count as usize;
        (*changes).queues[idx].init(param_id);
        (*changes).queue_count += 1;

        if !index.is_null() {
            *index = idx as i32;
        }
        &mut (*changes).queues[idx] as *mut HostParamValueQueue as *mut c_void
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_destroy() {
        let changes = HostParameterChanges::new();
        assert!(!changes.is_null());
        unsafe { HostParameterChanges::destroy(changes) };
    }

    #[test]
    fn test_add_and_count_changes() {
        let changes = HostParameterChanges::new();
        unsafe {
            assert_eq!(HostParameterChanges::change_count(changes), 0);

            HostParameterChanges::add_change(changes, 100, 0, 0.5);
            assert_eq!(HostParameterChanges::change_count(changes), 1);

            // Same parameter, different value — should reuse queue
            HostParameterChanges::add_change(changes, 100, 128, 0.8);
            assert_eq!(HostParameterChanges::change_count(changes), 1);

            // Different parameter
            HostParameterChanges::add_change(changes, 200, 0, 1.0);
            assert_eq!(HostParameterChanges::change_count(changes), 2);

            HostParameterChanges::destroy(changes);
        }
    }

    #[test]
    fn test_clear() {
        let changes = HostParameterChanges::new();
        unsafe {
            HostParameterChanges::add_change(changes, 100, 0, 0.5);
            HostParameterChanges::add_change(changes, 200, 0, 0.7);
            assert_eq!(HostParameterChanges::change_count(changes), 2);

            HostParameterChanges::clear(changes);
            assert_eq!(HostParameterChanges::change_count(changes), 0);

            HostParameterChanges::destroy(changes);
        }
    }

    #[test]
    fn test_vtable_get_parameter_count() {
        let changes = HostParameterChanges::new();
        unsafe {
            HostParameterChanges::add_change(changes, 42, 0, 0.5);

            let vtbl = &*(*changes).vtbl;
            let count = (vtbl.get_parameter_count)(changes as *mut c_void);
            assert_eq!(count, 1);

            HostParameterChanges::destroy(changes);
        }
    }

    #[test]
    fn test_vtable_get_parameter_data() {
        let changes = HostParameterChanges::new();
        unsafe {
            HostParameterChanges::add_change(changes, 42, 0, 0.5);
            HostParameterChanges::add_change(changes, 42, 128, 0.9);

            let vtbl = &*(*changes).vtbl;
            let queue_ptr = (vtbl.get_parameter_data)(changes as *mut c_void, 0);
            assert!(!queue_ptr.is_null());

            // Read through the IParamValueQueue vtable
            let queue = queue_ptr as *const HostParamValueQueue;
            let q_vtbl = &*(*queue).vtbl;

            let param_id = (q_vtbl.get_parameter_id)(queue_ptr);
            assert_eq!(param_id, 42);

            let point_count = (q_vtbl.get_point_count)(queue_ptr);
            assert_eq!(point_count, 2);

            let mut offset = 0i32;
            let mut value = 0.0f64;
            let result = (q_vtbl.get_point)(queue_ptr, 0, &mut offset, &mut value);
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(offset, 0);
            assert!((value - 0.5).abs() < f64::EPSILON);

            let result = (q_vtbl.get_point)(queue_ptr, 1, &mut offset, &mut value);
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(offset, 128);
            assert!((value - 0.9).abs() < f64::EPSILON);

            HostParameterChanges::destroy(changes);
        }
    }

    #[test]
    fn test_vtable_add_parameter_data() {
        let changes = HostParameterChanges::new();
        unsafe {
            let vtbl = &*(*changes).vtbl;
            let param_id: u32 = 55;
            let mut idx: i32 = -1;

            let queue_ptr = (vtbl.add_parameter_data)(changes as *mut c_void, &param_id, &mut idx);
            assert!(!queue_ptr.is_null());
            assert_eq!(idx, 0);

            // Add a point through the queue vtable
            let queue = queue_ptr as *mut HostParamValueQueue;
            let q_vtbl = &*(*queue).vtbl;
            let mut point_idx: i32 = -1;
            let result = (q_vtbl.add_point)(queue_ptr, 64, 0.75, &mut point_idx);
            assert_eq!(result, K_RESULT_OK);
            assert_eq!(point_idx, 0);

            // Verify
            let count = (vtbl.get_parameter_count)(changes as *mut c_void);
            assert_eq!(count, 1);

            HostParameterChanges::destroy(changes);
        }
    }

    #[test]
    fn test_get_parameter_data_out_of_range() {
        let changes = HostParameterChanges::new();
        unsafe {
            let vtbl = &*(*changes).vtbl;
            let ptr = (vtbl.get_parameter_data)(changes as *mut c_void, 0);
            assert!(ptr.is_null());

            let ptr = (vtbl.get_parameter_data)(changes as *mut c_void, -1);
            assert!(ptr.is_null());

            HostParameterChanges::destroy(changes);
        }
    }

    #[test]
    fn test_query_interface_parameter_changes() {
        let changes = HostParameterChanges::new();
        unsafe {
            let vtbl = &*(*changes).vtbl;
            let mut obj: *mut c_void = std::ptr::null_mut();

            let result = (vtbl.query_interface)(
                changes as *mut c_void,
                IPARAMETER_CHANGES_IID.as_ptr(),
                &mut obj,
            );
            assert_eq!(result, K_RESULT_OK);

            let result =
                (vtbl.query_interface)(changes as *mut c_void, FUNKNOWN_IID.as_ptr(), &mut obj);
            assert_eq!(result, K_RESULT_OK);

            let result =
                (vtbl.query_interface)(changes as *mut c_void, ICOMPONENT_IID.as_ptr(), &mut obj);
            assert_ne!(result, K_RESULT_OK);

            HostParameterChanges::destroy(changes);
        }
    }

    #[test]
    fn test_max_param_queues_overflow() {
        let changes = HostParameterChanges::new();
        unsafe {
            // Fill all 64 queue slots with different parameters
            for i in 0..MAX_PARAM_QUEUES {
                let result = HostParameterChanges::add_change(changes, i as u32, 0, 0.5);
                assert!(result, "Should succeed for param queue {}", i);
            }
            assert_eq!(
                HostParameterChanges::change_count(changes),
                MAX_PARAM_QUEUES as i32
            );

            // One more unique parameter should fail
            let result = HostParameterChanges::add_change(changes, MAX_PARAM_QUEUES as u32, 0, 0.5);
            assert!(!result, "Should fail when MAX_PARAM_QUEUES exceeded");
            assert_eq!(
                HostParameterChanges::change_count(changes),
                MAX_PARAM_QUEUES as i32
            );

            HostParameterChanges::destroy(changes);
        }
    }

    #[test]
    fn test_max_points_per_param_overflow() {
        let changes = HostParameterChanges::new();
        unsafe {
            // Fill one parameter queue with MAX_POINTS_PER_PARAM points
            for i in 0..MAX_POINTS_PER_PARAM {
                let result = HostParameterChanges::add_change(changes, 42, i as i32, 0.5);
                assert!(result, "Should succeed for point {}", i);
            }

            // One more point should fail (queue full)
            let result =
                HostParameterChanges::add_change(changes, 42, MAX_POINTS_PER_PARAM as i32, 0.99);
            assert!(!result, "Should fail when MAX_POINTS_PER_PARAM exceeded");

            // But we can still add to a different parameter
            let result = HostParameterChanges::add_change(changes, 99, 0, 0.1);
            assert!(result, "Different param should still work");

            HostParameterChanges::destroy(changes);
        }
    }

    #[test]
    fn test_pvq_query_interface_unknown_iid() {
        let changes = HostParameterChanges::new();
        unsafe {
            HostParameterChanges::add_change(changes, 42, 0, 0.5);

            let vtbl = &*(*changes).vtbl;
            let queue_ptr = (vtbl.get_parameter_data)(changes as *mut c_void, 0);
            assert!(!queue_ptr.is_null());

            let queue = queue_ptr as *const HostParamValueQueue;
            let q_vtbl = &*(*queue).vtbl;

            let mut obj: *mut c_void = std::ptr::null_mut();

            // Should succeed for IPARAM_VALUE_QUEUE_IID
            let result =
                (q_vtbl.query_interface)(queue_ptr, IPARAM_VALUE_QUEUE_IID.as_ptr(), &mut obj);
            assert_eq!(result, K_RESULT_OK);

            // Should succeed for FUnknown
            let result = (q_vtbl.query_interface)(queue_ptr, FUNKNOWN_IID.as_ptr(), &mut obj);
            assert_eq!(result, K_RESULT_OK);

            // Should fail for an unrelated IID
            let result = (q_vtbl.query_interface)(queue_ptr, ICOMPONENT_IID.as_ptr(), &mut obj);
            assert_ne!(result, K_RESULT_OK);
            assert!(obj.is_null());

            HostParameterChanges::destroy(changes);
        }
    }

    #[test]
    fn test_pvq_get_point_out_of_range() {
        let changes = HostParameterChanges::new();
        unsafe {
            HostParameterChanges::add_change(changes, 42, 0, 0.5);

            let vtbl = &*(*changes).vtbl;
            let queue_ptr = (vtbl.get_parameter_data)(changes as *mut c_void, 0);
            let queue = queue_ptr as *const HostParamValueQueue;
            let q_vtbl = &*(*queue).vtbl;

            let mut offset = 0i32;
            let mut value = 0.0f64;

            // Index 1 is out of range (only index 0 exists)
            let result = (q_vtbl.get_point)(queue_ptr, 1, &mut offset, &mut value);
            assert_eq!(result, K_INVALID_ARGUMENT);

            // Negative index
            let result = (q_vtbl.get_point)(queue_ptr, -1, &mut offset, &mut value);
            assert_eq!(result, K_INVALID_ARGUMENT);

            HostParameterChanges::destroy(changes);
        }
    }

    #[test]
    fn test_pvq_get_point_null_pointers() {
        let changes = HostParameterChanges::new();
        unsafe {
            HostParameterChanges::add_change(changes, 42, 0, 0.5);

            let vtbl = &*(*changes).vtbl;
            let queue_ptr = (vtbl.get_parameter_data)(changes as *mut c_void, 0);
            let queue = queue_ptr as *const HostParamValueQueue;
            let q_vtbl = &*(*queue).vtbl;

            let mut offset = 0i32;
            let mut value = 0.0f64;

            // Null sample_offset
            let result = (q_vtbl.get_point)(queue_ptr, 0, std::ptr::null_mut(), &mut value);
            assert_eq!(result, K_INVALID_ARGUMENT);

            // Null value
            let result = (q_vtbl.get_point)(queue_ptr, 0, &mut offset, std::ptr::null_mut());
            assert_eq!(result, K_INVALID_ARGUMENT);

            HostParameterChanges::destroy(changes);
        }
    }

    #[test]
    fn test_vtable_add_parameter_data_existing() {
        let changes = HostParameterChanges::new();
        unsafe {
            let vtbl = &*(*changes).vtbl;
            let param_id: u32 = 55;
            let mut idx: i32 = -1;

            // Add first time
            let queue1 = (vtbl.add_parameter_data)(changes as *mut c_void, &param_id, &mut idx);
            assert!(!queue1.is_null());
            assert_eq!(idx, 0);

            // Add same param again — should return same queue
            let mut idx2: i32 = -1;
            let queue2 = (vtbl.add_parameter_data)(changes as *mut c_void, &param_id, &mut idx2);
            assert_eq!(queue1, queue2, "Should reuse existing queue");
            assert_eq!(idx2, 0);

            // Count should still be 1
            let count = (vtbl.get_parameter_count)(changes as *mut c_void);
            assert_eq!(count, 1);

            HostParameterChanges::destroy(changes);
        }
    }

    #[test]
    fn test_vtable_add_parameter_data_null_id() {
        let changes = HostParameterChanges::new();
        unsafe {
            let vtbl = &*(*changes).vtbl;
            let mut idx: i32 = -1;

            let queue =
                (vtbl.add_parameter_data)(changes as *mut c_void, std::ptr::null(), &mut idx);
            assert!(queue.is_null());

            HostParameterChanges::destroy(changes);
        }
    }

    #[test]
    fn test_as_ptr() {
        let changes = HostParameterChanges::new();
        let ptr = HostParameterChanges::as_ptr(changes);
        assert!(!ptr.is_null());
        assert_eq!(ptr, changes as *mut c_void);
        unsafe { HostParameterChanges::destroy(changes) };
    }
}
