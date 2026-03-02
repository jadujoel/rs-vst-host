//! Undo/redo system — command pattern with parameter coalescing.
//!
//! Provides a reversible history of user actions (parameter changes,
//! rack modifications, transport edits, preset loads). Consecutive
//! parameter changes on the same parameter within a coalescing window
//! (default 500ms) are merged into a single undo entry.
//!
//! # Architecture
//!
//! ```text
//! User Action → UndoableAction → UndoStack
//!                                  ├── undo_stack: Vec<UndoableAction>
//!                                  └── redo_stack: Vec<UndoableAction>
//! ```
//!
//! The `UndoStack` stores concrete `UndoableAction` enum variants rather
//! than trait objects, avoiding dynamic dispatch and simplifying
//! serialization. Each variant carries the minimal data needed to
//! undo/redo the action.

use std::path::PathBuf;
use std::time::Instant;

// ── Action Types ────────────────────────────────────────────────────────

/// A reversible user action stored in the undo stack.
///
/// Each variant carries the data required to undo *and* redo the action.
/// Field naming convention: `old_*` = value before the action,
/// `new_*` = value after the action.
#[derive(Debug, Clone)]
pub enum UndoableAction {
    /// A parameter value change on the active plugin.
    SetParameter {
        /// Rack slot index.
        slot_index: usize,
        /// VST3 parameter ID.
        param_id: u32,
        /// Value before the change.
        old_value: f64,
        /// Value after the change.
        new_value: f64,
        /// Human-readable parameter name (for display in history).
        param_name: String,
    },

    /// A plugin was added to the rack.
    AddPlugin {
        /// Rack slot index where the plugin was added.
        slot_index: usize,
        /// Plugin display name.
        name: String,
        /// Plugin vendor.
        vendor: String,
        /// Plugin category.
        category: String,
        /// Path to the .vst3 bundle.
        path: PathBuf,
        /// Class ID.
        cid: [u8; 16],
    },

    /// A plugin was removed from the rack.
    RemovePlugin {
        /// Rack slot index where the plugin was removed.
        slot_index: usize,
        /// Plugin display name.
        name: String,
        /// Plugin vendor.
        vendor: String,
        /// Plugin category.
        category: String,
        /// Path to the .vst3 bundle.
        path: PathBuf,
        /// Class ID.
        cid: [u8; 16],
        /// Cached parameter state (for restoring on undo).
        param_cache: Vec<super::backend::ParamSnapshot>,
        /// Component state blob.
        component_state: Option<Vec<u8>>,
        /// Controller state blob.
        controller_state: Option<Vec<u8>>,
    },

    /// A plugin was reordered in the rack (drag-and-drop).
    ReorderPlugin {
        /// Original index before the move.
        old_index: usize,
        /// New index after the move.
        new_index: usize,
    },

    /// A preset was loaded on a plugin.
    LoadPreset {
        /// Rack slot index.
        slot_index: usize,
        /// Preset file path.
        preset_path: PathBuf,
        /// Preset name.
        preset_name: String,
        /// Component state before loading the preset.
        old_component_state: Option<Vec<u8>>,
        /// Controller state before loading the preset.
        old_controller_state: Option<Vec<u8>>,
        /// Component state after loading the preset.
        new_component_state: Option<Vec<u8>>,
        /// Controller state after loading the preset.
        new_controller_state: Option<Vec<u8>>,
    },

    /// The tempo was changed.
    SetTempo {
        /// BPM before the change.
        old_bpm: f64,
        /// BPM after the change.
        new_bpm: f64,
    },

    /// The time signature was changed.
    SetTimeSignature {
        /// Old numerator.
        old_numerator: u32,
        /// Old denominator.
        old_denominator: u32,
        /// New numerator.
        new_numerator: u32,
        /// New denominator.
        new_denominator: u32,
    },
}

impl UndoableAction {
    /// Human-readable description for display in the undo history.
    pub fn description(&self) -> String {
        match self {
            Self::SetParameter {
                param_name,
                new_value,
                ..
            } => format!("Set {} → {:.3}", param_name, new_value),

            Self::AddPlugin { name, .. } => format!("Add '{}'", name),

            Self::RemovePlugin { name, .. } => format!("Remove '{}'", name),

            Self::ReorderPlugin {
                old_index,
                new_index,
            } => format!("Move slot {} → {}", old_index, new_index),

            Self::LoadPreset { preset_name, .. } => {
                format!("Load preset '{}'", preset_name)
            }

            Self::SetTempo { new_bpm, .. } => format!("Set tempo → {:.1} BPM", new_bpm),

            Self::SetTimeSignature {
                new_numerator,
                new_denominator,
                ..
            } => format!("Set time sig → {}/{}", new_numerator, new_denominator),
        }
    }

