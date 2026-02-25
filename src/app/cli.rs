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
        /// Plugin name or path to .vst3 bundle.
        plugin: String,
    },
}
