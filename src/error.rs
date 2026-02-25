use thiserror::Error;

/// Top-level host error type.
#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum HostError {
    #[error("VST3 error: {0}")]
    Vst3(#[from] Vst3Error),

    #[error("Audio error: {0}")]
    Audio(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

/// VST3-specific errors.
#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum Vst3Error {
    #[error("Failed to load plugin module: {0}")]
    ModuleLoad(String),

    #[error("Plugin entry point not found: {0}")]
    EntryPoint(String),

    #[error("Factory error: {0}")]
    Factory(String),

    #[error("Plugin not found: {0}")]
    NotFound(String),

    #[error("Bundle error: {path}: {message}")]
    Bundle { path: String, message: String },

    #[error("Processing error: {0}")]
    Processing(String),
}

/// Audio subsystem errors.
#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum AudioError {
    #[error("No audio output device available")]
    NoDevice,

    #[error("Audio device error: {0}")]
    Device(String),

    #[error("Audio stream error: {0}")]
    Stream(String),
}