    /// Create the inverse action (for undo).
    ///
    /// Returns a new `UndoableAction` that reverses this one.
    #[allow(dead_code)]
    pub fn inverse(&self) -> Self {
        match self {
            Self::SetParameter {
                slot_index,
                param_id,
                old_value,
                new_value,
                param_name,
            } => Self::SetParameter {
                slot_index: *slot_index,
                param_id: *param_id,
                old_value: *new_value,
                new_value: *old_value,
                param_name: param_name.clone(),
            },

            Self::AddPlugin {
                slot_index,
                name,
                vendor,
                category,
                path,
                cid,
            } => Self::RemovePlugin {
                slot_index: *slot_index,
                name: name.clone(),
                vendor: vendor.clone(),
                category: category.clone(),
                path: path.clone(),
                cid: *cid,
                param_cache: Vec::new(),
                component_state: None,
                controller_state: None,
            },

            Self::RemovePlugin {
                slot_index,
                name,
                vendor,
                category,
                path,
                cid,
                ..
            } => Self::AddPlugin {
                slot_index: *slot_index,
                name: name.clone(),
                vendor: vendor.clone(),
                category: category.clone(),
                path: path.clone(),
                cid: *cid,
            },

            Self::ReorderPlugin {
                old_index,
                new_index,
            } => Self::ReorderPlugin {
                old_index: *new_index,
                new_index: *old_index,
            },

            Self::LoadPreset {
                slot_index,
                preset_path,
                preset_name,
                old_component_state,
                old_controller_state,
                new_component_state,
                new_controller_state,
            } => Self::LoadPreset {
                slot_index: *slot_index,
                preset_path: preset_path.clone(),
                preset_name: preset_name.clone(),
                old_component_state: new_component_state.clone(),
                old_controller_state: new_controller_state.clone(),
                new_component_state: old_component_state.clone(),
                new_controller_state: old_controller_state.clone(),
            },

            Self::SetTempo { old_bpm, new_bpm } => Self::SetTempo {
                old_bpm: *new_bpm,
                new_bpm: *old_bpm,
            },

            Self::SetTimeSignature {
                old_numerator,
                old_denominator,
                new_numerator,
                new_denominator,
            } => Self::SetTimeSignature {
                old_numerator: *new_numerator,
                old_denominator: *new_denominator,
                new_numerator: *old_numerator,
                new_denominator: *old_denominator,
            },
        }
    }
}

// ── Undo Stack ──────────────────────────────────────────────────────────

/// Default maximum undo depth.
pub const DEFAULT_MAX_DEPTH: usize = 100;

/// Default coalescing window for parameter changes (milliseconds).
pub const DEFAULT_COALESCE_MS: u64 = 500;

/// The undo/redo stack.
///
/// Stores a linear history of undoable actions. When the user performs
/// a new action after undoing, the redo stack is cleared (standard
/// linear undo model).
///
/// Parameter changes on the same parameter within `coalesce_window_ms`
/// of each other are merged into a single undo entry, preventing the
/// history from being flooded by continuous slider drags.
pub struct UndoStack {
    /// Actions that can be undone (most recent at the end).
    undo_stack: Vec<UndoEntry>,
    /// Actions that can be redone (most recent at the end).
    redo_stack: Vec<UndoEntry>,
    /// Maximum number of entries in the undo stack.
    max_depth: usize,
    /// Window (in ms) for coalescing consecutive parameter changes.
    coalesce_window_ms: u64,
}

/// An entry in the undo stack, pairing an action with its timestamp.
#[derive(Debug, Clone)]
struct UndoEntry {
    /// The undoable action.
    action: UndoableAction,
    /// When the action was performed (for coalescing).
    timestamp: Instant,
}

impl Default for UndoStack {
    fn default() -> Self {
        Self::new()
    }
}

