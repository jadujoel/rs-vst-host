mod app;
mod audio;
mod error;
mod gui;
mod host;
mod midi;
mod vst3;

use app::cli::{Cli, Command};
use clap::Parser;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Scan { paths } => app::commands::scan(paths)?,
        Command::List => app::commands::list()?,
        Command::Run {
            plugin,
            device,
            midi,
            sample_rate,
            buffer_size,
            no_tone,
            list_params,
        } => app::commands::run(
            &plugin,
            device.as_deref(),
            midi.as_deref(),
            sample_rate,
            buffer_size,
            no_tone,
            list_params,
        )?,
        Command::Devices => app::commands::devices()?,
        Command::MidiPorts => app::commands::midi_ports()?,
        Command::Gui => gui::launch()?,
    }

    Ok(())
}
