//! Process-per-plugin sandboxing via IPC.
//!
//! Each VST3 plugin runs in its own child process, providing true memory
//! isolation, crash safety, and independent CPU scheduling. Communication
//! between the host and plugin processes uses:
//!
//! - **Shared memory** for audio buffers (zero-copy, lowest latency)
//! - **Unix domain sockets** for control messages (load, configure, shutdown)
//! - **POSIX named semaphores** for audio block synchronization
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────┐         ┌──────────────────────┐
//! │   Host Process   │         │   Plugin Process      │
//! │                  │         │                       │
//! │  PluginProcess   │◄─shm──►│  PluginWorker         │
//! │  (proxy engine)  │         │  (Vst3Instance +      │
//! │                  │◄─sock──►│   AudioEngine)        │
//! │  cpal callback   │         │                       │
//! │  calls process() │◄─sem──►│  waits for process    │
//! └─────────────────┘         └──────────────────────┘
//! ```
//!
//! # Modules
//!
//! - [`messages`] — Serializable IPC command/response protocol
//! - [`shm`] — POSIX shared memory audio buffer management
//! - [`worker`] — Child process entry point and plugin lifecycle
//! - [`proxy`] — Host-side proxy that communicates with the child process

pub mod messages;
pub mod proxy;
pub mod shm;
pub mod worker;
