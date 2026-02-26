use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// A minimal VST3 host in Rust.
#[derive(Parser)]
#[command(name = "rs-vst-host", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Scan for VST3 plugins and cache metadata.
    Scan {
        /// Additional directories to scan for plugins.
        #[arg(short, long)]
        paths: Vec<PathBuf>,
    },
    /// List discovered plugins from cache.
    List,
    /// Load and run a plugin with audio processing.
    Run {
        /// Plugin name (as shown in `list`) or path to a .vst3 bundle.
        plugin: String,

        /// Audio output device name (uses default if not specified).
        #[arg(short, long)]
        device: Option<String>,

        /// MIDI input port name (no MIDI if not specified).
        #[arg(short, long)]
        midi: Option<String>,

        /// Sample rate in Hz (uses device default if not specified).
        #[arg(short, long)]
        sample_rate: Option<u32>,

        /// Buffer size in frames (uses device default if not specified).
        #[arg(short = 'B', long)]
        buffer_size: Option<u32>,

        /// Disable the test tone input signal.
        #[arg(long)]
        no_tone: bool,

        /// List plugin parameters after loading.
        #[arg(long)]
        list_params: bool,
    },
    /// List available audio output devices.
    Devices,
    /// List available MIDI input ports.
    MidiPorts,
    /// Launch the graphical user interface.
    Gui {
        /// Launch in safe mode with no plugins loaded from cache.
        #[arg(long)]
        safe_mode: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_parse_scan() {
        let cli = Cli::try_parse_from(["rs-vst-host", "scan"]).unwrap();
        match cli.command {
            Command::Scan { paths } => assert!(paths.is_empty()),
            _ => panic!("Expected Scan command"),
        }
    }

    #[test]
    fn test_parse_scan_with_paths() {
        let cli = Cli::try_parse_from(["rs-vst-host", "scan", "--paths", "/custom/vst3"]).unwrap();
        match cli.command {
            Command::Scan { paths } => {
                assert_eq!(paths.len(), 1);
                assert_eq!(paths[0], PathBuf::from("/custom/vst3"));
            }
            _ => panic!("Expected Scan command"),
        }
    }

    #[test]
    fn test_parse_list() {
        let cli = Cli::try_parse_from(["rs-vst-host", "list"]).unwrap();
        matches!(cli.command, Command::List);
    }

    #[test]
    fn test_parse_devices() {
        let cli = Cli::try_parse_from(["rs-vst-host", "devices"]).unwrap();
        matches!(cli.command, Command::Devices);
    }

    #[test]
    fn test_parse_midi_ports() {
        let cli = Cli::try_parse_from(["rs-vst-host", "midi-ports"]).unwrap();
        matches!(cli.command, Command::MidiPorts);
    }

    #[test]
    fn test_parse_run_minimal() {
        let cli = Cli::try_parse_from(["rs-vst-host", "run", "MyPlugin"]).unwrap();
        match cli.command {
            Command::Run {
                plugin,
                device,
                midi,
                sample_rate,
                buffer_size,
                no_tone,
                list_params,
            } => {
                assert_eq!(plugin, "MyPlugin");
                assert!(device.is_none());
                assert!(midi.is_none());
                assert!(sample_rate.is_none());
                assert!(buffer_size.is_none());
                assert!(!no_tone);
                assert!(!list_params);
            }
            _ => panic!("Expected Run command"),
        }
    }

    #[test]
    fn test_parse_run_all_options() {
        let cli = Cli::try_parse_from([
            "rs-vst-host",
            "run",
            "MyPlugin",
            "--device",
            "Speaker",
            "--midi",
            "Keyboard",
            "--sample-rate",
            "48000",
            "-B",
            "256",
            "--no-tone",
            "--list-params",
        ])
        .unwrap();
        match cli.command {
            Command::Run {
                plugin,
                device,
                midi,
                sample_rate,
                buffer_size,
                no_tone,
                list_params,
            } => {
                assert_eq!(plugin, "MyPlugin");
                assert_eq!(device.as_deref(), Some("Speaker"));
                assert_eq!(midi.as_deref(), Some("Keyboard"));
                assert_eq!(sample_rate, Some(48000));
                assert_eq!(buffer_size, Some(256));
                assert!(no_tone);
                assert!(list_params);
            }
            _ => panic!("Expected Run command"),
        }
    }

    #[test]
    fn test_parse_run_missing_plugin_fails() {
        let result = Cli::try_parse_from(["rs-vst-host", "run"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_unknown_subcommand_fails() {
        let result = Cli::try_parse_from(["rs-vst-host", "foobar"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_no_subcommand_fails() {
        let result = Cli::try_parse_from(["rs-vst-host"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_run_buffer_size_short_flag() {
        let cli = Cli::try_parse_from(["rs-vst-host", "run", "P", "-B", "1024"]).unwrap();
        match cli.command {
            Command::Run { buffer_size, .. } => assert_eq!(buffer_size, Some(1024)),
            _ => panic!("Expected Run command"),
        }
    }

    #[test]
    fn test_parse_gui() {
        let cli = Cli::try_parse_from(["rs-vst-host", "gui"]).unwrap();
        assert!(matches!(cli.command, Command::Gui { safe_mode: false }));
    }

    #[test]
    fn test_parse_gui_safe_mode() {
        let cli = Cli::try_parse_from(["rs-vst-host", "gui", "--safe-mode"]).unwrap();
        assert!(matches!(cli.command, Command::Gui { safe_mode: true }));
    }
}
