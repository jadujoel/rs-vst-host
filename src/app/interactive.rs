//! Interactive command loop for runtime parameter control during audio processing.
//!
//! When the `run` command is active, this module reads stdin line-by-line and
//! dispatches commands to query/set parameters, change tempo, etc.

use crate::vst3::component_handler::HostComponentHandler;
use crate::vst3::params::ParameterRegistry;
use std::io::{self, BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// State shared between the interactive loop and the audio engine.
pub struct InteractiveState {
    /// Parameter registry for the plugin (if available).
    pub params: Option<ParameterRegistry>,
    /// Component handler for polling plugin-initiated changes.
    pub component_handler: *mut HostComponentHandler,
    /// Queue for sending parameter changes to the audio thread.
    pub param_queue: Arc<Mutex<Vec<(u32, f64)>>>,
    /// Running flag — set to false to stop the audio loop.
    pub running: Arc<AtomicBool>,
}

// Safety: component_handler is only read from the interactive thread,
// and the HostComponentHandler itself uses internal Mutex.
unsafe impl Send for InteractiveState {}

/// Run the interactive command loop on the current thread.
///
/// Reads commands from stdin and dispatches them. Returns when the user
/// types `quit` or when `state.running` becomes false.
pub fn run_interactive(state: &mut InteractiveState) {
    print_help();
    print_prompt();

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        if !state.running.load(Ordering::Relaxed) {
            break;
        }

        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            print_prompt();
            continue;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        let cmd = parts[0].to_lowercase();

        match cmd.as_str() {
            "help" | "h" | "?" => print_help(),
            "quit" | "q" | "exit" => {
                state.running.store(false, Ordering::Relaxed);
                break;
            }
            "params" | "p" => cmd_params(state),
            "get" | "g" => cmd_get(state, &parts[1..]),
            "set" | "s" => cmd_set(state, &parts[1..]),
            "tempo" | "t" => cmd_tempo(&parts[1..]),
            "status" => cmd_status(state),
            _ => {
                println!(
                    "Unknown command: '{}'. Type 'help' for available commands.",
                    cmd
                );
            }
        }

        if !state.running.load(Ordering::Relaxed) {
            break;
        }

        // Check for plugin-initiated parameter changes
        poll_handler_changes(state);

        print_prompt();
    }
}

fn print_prompt() {
    print!("> ");
    let _ = io::stdout().flush();
}

fn print_help() {
    println!("Interactive commands:");
    println!("  params, p              List all plugin parameters");
    println!("  get <id|name>          Get a parameter's current value");
    println!("  set <id|name> <value>  Set a parameter (0.0-1.0 normalized)");
    println!("  tempo <bpm>            Set tempo (display only, for plugins)");
    println!("  status                 Show current engine status");
    println!("  help, h, ?             Show this help");
    println!("  quit, q, exit          Stop and exit");
    println!();
}

fn cmd_params(state: &InteractiveState) {
    match &state.params {
        Some(params) => {
            println!("\nPlugin parameters ({}):\n", params.count());
            params.print_table();
            println!();
        }
        None => println!("No parameters available."),
    }
}

fn cmd_get(state: &InteractiveState, args: &[&str]) {
    if args.is_empty() {
        println!("Usage: get <id|name>");
        return;
    }

    let params = match &state.params {
        Some(p) => p,
        None => {
            println!("No parameters available.");
            return;
        }
    };

    let query = args[0];

    // Try parsing as parameter ID first
    let entry = if let Ok(id) = query.parse::<u32>() {
        params.get(id)
    } else {
        params.find_by_name(query)
    };

    match entry {
        Some(param) => {
            let display = params
                .value_to_string(param.id, param.current_normalized)
                .unwrap_or_else(|| format!("{:.4}", param.current_normalized));
            let units = if param.units.is_empty() {
                String::new()
            } else {
                format!(" {}", param.units)
            };
            println!(
                "  {} (ID {}): {}{} [normalized: {:.4}]",
                param.title, param.id, display, units, param.current_normalized
            );
        }
        None => println!("Parameter '{}' not found.", query),
    }
}

