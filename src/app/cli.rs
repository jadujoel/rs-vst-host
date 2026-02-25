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

        /// Sample rate in Hz (uses device default if not specified).
        #[arg(short, long)]
        sample_rate: Option<u32>,

        /// Buffer size in frames (uses device default if not specified).
        #[arg(short, long)]
        buffer_size: Option<u32>,

        /// Disable the test tone input signal.
        #[arg(long)]
        no_tone: bool,
    },
    /// List available audio output devices.
    Devices,
}
