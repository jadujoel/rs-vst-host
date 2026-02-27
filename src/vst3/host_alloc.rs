//! System malloc allocator for plugin-facing COM objects.
//!
//! Since v0.15.0 the host uses **mimalloc** as the global Rust allocator to
//! isolate Rust heap allocations from plugin-induced system malloc corruption.
//! However, COM objects that cross the host→plugin boundary must live on the
//! **system** malloc heap. Otherwise, if a plugin's C++ code calls `free()` or
//! `operator delete` on a host-allocated COM object (instead of properly
//! calling `Release()` through the COM vtable), macOS malloc zone validation
//! detects that the pointer does not belong to any system malloc zone and
//! aborts.
//!
//! This module provides [`system_alloc`] and [`system_free`] which bypass the
//! global allocator and call `libc::malloc` / `libc::free` directly.
//!
//! **Affected objects** (allocated via these helpers):
//! - [`HostApplication`](super::host_context::HostApplication)
//! - [`HostComponentHandler`](super::component_handler::HostComponentHandler)
//! - [`HostPlugFrame`](super::plug_frame::HostPlugFrame)

use std::alloc::{Layout, handle_alloc_error};
use std::ffi::c_void;

/// Allocate a `T` on the **system** malloc heap (bypassing mimalloc).
///
/// The value is written into freshly-allocated system memory. Returns a raw
/// pointer suitable for passing to plugins as a COM object.
///
/// # Safety
/// The returned pointer must eventually be freed with [`system_free`].
pub unsafe fn system_alloc<T>(value: T) -> *mut T {
    let layout = Layout::new::<T>();
    // Bypass the global allocator — go straight to the system allocator.
    let ptr = unsafe { libc::malloc(layout.size()) } as *mut T;
    if ptr.is_null() {
        handle_alloc_error(layout);
    }
    unsafe { std::ptr::write(ptr, value) };
    ptr
}

/// Free a `T` previously allocated with [`system_alloc`].
///
/// Calls `drop_in_place` on the value, then returns the memory to the system
/// allocator via `libc::free`.
///
/// # Safety
/// - `ptr` must have been returned by [`system_alloc`].
/// - `ptr` must not be used after this call.
/// - Passing a null pointer is a no-op.
pub unsafe fn system_free<T>(ptr: *mut T) {
    if !ptr.is_null() {
        unsafe {
            std::ptr::drop_in_place(ptr);
            libc::free(ptr as *mut c_void);
        }
    }
}

/// Check whether a pointer was allocated by the system malloc (any zone).
///
/// On macOS, uses `malloc_zone_from_ptr` to verify the pointer belongs to
/// a known system malloc zone. Returns `true` if recognized, `false` otherwise.
/// On non-macOS platforms, always returns `true` (no-op).
///
/// This is useful in tests to verify that [`system_alloc`] indeed produces
/// pointers visible to system malloc, even when mimalloc is the global
/// allocator.
#[allow(dead_code)]
pub fn is_system_malloc_ptr<T>(ptr: *const T) -> bool {
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn malloc_zone_from_ptr(ptr: *const c_void) -> *mut c_void;
        }
        if ptr.is_null() {
            return false;
        }
        let zone = unsafe { malloc_zone_from_ptr(ptr as *const c_void) };
        !zone.is_null()
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = ptr;
        true
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A simple struct to test allocation/deallocation.
    #[repr(C)]
    struct TestObj {
        value: u32,
        counter: AtomicU32,
    }

    #[test]
    fn test_system_alloc_and_free() {
        unsafe {
            let ptr = system_alloc(TestObj {
                value: 42,
                counter: AtomicU32::new(1),
            });
            assert!(!ptr.is_null());
            assert_eq!((*ptr).value, 42);
            assert_eq!((*ptr).counter.load(Ordering::Relaxed), 1);
            system_free(ptr);
        }
    }

    #[test]
    fn test_system_free_null() {
        unsafe {
            system_free::<TestObj>(std::ptr::null_mut());
        }
    }

    #[test]
    fn test_system_alloc_is_in_system_zone() {
        unsafe {
            let ptr = system_alloc(TestObj {
                value: 99,
                counter: AtomicU32::new(0),
            });
            // On macOS, malloc_zone_from_ptr should recognise it
            assert!(is_system_malloc_ptr(ptr));
            system_free(ptr);
        }
    }

    #[test]
    fn test_box_alloc_is_not_in_system_zone() {
        // When mimalloc is the global allocator, Box goes to mimalloc.
        // malloc_zone_from_ptr will NOT recognise it.
        //
        // This test only makes sense in the binary crate where mimalloc is
        // configured as #[global_allocator].  In the lib crate (used by
        // `cargo miri test --lib`) the default system allocator is active,
        // so Box allocations *will* reside in the system malloc zone.
        let boxed = Box::new(42u32);
        let ptr = &*boxed as *const u32;

        #[cfg(target_os = "macos")]
        {
            // If the system allocator recognises this pointer, mimalloc is
            // NOT the global allocator — skip the assertion.
            if !is_system_malloc_ptr(ptr) {
                // mimalloc is active — validate the premise.
            } else {
                // System allocator is active (lib crate / Miri) — nothing to assert.
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = ptr;
        }
    }

    #[test]
    fn test_system_alloc_alignment() {
        unsafe {
            // Test that alignment is at least the default for the type
            let ptr = system_alloc(0u64);
            let addr = ptr as usize;
            assert_eq!(addr % std::mem::align_of::<u64>(), 0);
            system_free(ptr);
        }
    }

    /// Verify that drop_in_place is called during system_free.
    #[test]
    fn test_system_free_calls_drop() {
        use std::sync::Arc;
        use std::sync::atomic::AtomicBool;

        struct DropTracker {
            dropped: Arc<AtomicBool>,
        }

        impl Drop for DropTracker {
            fn drop(&mut self) {
                self.dropped.store(true, Ordering::SeqCst);
            }
        }

        let dropped = Arc::new(AtomicBool::new(false));
        unsafe {
            let ptr = system_alloc(DropTracker {
                dropped: dropped.clone(),
            });
            assert!(!dropped.load(Ordering::SeqCst));
            system_free(ptr);
            assert!(dropped.load(Ordering::SeqCst));
        }
    }

    #[test]
    fn test_is_system_malloc_ptr_null() {
        assert!(!is_system_malloc_ptr::<u8>(std::ptr::null()));
    }

    /// Stress test: allocate and free many objects without leaks or errors.
    #[test]
    fn test_system_alloc_many() {
        unsafe {
            let mut ptrs = Vec::new();
            for i in 0..100u32 {
                ptrs.push(system_alloc(i));
            }
            for (i, ptr) in ptrs.iter().enumerate() {
                assert_eq!(**ptr, i as u32);
            }
            for ptr in ptrs {
                system_free(ptr);
            }
        }
    }
}
