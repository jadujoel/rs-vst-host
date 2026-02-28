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
        /// Directories to scan for plugins. When provided, ONLY these paths are
        /// used (default system paths and persistent config paths are excluded).
        #[arg(short, long, num_args = 1..)]
        paths: Vec<PathBuf>,
    },
    /// Manage persistent plugin scan paths.
    ///
    /// Add or remove directories that are automatically included every time
    /// you run `scan`. Paths are stored in the config file and persist across runs.
    ScanPaths {
        #[command(subcommand)]
        action: ScanPathsAction,
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
        /// Directories to scan for plugins. When provided, ONLY these paths are
        /// used (default system paths and persistent config paths are excluded).
        #[arg(short, long, num_args = 1..)]
        paths: Vec<PathBuf>,

        /// Launch in safe mode with no plugins loaded from cache.
        #[arg(long)]
        safe_mode: bool,

        /// Enable malloc debug diagnostics: check heap integrity periodically,
        /// log malloc debug env var status, and print re-launch instructions
        /// if recommended vars aren't set.
        #[arg(long)]
        malloc_debug: bool,

        /// Run GUI in-process instead of a separate process (legacy mode).
        /// In-process mode shares memory with plugins, so a plugin crash
        /// can bring down the entire host. The default (separate process)
        /// is crash-resilient.
        #[arg(long)]
        in_process: bool,
    },
    /// Internal: run as a plugin worker process (used by process-per-plugin sandboxing).
    #[command(hide = true)]
    Worker {
        /// Path to the Unix domain socket for IPC with the host.
        #[arg(long)]
        socket: String,
    },
    /// Internal: run as the GUI worker process (used by GUI process separation).
    #[command(hide = true)]
    GuiWorker {
        /// Path to the Unix domain socket for IPC with the supervisor.
        #[arg(long)]
        socket: String,

        /// Launch in safe mode.
        #[arg(long)]
        safe_mode: bool,

        /// Enable malloc debug.
        #[arg(long)]
        malloc_debug: bool,
    },
    /// Internal: run as the audio worker process (used by supervisor process separation).
    #[command(hide = true)]
    AudioWorker {
        /// Path to the Unix domain socket for IPC with the supervisor.
        #[arg(long)]
        socket: String,

        /// Custom plugin scan paths (exclusive — skips defaults).
        #[arg(short, long, num_args = 1..)]
        paths: Vec<PathBuf>,

        /// Launch in safe mode.
        #[arg(long)]
        safe_mode: bool,

        /// Enable malloc debug.
        #[arg(long)]
        malloc_debug: bool,
    },
}