fn cmd_set(state: &mut InteractiveState, args: &[&str]) {
    if args.len() < 2 {
        println!("Usage: set <id|name> <value>");
        println!("  value: 0.0-1.0 (normalized)");
        return;
    }

    let params = match &mut state.params {
        Some(p) => p,
        None => {
            println!("No parameters available.");
            return;
        }
    };

    let query = args[0];
    let value: f64 = match args[1].parse() {
        Ok(v) => v,
        Err(_) => {
            println!("Invalid value: '{}'. Expected a number 0.0-1.0.", args[1]);
            return;
        }
    };

    if !(0.0..=1.0).contains(&value) {
        println!("Warning: value {:.4} is outside [0.0, 1.0] range.", value);
    }

    // Find the parameter
    let param_id = if let Ok(id) = query.parse::<u32>() {
        if params.get(id).is_some() {
            Some(id)
        } else {
            None
        }
    } else {
        params.find_by_name(query).map(|p| p.id)
    };

    let param_id = match param_id {
        Some(id) => id,
        None => {
            println!("Parameter '{}' not found.", query);
            return;
        }
    };

    // Set on the controller directly
    match params.set_normalized(param_id, value) {
        Ok(actual) => {
            // Also queue for the audio thread
            if let Ok(mut queue) = state.param_queue.lock() {
                queue.push((param_id, actual));
            }

            let entry = params.get(param_id).unwrap();
            let display = params
                .value_to_string(param_id, actual)
                .unwrap_or_else(|| format!("{:.4}", actual));
            println!(
                "  {} = {} [normalized: {:.4}]",
                entry.title, display, actual
            );
        }
        Err(e) => println!("Failed to set parameter: {}", e),
    }
}

fn cmd_tempo(args: &[&str]) {
    if args.is_empty() {
        println!("Usage: tempo <bpm>");
        return;
    }

    let bpm: f64 = match args[0].parse() {
        Ok(v) if v > 0.0 && v <= 999.0 => v,
        _ => {
            println!("Invalid tempo: '{}'. Expected 1-999 BPM.", args[0]);
            return;
        }
    };

    // Note: tempo changes are applied at the engine level but we don't
    // have direct access to the engine from here. We just report it.
    // A future enhancement could add a shared tempo value.
    println!(
        "  Tempo: {:.1} BPM (note: requires engine access to apply)",
        bpm
    );
}

fn cmd_status(state: &InteractiveState) {
    let param_count = state.params.as_ref().map(|p| p.count()).unwrap_or(0);
    let handler_status = if state.component_handler.is_null() {
        "not installed"
    } else {
        "active"
    };
    println!("  Parameters: {}", param_count);
    println!("  Component handler: {}", handler_status);
    println!("  Running: {}", state.running.load(Ordering::Relaxed));
}