impl UndoStack {
    /// Create a new empty undo stack with default settings.
    pub fn new() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_depth: DEFAULT_MAX_DEPTH,
            coalesce_window_ms: DEFAULT_COALESCE_MS,
        }
    }

    /// Create an undo stack with custom max depth and coalescing window.
    #[allow(dead_code)]
    pub fn with_config(max_depth: usize, coalesce_window_ms: u64) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_depth: max_depth.max(1),
            coalesce_window_ms,
        }
    }

    /// Push a new action onto the undo stack.
    ///
    /// For `SetParameter` actions, attempts to coalesce with the most
    /// recent entry if it targets the same parameter and is within the
    /// coalescing window. Otherwise, pushes a new entry.
    ///
    /// Clears the redo stack (new action invalidates redo history).
    pub fn push(&mut self, action: UndoableAction) {
        let now = Instant::now();

        // Try to coalesce SetParameter actions
        if let UndoableAction::SetParameter {
            slot_index,
            param_id,
            old_value,
            ..
        } = &action
            && let Some(last) = self.undo_stack.last_mut()
            && let UndoableAction::SetParameter {
                slot_index: last_slot,
                param_id: last_param,
                old_value: last_old,
                ..
            } = &last.action
        {
            let elapsed = now.duration_since(last.timestamp).as_millis() as u64;
            if *last_slot == *slot_index
                && *last_param == *param_id
                && elapsed <= self.coalesce_window_ms
            {
                // Coalesce: keep the original old_value, update new_value and timestamp
                let preserved_old = *last_old;
                let _ = old_value; // suppress unused warning
                last.action = UndoableAction::SetParameter {
                    slot_index: *slot_index,
                    param_id: *param_id,
                    old_value: preserved_old,
                    new_value: match &action {
                        UndoableAction::SetParameter { new_value, .. } => *new_value,
                        _ => unreachable!(),
                    },
                    param_name: match &action {
                        UndoableAction::SetParameter { param_name, .. } => param_name.clone(),
                        _ => unreachable!(),
                    },
                };
                last.timestamp = now;

                // Clear redo on coalesced edit too
                self.redo_stack.clear();
                return;
            }
        }

        // Not coalesced — push a new entry
        self.redo_stack.clear();

        self.undo_stack.push(UndoEntry {
            action,
            timestamp: now,
        });

        // Enforce max depth
        if self.undo_stack.len() > self.max_depth {
            self.undo_stack.remove(0);
        }
    }

    /// Pop the most recent action for undo.
    ///
    /// Returns the action to undo. The caller is responsible for
    /// applying the action's inverse to the application state.
    /// The action is moved to the redo stack.
    pub fn undo(&mut self) -> Option<UndoableAction> {
        let entry = self.undo_stack.pop()?;
        let action = entry.action.clone();
        self.redo_stack.push(entry);
        Some(action)
    }

    /// Pop the most recent undone action for redo.
    ///
    /// Returns the action to redo. The caller is responsible for
    /// re-applying the action to the application state.
    /// The action is moved back to the undo stack.
    pub fn redo(&mut self) -> Option<UndoableAction> {
        let entry = self.redo_stack.pop()?;
        let action = entry.action.clone();
        self.undo_stack.push(entry);
        Some(action)
    }

    /// Whether there are actions available to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Whether there are actions available to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// The number of undoable actions.
    #[allow(dead_code)]
    pub fn undo_count(&self) -> usize {
        self.undo_stack.len()
    }

    /// The number of redoable actions.
    #[allow(dead_code)]
    pub fn redo_count(&self) -> usize {
        self.redo_stack.len()
    }

    /// Description of the next action to undo, if any.
    pub fn undo_description(&self) -> Option<String> {
        self.undo_stack.last().map(|e| e.action.description())
    }

    /// Description of the next action to redo, if any.
    pub fn redo_description(&self) -> Option<String> {
        self.redo_stack.last().map(|e| e.action.description())
    }

    /// Get the N most recent undo descriptions (newest first).
    #[allow(dead_code)]
    pub fn recent_undo_descriptions(&self, n: usize) -> Vec<String> {
        self.undo_stack
            .iter()
            .rev()
            .take(n)
            .map(|e| e.action.description())
            .collect()
    }

    /// Clear all undo/redo history.
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    /// Get the maximum depth.
    #[allow(dead_code)]
    pub fn max_depth(&self) -> usize {
        self.max_depth
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    // ── UndoableAction tests ────────────────────────────────────────

    #[test]
    fn test_set_parameter_description() {
        let action = UndoableAction::SetParameter {
            slot_index: 0,
            param_id: 42,
            old_value: 0.5,
            new_value: 0.8,
            param_name: "Volume".into(),
        };
        assert_eq!(action.description(), "Set Volume → 0.800");
    }

    #[test]
    fn test_add_plugin_description() {
        let action = UndoableAction::AddPlugin {
            slot_index: 0,
            name: "Pro-Q 4".into(),
            vendor: "FabFilter".into(),
            category: "Fx".into(),
            path: PathBuf::from("/path/to/plugin.vst3"),
            cid: [0; 16],
        };
        assert_eq!(action.description(), "Add 'Pro-Q 4'");
    }

    #[test]
    fn test_remove_plugin_description() {
        let action = UndoableAction::RemovePlugin {
            slot_index: 0,
            name: "Pro-Q 4".into(),
            vendor: "FabFilter".into(),
            category: "Fx".into(),
            path: PathBuf::from("/path/to/plugin.vst3"),
            cid: [0; 16],
            param_cache: Vec::new(),
            component_state: None,
            controller_state: None,
        };
        assert_eq!(action.description(), "Remove 'Pro-Q 4'");
    }

    #[test]
    fn test_reorder_plugin_description() {
        let action = UndoableAction::ReorderPlugin {
            old_index: 0,
            new_index: 2,
        };
        assert_eq!(action.description(), "Move slot 0 → 2");
    }

    #[test]
    fn test_load_preset_description() {
        let action = UndoableAction::LoadPreset {
            slot_index: 0,
            preset_path: PathBuf::from("/presets/warm.json"),
            preset_name: "Warm".into(),
            old_component_state: None,
            old_controller_state: None,
            new_component_state: None,
            new_controller_state: None,
        };
        assert_eq!(action.description(), "Load preset 'Warm'");
    }

    #[test]
    fn test_set_tempo_description() {
        let action = UndoableAction::SetTempo {
            old_bpm: 120.0,
            new_bpm: 140.0,
        };
        assert_eq!(action.description(), "Set tempo → 140.0 BPM");
    }

    #[test]
    fn test_set_time_signature_description() {
        let action = UndoableAction::SetTimeSignature {
            old_numerator: 4,
            old_denominator: 4,
            new_numerator: 3,
            new_denominator: 4,
        };
        assert_eq!(action.description(), "Set time sig → 3/4");
    }

    // ── Inverse tests ───────────────────────────────────────────────

    #[test]
    fn test_set_parameter_inverse() {
        let action = UndoableAction::SetParameter {
            slot_index: 0,
            param_id: 42,
            old_value: 0.5,
            new_value: 0.8,
            param_name: "Volume".into(),
        };
        let inv = action.inverse();
        match inv {
            UndoableAction::SetParameter {
                old_value,
                new_value,
                ..
            } => {
                assert!((old_value - 0.8).abs() < f64::EPSILON);
                assert!((new_value - 0.5).abs() < f64::EPSILON);
            }
            _ => panic!("Expected SetParameter inverse"),
        }
    }

    #[test]
    fn test_add_plugin_inverse() {
        let action = UndoableAction::AddPlugin {
            slot_index: 2,
            name: "TestPlugin".into(),
            vendor: "Vendor".into(),
            category: "Fx".into(),
            path: PathBuf::from("/test.vst3"),
            cid: [1; 16],
        };
        let inv = action.inverse();
        match inv {
            UndoableAction::RemovePlugin {
                slot_index, name, ..
            } => {
                assert_eq!(slot_index, 2);
                assert_eq!(name, "TestPlugin");
            }
            _ => panic!("Expected RemovePlugin inverse"),
        }
    }

    #[test]
    fn test_remove_plugin_inverse() {
        let action = UndoableAction::RemovePlugin {
            slot_index: 1,
            name: "Plugin".into(),
            vendor: "V".into(),
            category: "Inst".into(),
            path: PathBuf::from("/p.vst3"),
            cid: [2; 16],
            param_cache: Vec::new(),
            component_state: Some(vec![1, 2, 3]),
            controller_state: None,
        };
        let inv = action.inverse();
        match inv {
            UndoableAction::AddPlugin {
                slot_index,
                name,
                cid,
                ..
            } => {
                assert_eq!(slot_index, 1);
                assert_eq!(name, "Plugin");
                assert_eq!(cid, [2; 16]);
            }
            _ => panic!("Expected AddPlugin inverse"),
        }
    }

    #[test]
    fn test_reorder_inverse() {
        let action = UndoableAction::ReorderPlugin {
            old_index: 0,
            new_index: 3,
        };
        let inv = action.inverse();
        match inv {
            UndoableAction::ReorderPlugin {
                old_index,
                new_index,
            } => {
                assert_eq!(old_index, 3);
                assert_eq!(new_index, 0);
            }
            _ => panic!("Expected ReorderPlugin inverse"),
        }
    }

    #[test]
    fn test_set_tempo_inverse() {
        let action = UndoableAction::SetTempo {
            old_bpm: 120.0,
            new_bpm: 140.0,
        };
        let inv = action.inverse();
        match inv {
            UndoableAction::SetTempo { old_bpm, new_bpm } => {
                assert!((old_bpm - 140.0).abs() < f64::EPSILON);
                assert!((new_bpm - 120.0).abs() < f64::EPSILON);
            }
            _ => panic!("Expected SetTempo inverse"),
        }
    }

    #[test]
    fn test_set_time_signature_inverse() {
        let action = UndoableAction::SetTimeSignature {
            old_numerator: 4,
            old_denominator: 4,
            new_numerator: 6,
            new_denominator: 8,
        };
        let inv = action.inverse();
        match inv {
            UndoableAction::SetTimeSignature {
                old_numerator,
                old_denominator,
                new_numerator,
                new_denominator,
            } => {
                assert_eq!(old_numerator, 6);
                assert_eq!(old_denominator, 8);
                assert_eq!(new_numerator, 4);
                assert_eq!(new_denominator, 4);
            }
            _ => panic!("Expected SetTimeSignature inverse"),
        }
    }

    #[test]
    fn test_load_preset_inverse() {
        let action = UndoableAction::LoadPreset {
            slot_index: 0,
            preset_path: PathBuf::from("/presets/warm.json"),
            preset_name: "Warm".into(),
            old_component_state: Some(vec![10, 20]),
            old_controller_state: Some(vec![30, 40]),
            new_component_state: Some(vec![50, 60]),
            new_controller_state: Some(vec![70, 80]),
        };
        let inv = action.inverse();
        match inv {
            UndoableAction::LoadPreset {
                old_component_state,
                old_controller_state,
                new_component_state,
                new_controller_state,
                ..
            } => {
                // Inverse swaps old and new
                assert_eq!(old_component_state, Some(vec![50, 60]));
                assert_eq!(old_controller_state, Some(vec![70, 80]));
                assert_eq!(new_component_state, Some(vec![10, 20]));
                assert_eq!(new_controller_state, Some(vec![30, 40]));
            }
            _ => panic!("Expected LoadPreset inverse"),
        }
    }

    // ── UndoStack basic tests ───────────────────────────────────────

    #[test]
    fn test_new_stack_is_empty() {
        let stack = UndoStack::new();
        assert!(!stack.can_undo());
        assert!(!stack.can_redo());
        assert_eq!(stack.undo_count(), 0);
        assert_eq!(stack.redo_count(), 0);
        assert!(stack.undo_description().is_none());
        assert!(stack.redo_description().is_none());
    }

    #[test]
    fn test_push_and_undo() {
        let mut stack = UndoStack::new();
        stack.push(UndoableAction::SetTempo {
            old_bpm: 120.0,
            new_bpm: 140.0,
        });

        assert!(stack.can_undo());
        assert!(!stack.can_redo());
        assert_eq!(stack.undo_count(), 1);

        let action = stack.undo().unwrap();
        assert!(!stack.can_undo());
        assert!(stack.can_redo());
        assert_eq!(stack.redo_count(), 1);

        match action {
            UndoableAction::SetTempo { new_bpm, .. } => {
                assert!((new_bpm - 140.0).abs() < f64::EPSILON);
            }
            _ => panic!("Wrong action type"),
        }
    }

    #[test]
    fn test_undo_and_redo() {
        let mut stack = UndoStack::new();
        stack.push(UndoableAction::SetTempo {
            old_bpm: 120.0,
            new_bpm: 140.0,
        });

        let _undone = stack.undo().unwrap();
        assert!(stack.can_redo());

        let redone = stack.redo().unwrap();
        assert!(!stack.can_redo());
        assert!(stack.can_undo());

        match redone {
            UndoableAction::SetTempo { new_bpm, .. } => {
                assert!((new_bpm - 140.0).abs() < f64::EPSILON);
            }
            _ => panic!("Wrong action type"),
        }
    }

    #[test]
    fn test_new_action_clears_redo() {
        let mut stack = UndoStack::new();
        stack.push(UndoableAction::SetTempo {
            old_bpm: 120.0,
            new_bpm: 130.0,
        });
        stack.push(UndoableAction::SetTempo {
            old_bpm: 130.0,
            new_bpm: 140.0,
        });

        // Undo one
        stack.undo();
        assert!(stack.can_redo());

        // Push a new action — redo should be cleared
        stack.push(UndoableAction::SetTempo {
            old_bpm: 130.0,
            new_bpm: 150.0,
        });
        assert!(!stack.can_redo());
        assert_eq!(stack.undo_count(), 2);
    }

    #[test]
    fn test_max_depth_eviction() {
        let mut stack = UndoStack::with_config(3, DEFAULT_COALESCE_MS);

        stack.push(UndoableAction::SetTempo {
            old_bpm: 100.0,
            new_bpm: 110.0,
        });
        stack.push(UndoableAction::SetTempo {
            old_bpm: 110.0,
            new_bpm: 120.0,
        });
        stack.push(UndoableAction::SetTempo {
            old_bpm: 120.0,
            new_bpm: 130.0,
        });
        assert_eq!(stack.undo_count(), 3);

        // Push a 4th — oldest should be evicted
        stack.push(UndoableAction::SetTempo {
            old_bpm: 130.0,
            new_bpm: 140.0,
        });
        assert_eq!(stack.undo_count(), 3);

        // The oldest remaining should be 110→120
        let a1 = stack.undo().unwrap();
        let a2 = stack.undo().unwrap();
        let a3 = stack.undo().unwrap();
        assert!(stack.undo().is_none());

        match a3 {
            UndoableAction::SetTempo { new_bpm, .. } => {
                assert!((new_bpm - 120.0).abs() < f64::EPSILON, "oldest = 120");
            }
            _ => panic!("Wrong type"),
        }
        match a2 {
            UndoableAction::SetTempo { new_bpm, .. } => {
                assert!((new_bpm - 130.0).abs() < f64::EPSILON);
            }
            _ => panic!("Wrong type"),
        }
        match a1 {
            UndoableAction::SetTempo { new_bpm, .. } => {
                assert!((new_bpm - 140.0).abs() < f64::EPSILON);
            }
            _ => panic!("Wrong type"),
        }
    }

    #[test]
    fn test_undo_on_empty_returns_none() {
        let mut stack = UndoStack::new();
        assert!(stack.undo().is_none());
    }

    #[test]
    fn test_redo_on_empty_returns_none() {
        let mut stack = UndoStack::new();
        assert!(stack.redo().is_none());
    }

    #[test]
    fn test_clear() {
        let mut stack = UndoStack::new();
        stack.push(UndoableAction::SetTempo {
            old_bpm: 120.0,
            new_bpm: 130.0,
        });
        stack.push(UndoableAction::SetTempo {
            old_bpm: 130.0,
            new_bpm: 140.0,
        });
        stack.undo();

        assert!(stack.can_undo());
        assert!(stack.can_redo());

        stack.clear();
        assert!(!stack.can_undo());
        assert!(!stack.can_redo());
        assert_eq!(stack.undo_count(), 0);
        assert_eq!(stack.redo_count(), 0);
    }

    // ── Parameter coalescing tests ──────────────────────────────────

    #[test]
    fn test_coalesce_same_parameter() {
        // Use a very large coalesce window so the test is deterministic
        let mut stack = UndoStack::with_config(DEFAULT_MAX_DEPTH, 10_000);

        stack.push(UndoableAction::SetParameter {
            slot_index: 0,
            param_id: 1,
            old_value: 0.0,
            new_value: 0.3,
            param_name: "Volume".into(),
        });

        // Quick consecutive change on the same parameter — should coalesce
        stack.push(UndoableAction::SetParameter {
            slot_index: 0,
            param_id: 1,
            old_value: 0.3,
            new_value: 0.6,
            param_name: "Volume".into(),
        });

        // Only 1 entry in the stack (coalesced)
        assert_eq!(stack.undo_count(), 1);

        // The coalesced entry should go from 0.0 → 0.6
        let action = stack.undo().unwrap();
        match action {
            UndoableAction::SetParameter {
                old_value,
                new_value,
                ..
            } => {
                assert!((old_value - 0.0).abs() < f64::EPSILON);
                assert!((new_value - 0.6).abs() < f64::EPSILON);
            }
            _ => panic!("Expected SetParameter"),
        }
    }

    #[test]
    fn test_no_coalesce_different_param() {
        let mut stack = UndoStack::with_config(DEFAULT_MAX_DEPTH, 10_000);

        stack.push(UndoableAction::SetParameter {
            slot_index: 0,
            param_id: 1,
            old_value: 0.0,
            new_value: 0.5,
            param_name: "Volume".into(),
        });
        stack.push(UndoableAction::SetParameter {
            slot_index: 0,
            param_id: 2,
            old_value: 0.5,
            new_value: 0.8,
            param_name: "Pan".into(),
        });

        assert_eq!(
            stack.undo_count(),
            2,
            "Different params should not coalesce"
        );
    }

    #[test]
    fn test_no_coalesce_different_slot() {
        let mut stack = UndoStack::with_config(DEFAULT_MAX_DEPTH, 10_000);

        stack.push(UndoableAction::SetParameter {
            slot_index: 0,
            param_id: 1,
            old_value: 0.0,
            new_value: 0.5,
            param_name: "Volume".into(),
        });
        stack.push(UndoableAction::SetParameter {
            slot_index: 1,
            param_id: 1,
            old_value: 0.5,
            new_value: 0.8,
            param_name: "Volume".into(),
        });

        assert_eq!(stack.undo_count(), 2, "Different slots should not coalesce");
    }

    #[test]
    fn test_no_coalesce_after_timeout() {
        // Use a very short coalesce window
        let mut stack = UndoStack::with_config(DEFAULT_MAX_DEPTH, 10);

        stack.push(UndoableAction::SetParameter {
            slot_index: 0,
            param_id: 1,
            old_value: 0.0,
            new_value: 0.5,
            param_name: "Volume".into(),
        });

        // Wait longer than the coalesce window
        thread::sleep(Duration::from_millis(50));

        stack.push(UndoableAction::SetParameter {
            slot_index: 0,
            param_id: 1,
            old_value: 0.5,
            new_value: 0.8,
            param_name: "Volume".into(),
        });

        assert_eq!(
            stack.undo_count(),
            2,
            "Changes after timeout should not coalesce"
        );
    }

    #[test]
    fn test_no_coalesce_with_non_param_action() {
        let mut stack = UndoStack::with_config(DEFAULT_MAX_DEPTH, 10_000);

        stack.push(UndoableAction::SetParameter {
            slot_index: 0,
            param_id: 1,
            old_value: 0.0,
            new_value: 0.5,
            param_name: "Volume".into(),
        });

        // Interleave a non-parameter action
        stack.push(UndoableAction::SetTempo {
            old_bpm: 120.0,
            new_bpm: 130.0,
        });

        // Next param change should NOT coalesce (different action type in between)
        stack.push(UndoableAction::SetParameter {
            slot_index: 0,
            param_id: 1,
            old_value: 0.5,
            new_value: 0.8,
            param_name: "Volume".into(),
        });

        assert_eq!(stack.undo_count(), 3);
    }

    #[test]
    fn test_coalesce_clears_redo() {
        let mut stack = UndoStack::with_config(DEFAULT_MAX_DEPTH, 10_000);

        stack.push(UndoableAction::SetParameter {
            slot_index: 0,
            param_id: 1,
            old_value: 0.0,
            new_value: 0.5,
            param_name: "Volume".into(),
        });

        // Create a redo entry via undo + redo + undo
        stack.push(UndoableAction::SetTempo {
            old_bpm: 120.0,
            new_bpm: 130.0,
        });
        stack.undo(); // undo tempo → creates redo
        assert!(stack.can_redo());

        // Push a new action (clears redo)
        stack.push(UndoableAction::SetParameter {
            slot_index: 0,
            param_id: 2,
            old_value: 0.0,
            new_value: 0.3,
            param_name: "Pan".into(),
        });
        assert!(!stack.can_redo());
    }

    #[test]
    fn test_multiple_coalesces() {
        let mut stack = UndoStack::with_config(DEFAULT_MAX_DEPTH, 10_000);

        // Simulate a slider drag: many consecutive changes
        for i in 1..=10 {
            stack.push(UndoableAction::SetParameter {
                slot_index: 0,
                param_id: 1,
                old_value: (i - 1) as f64 * 0.1,
                new_value: i as f64 * 0.1,
                param_name: "Volume".into(),
            });
        }

        // All 10 changes should be coalesced into 1
        assert_eq!(stack.undo_count(), 1);

        let action = stack.undo().unwrap();
        match action {
            UndoableAction::SetParameter {
                old_value,
                new_value,
                ..
            } => {
                assert!((old_value - 0.0).abs() < f64::EPSILON, "Original old value");
                assert!((new_value - 1.0).abs() < f64::EPSILON, "Final new value");
            }
            _ => panic!("Expected SetParameter"),
        }
    }

    // ── Description/history tests ───────────────────────────────────

    #[test]
    fn test_descriptions() {
        let mut stack = UndoStack::new();
        stack.push(UndoableAction::SetTempo {
            old_bpm: 120.0,
            new_bpm: 130.0,
        });

        assert_eq!(
            stack.undo_description(),
            Some("Set tempo → 130.0 BPM".into())
        );
        assert!(stack.redo_description().is_none());

        stack.undo();
        assert!(stack.undo_description().is_none());
        assert_eq!(
            stack.redo_description(),
            Some("Set tempo → 130.0 BPM".into())
        );
    }

    #[test]
    fn test_recent_descriptions() {
        let mut stack = UndoStack::with_config(DEFAULT_MAX_DEPTH, 0); // No coalescing

        stack.push(UndoableAction::SetTempo {
            old_bpm: 120.0,
            new_bpm: 130.0,
        });
        stack.push(UndoableAction::AddPlugin {
            slot_index: 0,
            name: "EQ".into(),
            vendor: "V".into(),
            category: "Fx".into(),
            path: PathBuf::from("/eq.vst3"),
            cid: [0; 16],
        });
        stack.push(UndoableAction::SetTempo {
            old_bpm: 130.0,
            new_bpm: 140.0,
        });

        let descs = stack.recent_undo_descriptions(2);
        assert_eq!(descs.len(), 2);
        assert_eq!(descs[0], "Set tempo → 140.0 BPM");
        assert_eq!(descs[1], "Add 'EQ'");
    }

    #[test]
    fn test_recent_descriptions_clamped() {
        let mut stack = UndoStack::new();
        stack.push(UndoableAction::SetTempo {
            old_bpm: 120.0,
            new_bpm: 130.0,
        });

        let descs = stack.recent_undo_descriptions(10);
        assert_eq!(descs.len(), 1);
    }

    // ── Mixed action sequence test ──────────────────────────────────

    #[test]
    fn test_mixed_action_sequence() {
        let mut stack = UndoStack::with_config(DEFAULT_MAX_DEPTH, 0); // No coalescing

        // Add plugin
        stack.push(UndoableAction::AddPlugin {
            slot_index: 0,
            name: "EQ".into(),
            vendor: "V".into(),
            category: "Fx".into(),
            path: PathBuf::from("/eq.vst3"),
            cid: [0; 16],
        });

        // Change tempo
        stack.push(UndoableAction::SetTempo {
            old_bpm: 120.0,
            new_bpm: 140.0,
        });

        // Change parameter
        stack.push(UndoableAction::SetParameter {
            slot_index: 0,
            param_id: 1,
            old_value: 0.5,
            new_value: 0.8,
            param_name: "Gain".into(),
        });

        assert_eq!(stack.undo_count(), 3);

        // Undo all three
        let a3 = stack.undo().unwrap();
        let a2 = stack.undo().unwrap();
        let a1 = stack.undo().unwrap();

        assert!(matches!(a3, UndoableAction::SetParameter { .. }));
        assert!(matches!(a2, UndoableAction::SetTempo { .. }));
        assert!(matches!(a1, UndoableAction::AddPlugin { .. }));

        // All on redo stack now
        assert_eq!(stack.redo_count(), 3);
        assert!(!stack.can_undo());
    }

    // ── with_config tests ───────────────────────────────────────────

    #[test]
    fn test_with_config_min_depth() {
        let stack = UndoStack::with_config(0, 500);
        assert_eq!(stack.max_depth(), 1, "Min depth is clamped to 1");
    }

    #[test]
    fn test_default_config() {
        let stack = UndoStack::new();
        assert_eq!(stack.max_depth(), DEFAULT_MAX_DEPTH);
    }

    // ── Edge case: double undo/redo ──────────────────────────────────

    #[test]
    fn test_undo_redo_undo_redo() {
        let mut stack = UndoStack::new();
        stack.push(UndoableAction::SetTempo {
            old_bpm: 120.0,
            new_bpm: 130.0,
        });

        // Undo → redo → undo → redo
        stack.undo();
        stack.redo();
        stack.undo();
        let action = stack.redo().unwrap();

        assert!(stack.can_undo());
        assert!(!stack.can_redo());

        match action {
            UndoableAction::SetTempo { new_bpm, .. } => {
                assert!((new_bpm - 130.0).abs() < f64::EPSILON);
            }
            _ => panic!("Wrong type"),
        }
    }
}
