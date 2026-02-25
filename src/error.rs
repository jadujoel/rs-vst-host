use thiserror::Error;

/// Top-level host error type.
#[derive(Error, Debug)]
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
}
