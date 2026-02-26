mod app;
mod audio;
pub mod diagnostics;
mod error;
mod gui;
mod host;
mod midi;
mod vst3;

use app::cli::{Cli, Command};
use clap::Parser;

// ── dhat global allocator (behind feature flag) ─────────────────────────────
//
// When `debug-alloc` is enabled, dhat replaces the system allocator to
// profile all heap allocations. On exit, writes `dhat-heap.json` showing
// which allocation sites are hit (including those after crash recovery).
// We keep the system malloc for non-debug builds to reproduce the corruption.

#[cfg(feature = "debug-alloc")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() -> anyhow::Result<()> {
    // ── dhat profiler guard ─────────────────────────────────────────────
    #[cfg(feature = "debug-alloc")]
    let _profiler = diagnostics::init_profiler();

    // ── Structured tracing with layered Registry ────────────────────────
    //
    // Uses tracing-subscriber's Registry pattern for composable layers:
    // - Always: fmt layer with env-filter for log output
    // - Optional: tracing-chrome layer for Chrome trace timeline (debug-trace feature)
    init_tracing();

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
        Command::Gui {
            safe_mode,
            malloc_debug,
        } => {
            if malloc_debug {
                diagnostics::print_malloc_debug_instructions();
            }
            gui::launch(safe_mode, malloc_debug)?;
        }
    }

    Ok(())
}

/// Initialize the tracing subscriber with layered Registry pattern.
///
/// - Always: `fmt` layer with `RUST_LOG` env-filter
/// - With `debug-trace` feature: `tracing-chrome` layer producing
///   `trace-{timestamp}.json` viewable in `chrome://tracing` or Perfetto UI
fn init_tracing() {
    use tracing_subscriber::{Layer, Registry, layer::SubscriberExt, util::SubscriberInitExt};

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_ids(true);

    let env_filter = tracing_subscriber::EnvFilter::from_default_env();

    #[cfg(feature = "debug-trace")]
    {
        let (chrome_layer, _guard) = tracing_chrome::ChromeLayerBuilder::new()
            .include_args(true)
            .build();

        // Leak the guard so the trace file is written on process exit.
        // This is intentional — the guard must live for the entire process.
        std::mem::forget(_guard);

        Registry::default()
            .with(fmt_layer.with_filter(env_filter))
            .with(chrome_layer)
            .init();
    }

    #[cfg(not(feature = "debug-trace"))]
    {
        Registry::default()
            .with(fmt_layer.with_filter(env_filter))
            .init();
    }
}