/// Actions for the `scan-paths` subcommand.
#[derive(Subcommand)]
pub enum ScanPathsAction {
    /// Add a directory to the persistent scan paths.
    Add {
        /// Directory to add to the scan path list.
        dir: PathBuf,
    },
    /// Remove a directory from the persistent scan paths.
    Remove {
        /// Directory to remove from the scan path list.
        dir: PathBuf,
    },
    /// List all persistent scan paths.
    List,
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
        assert!(matches!(
            cli.command,
            Command::Gui {
                safe_mode: false,
                malloc_debug: false,
                ..
            }
        ));
    }

    #[test]
    fn test_parse_gui_safe_mode() {
        let cli = Cli::try_parse_from(["rs-vst-host", "gui", "--safe-mode"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Gui {
                safe_mode: true,
                malloc_debug: false,
                ..
            }
        ));
    }

    #[test]
    fn test_parse_gui_malloc_debug() {
        let cli = Cli::try_parse_from(["rs-vst-host", "gui", "--malloc-debug"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Gui {
                safe_mode: false,
                malloc_debug: true,
                ..
            }
        ));
    }

    #[test]
    fn test_parse_gui_all_flags() {
        let cli =
            Cli::try_parse_from(["rs-vst-host", "gui", "--safe-mode", "--malloc-debug"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Gui {
                safe_mode: true,
                malloc_debug: true,
                ..
            }
        ));
    }

    #[test]
    fn test_parse_audio_worker() {
        let cli =
            Cli::try_parse_from(["rs-vst-host", "audio-worker", "--socket", "/tmp/audio.sock"])
                .unwrap();
        match cli.command {
            Command::AudioWorker {
                socket,
                paths,
                safe_mode,
                malloc_debug,
            } => {
                assert_eq!(socket, "/tmp/audio.sock");
                assert!(paths.is_empty());
                assert!(!safe_mode);
                assert!(!malloc_debug);
            }
            _ => panic!("Expected AudioWorker command"),
        }
    }

    #[test]
    fn test_parse_audio_worker_with_flags() {
        let cli = Cli::try_parse_from([
            "rs-vst-host",
            "audio-worker",
            "--socket",
            "/tmp/audio.sock",
            "--safe-mode",
            "--malloc-debug",
        ])
        .unwrap();
        match cli.command {
            Command::AudioWorker {
                socket,
                paths,
                safe_mode,
                malloc_debug,
            } => {
                assert_eq!(socket, "/tmp/audio.sock");
                assert!(paths.is_empty());
                assert!(safe_mode);
                assert!(malloc_debug);
            }
            _ => panic!("Expected AudioWorker command"),
        }
    }

    #[test]
    fn test_parse_scan_paths_add() {
        let cli =
            Cli::try_parse_from(["rs-vst-host", "scan-paths", "add", "/custom/vst3"]).unwrap();
        match cli.command {
            Command::ScanPaths { action } => match action {
                ScanPathsAction::Add { dir } => {
                    assert_eq!(dir, PathBuf::from("/custom/vst3"));
                }
                _ => panic!("Expected Add action"),
            },
            _ => panic!("Expected ScanPaths command"),
        }
    }

    #[test]
    fn test_parse_scan_paths_remove() {
        let cli =
            Cli::try_parse_from(["rs-vst-host", "scan-paths", "remove", "/custom/vst3"]).unwrap();
        match cli.command {
            Command::ScanPaths { action } => match action {
                ScanPathsAction::Remove { dir } => {
                    assert_eq!(dir, PathBuf::from("/custom/vst3"));
                }
                _ => panic!("Expected Remove action"),
            },
            _ => panic!("Expected ScanPaths command"),
        }
    }

    #[test]
    fn test_parse_scan_paths_list() {
        let cli = Cli::try_parse_from(["rs-vst-host", "scan-paths", "list"]).unwrap();
        match cli.command {
            Command::ScanPaths { action } => {
                assert!(matches!(action, ScanPathsAction::List));
            }
            _ => panic!("Expected ScanPaths command"),
        }
    }

    #[test]
    fn test_parse_scan_paths_add_missing_dir_fails() {
        let result = Cli::try_parse_from(["rs-vst-host", "scan-paths", "add"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_scan_paths_no_action_fails() {
        let result = Cli::try_parse_from(["rs-vst-host", "scan-paths"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_gui_with_paths() {
        let cli = Cli::try_parse_from([
            "rs-vst-host",
            "gui",
            "--paths",
            "/custom/vst3",
            "/another/path",
        ])
        .unwrap();
        match cli.command {
            Command::Gui { paths, .. } => {
                assert_eq!(paths.len(), 2);
                assert_eq!(paths[0], PathBuf::from("/custom/vst3"));
                assert_eq!(paths[1], PathBuf::from("/another/path"));
            }
            _ => panic!("Expected Gui command"),
        }
    }

    #[test]
    fn test_parse_gui_without_paths_has_empty_vec() {
        let cli = Cli::try_parse_from(["rs-vst-host", "gui"]).unwrap();
        match cli.command {
            Command::Gui { paths, .. } => {
                assert!(paths.is_empty());
            }
            _ => panic!("Expected Gui command"),
        }
    }

    #[test]
    fn test_parse_audio_worker_with_paths() {
        let cli = Cli::try_parse_from([
            "rs-vst-host",
            "audio-worker",
            "--socket",
            "/tmp/audio.sock",
            "--paths",
            "/custom/vst3",
        ])
        .unwrap();
        match cli.command {
            Command::AudioWorker {
                socket,
                paths,
                safe_mode,
                malloc_debug,
            } => {
                assert_eq!(socket, "/tmp/audio.sock");
                assert_eq!(paths.len(), 1);
                assert_eq!(paths[0], PathBuf::from("/custom/vst3"));
                assert!(!safe_mode);
                assert!(!malloc_debug);
            }
            _ => panic!("Expected AudioWorker command"),
        }
    }

    #[test]
    fn test_parse_scan_exclusive_paths() {
        // When --paths is provided, it should be the only paths used
        let cli = Cli::try_parse_from(["rs-vst-host", "scan", "--paths", "./vsts", "/other/dir"])
            .unwrap();
        match cli.command {
            Command::Scan { paths } => {
                assert_eq!(paths.len(), 2);
                assert_eq!(paths[0], PathBuf::from("./vsts"));
                assert_eq!(paths[1], PathBuf::from("/other/dir"));
            }
            _ => panic!("Expected Scan command"),
        }
    }
}
