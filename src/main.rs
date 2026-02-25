mod app;
mod audio;
mod error;
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
        Command::Run { plugin } => app::commands::run(&plugin)?,
    }

    Ok(())
}
