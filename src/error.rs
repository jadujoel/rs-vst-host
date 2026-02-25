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

/// MIDI subsystem errors.
#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum MidiError {
    #[error("No MIDI input port available")]
    NoPort,

    #[error("MIDI error: {0}")]
    Device(String),

    #[error("MIDI connection error: {0}")]
    Connection(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── HostError Display tests ──────────────────────────────────────

    #[test]
    fn test_host_error_vst3_display() {
        let err = HostError::Vst3(Vst3Error::NotFound("MyPlugin".into()));
        assert_eq!(err.to_string(), "VST3 error: Plugin not found: MyPlugin");
    }

    #[test]
    fn test_host_error_audio_display() {
        let err = HostError::Audio("device unavailable".into());
        assert_eq!(err.to_string(), "Audio error: device unavailable");
    }

    #[test]
    fn test_host_error_io_display() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let err = HostError::Io(io_err);
        assert!(err.to_string().starts_with("IO error:"));
        assert!(err.to_string().contains("file missing"));
    }

    #[test]
    fn test_host_error_serde_display() {
        let json_err = serde_json::from_str::<String>("not json").unwrap_err();
        let err = HostError::Serde(json_err);
        assert!(err.to_string().starts_with("Serialization error:"));
    }

    // ── Vst3Error Display tests ──────────────────────────────────────

    #[test]
    fn test_vst3_error_module_load() {
        let err = Vst3Error::ModuleLoad("libfoo.dylib".into());
        assert_eq!(
            err.to_string(),
            "Failed to load plugin module: libfoo.dylib"
        );
    }

    #[test]
    fn test_vst3_error_entry_point() {
        let err = Vst3Error::EntryPoint("bundleEntry".into());
        assert_eq!(err.to_string(), "Plugin entry point not found: bundleEntry");
    }

    #[test]
    fn test_vst3_error_factory() {
        let err = Vst3Error::Factory("getFactory returned null".into());
        assert_eq!(err.to_string(), "Factory error: getFactory returned null");
    }

    #[test]
    fn test_vst3_error_not_found() {
        let err = Vst3Error::NotFound("FooSynth".into());
        assert_eq!(err.to_string(), "Plugin not found: FooSynth");
    }

    #[test]
    fn test_vst3_error_bundle() {
        let err = Vst3Error::Bundle {
            path: "/Library/Audio/Plug-Ins/VST3/Foo.vst3".into(),
            message: "missing binary".into(),
        };
        assert_eq!(
            err.to_string(),
            "Bundle error: /Library/Audio/Plug-Ins/VST3/Foo.vst3: missing binary"
        );
    }

    #[test]
    fn test_vst3_error_processing() {
        let err = Vst3Error::Processing("process() returned -1".into());
        assert_eq!(err.to_string(), "Processing error: process() returned -1");
    }

    // ── AudioError Display tests ─────────────────────────────────────

    #[test]
    fn test_audio_error_no_device() {
        let err = AudioError::NoDevice;
        assert_eq!(err.to_string(), "No audio output device available");
    }

    #[test]
    fn test_audio_error_device() {
        let err = AudioError::Device("buffer underrun".into());
        assert_eq!(err.to_string(), "Audio device error: buffer underrun");
    }

    #[test]
    fn test_audio_error_stream() {
        let err = AudioError::Stream("failed to open".into());
        assert_eq!(err.to_string(), "Audio stream error: failed to open");
    }

    // ── MidiError Display tests ──────────────────────────────────────

    #[test]
    fn test_midi_error_no_port() {
        let err = MidiError::NoPort;
        assert_eq!(err.to_string(), "No MIDI input port available");
    }

    #[test]
    fn test_midi_error_device() {
        let err = MidiError::Device("init failed".into());
        assert_eq!(err.to_string(), "MIDI error: init failed");
    }

    #[test]
    fn test_midi_error_connection() {
        let err = MidiError::Connection("port busy".into());
        assert_eq!(err.to_string(), "MIDI connection error: port busy");
    }

    // ── From conversions ─────────────────────────────────────────────

    #[test]
    fn test_host_error_from_vst3_error() {
        let vst3_err = Vst3Error::NotFound("test".into());
        let host_err: HostError = vst3_err.into();
        match host_err {
            HostError::Vst3(_) => {} // expected
            _ => panic!("Expected HostError::Vst3"),
        }
    }

    #[test]
    fn test_host_error_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let host_err: HostError = io_err.into();
        match host_err {
            HostError::Io(_) => {} // expected
            _ => panic!("Expected HostError::Io"),
        }
    }

    #[test]
    fn test_host_error_from_serde_error() {
        let serde_err = serde_json::from_str::<String>("{{bad").unwrap_err();
        let host_err: HostError = serde_err.into();
        match host_err {
            HostError::Serde(_) => {} // expected
            _ => panic!("Expected HostError::Serde"),
        }
    }

    // ── Debug formatting ─────────────────────────────────────────────

    #[test]
    fn test_error_types_implement_debug() {
        let err = HostError::Audio("test".into());
        let debug = format!("{:?}", err);
        assert!(debug.contains("Audio"));

        let err = Vst3Error::Factory("test".into());
        let debug = format!("{:?}", err);
        assert!(debug.contains("Factory"));

        let err = AudioError::NoDevice;
        let debug = format!("{:?}", err);
        assert!(debug.contains("NoDevice"));

        let err = MidiError::NoPort;
        let debug = format!("{:?}", err);
        assert!(debug.contains("NoPort"));
    }
}