/// Poll the component handler for plugin-initiated parameter changes.
fn poll_handler_changes(state: &InteractiveState) {
    if state.component_handler.is_null() {
        return;
    }

    unsafe {
        let changes = HostComponentHandler::drain_changes(state.component_handler);
        if !changes.is_empty() {
            for change in &changes {
                if let Some(params) = &state.params {
                    if let Some(param) = params.get(change.id) {
                        let display = params
                            .value_to_string(change.id, change.value)
                            .unwrap_or_else(|| format!("{:.4}", change.value));
                        println!(
                            "  [plugin] {} = {} [normalized: {:.4}]",
                            param.title, display, change.value
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interactive_state_creation() {
        let state = InteractiveState {
            params: None,
            component_handler: std::ptr::null_mut(),
            param_queue: Arc::new(Mutex::new(Vec::new())),
            running: Arc::new(AtomicBool::new(true)),
        };
        assert!(state.running.load(Ordering::Relaxed));
        assert!(state.params.is_none());
        assert!(state.component_handler.is_null());
    }

    #[test]
    fn test_poll_handler_changes_null_handler() {
        let state = InteractiveState {
            params: None,
            component_handler: std::ptr::null_mut(),
            param_queue: Arc::new(Mutex::new(Vec::new())),
            running: Arc::new(AtomicBool::new(true)),
        };
        // Should not panic with null handler
        poll_handler_changes(&state);
    }

    #[test]
    fn test_poll_handler_changes_with_handler() {
        let handler = HostComponentHandler::new();
        let state = InteractiveState {
            params: None,
            component_handler: handler,
            param_queue: Arc::new(Mutex::new(Vec::new())),
            running: Arc::new(AtomicBool::new(true)),
        };
        // Should not panic even without params
        poll_handler_changes(&state);
        unsafe { HostComponentHandler::destroy(handler) };
    }

    #[test]
    fn test_cmd_tempo_valid_parsing() {
        // Valid tempo should not panic (we can't easily capture stdout, but we can verify no panic)
        cmd_tempo(&["120"]);
        cmd_tempo(&["60.5"]);
        cmd_tempo(&["1"]);
        cmd_tempo(&["999"]);
    }

    #[test]
    fn test_cmd_tempo_invalid_parsing() {
        // Invalid tempos should not panic
        cmd_tempo(&["0"]);
        cmd_tempo(&["-1"]);
        cmd_tempo(&["1000"]);
        cmd_tempo(&["abc"]);
        cmd_tempo(&[]);
    }

    #[test]
    fn test_cmd_status_no_params() {
        let state = InteractiveState {
            params: None,
            component_handler: std::ptr::null_mut(),
            param_queue: Arc::new(Mutex::new(Vec::new())),
            running: Arc::new(AtomicBool::new(true)),
        };
        // Should not panic
        cmd_status(&state);
    }

    #[test]
    fn test_cmd_params_no_params() {
        let state = InteractiveState {
            params: None,
            component_handler: std::ptr::null_mut(),
            param_queue: Arc::new(Mutex::new(Vec::new())),
            running: Arc::new(AtomicBool::new(true)),
        };
        // Should print "No parameters available." without panicking
        cmd_params(&state);
    }

    #[test]
    fn test_cmd_get_no_args() {
        let state = InteractiveState {
            params: None,
            component_handler: std::ptr::null_mut(),
            param_queue: Arc::new(Mutex::new(Vec::new())),
            running: Arc::new(AtomicBool::new(true)),
        };
        // Should not panic
        cmd_get(&state, &[]);
    }

    #[test]
    fn test_cmd_get_no_params() {
        let state = InteractiveState {
            params: None,
            component_handler: std::ptr::null_mut(),
            param_queue: Arc::new(Mutex::new(Vec::new())),
            running: Arc::new(AtomicBool::new(true)),
        };
        // Should print "No parameters available." without panicking
        cmd_get(&state, &["42"]);
    }

    #[test]
    fn test_cmd_set_no_args() {
        let mut state = InteractiveState {
            params: None,
            component_handler: std::ptr::null_mut(),
            param_queue: Arc::new(Mutex::new(Vec::new())),
            running: Arc::new(AtomicBool::new(true)),
        };
        // Should print usage without panicking
        cmd_set(&mut state, &[]);
        cmd_set(&mut state, &["42"]); // Missing value
    }

    #[test]
    fn test_cmd_set_no_params() {
        let mut state = InteractiveState {
            params: None,
            component_handler: std::ptr::null_mut(),
            param_queue: Arc::new(Mutex::new(Vec::new())),
            running: Arc::new(AtomicBool::new(true)),
        };
        cmd_set(&mut state, &["42", "0.5"]);
    }

    #[test]
    fn test_cmd_set_invalid_value() {
        let mut state = InteractiveState {
            params: None,
            component_handler: std::ptr::null_mut(),
            param_queue: Arc::new(Mutex::new(Vec::new())),
            running: Arc::new(AtomicBool::new(true)),
        };
        cmd_set(&mut state, &["42", "abc"]); // Should not panic
    }

    #[test]
    fn test_poll_handler_with_pending_changes() {
        let handler = HostComponentHandler::new();
        unsafe {
            // Verify drain_changes works with an empty handler
            let changes = HostComponentHandler::drain_changes(handler);
            assert!(changes.is_empty());
        }

        let state = InteractiveState {
            params: None,
            component_handler: handler,
            param_queue: Arc::new(Mutex::new(Vec::new())),
            running: Arc::new(AtomicBool::new(true)),
        };

        // Should not panic
        poll_handler_changes(&state);

        unsafe { HostComponentHandler::destroy(handler) };
    }
}
