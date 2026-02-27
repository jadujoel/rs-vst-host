// Library crate — exposes modules for testing (including Miri dynamic analysis).
//
// The binary crate (main.rs) re-declares the same modules for the actual
// executable.  This lib.rs exists primarily so that `cargo miri test --lib`
// can exercise the unit tests embedded in each module without needing to
// compile the binary entry-point (which pulls in system-level allocator
// setup and other FFI that Miri cannot interpret).

pub mod app;
pub mod audio;
pub mod diagnostics;
pub mod error;
pub mod gui;
pub mod host;
pub mod ipc;
pub mod midi;
#[cfg(test)]
mod miri_tests;
pub mod vst3;
