//! Audio routing graph — models plugins as nodes in a directed acyclic graph.
//!
//! Supports serial chains (the common case), parallel branches with
//! split/mix nodes, and topological ordering for correct processing.
//!
//! # Architecture
//!
//! ```text
//! Input → [Plugin A] → [Plugin B] → Output   (serial chain)
//!
//! Input → Split → [Plugin A] → Mix → Output   (parallel)
//!                → [Plugin B] ↗
//! ```
//!
//! Each node has typed input/output ports. Edges connect an output port

#![allow(dead_code)]
//! of one node to an input port of another. The graph must be a DAG
//! (directed acyclic graph) — adding a cycle is rejected.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

// ── Node Types ──────────────────────────────────────────────────────────

/// Unique identifier for a node in the audio graph.
pub type NodeId = u32;

/// Unique identifier for an edge in the audio graph.
pub type EdgeId = u32;

/// The type of a node in the routing graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeKind {
    /// Audio input source (test tone, audio device input).
    Input,
    /// Audio output sink (audio device output).
    Output,
    /// A VST3 plugin processing node.
    Plugin {
        /// Index into the rack's plugin slot list.
        slot_index: usize,
    },
    /// Splits a stereo signal into multiple parallel paths.
    Split,
    /// Mixes multiple parallel paths back into a single stereo signal.
    Mix,
}

/// A node in the audio routing graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioNode {
    /// Unique node identifier.
    pub id: NodeId,
    /// The kind of this node.
    pub kind: NodeKind,
    /// Human-readable label (plugin name, "Input", "Output", etc.)
    pub label: String,
    /// Whether this node is bypassed (audio passes through unchanged).
    pub bypassed: bool,
    /// X position in the visual editor (normalized 0..1).
    pub x: f32,
    /// Y position in the visual editor (normalized 0..1).
    pub y: f32,
}

/// A directed edge connecting two nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioEdge {
    /// Unique edge identifier.
    pub id: EdgeId,
    /// Source node.
    pub from_node: NodeId,
    /// Destination node.
    pub to_node: NodeId,
}

// ── Audio Graph ─────────────────────────────────────────────────────────

/// The audio routing graph.
///
/// Maintains nodes and edges, provides topological sort for processing
/// order, and enforces the DAG invariant (no cycles).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioGraph {
    /// All nodes in the graph, keyed by ID.
    nodes: HashMap<NodeId, AudioNode>,
    /// All edges in the graph, keyed by ID.
    edges: HashMap<EdgeId, AudioEdge>,
    /// Next node ID to assign.
    next_node_id: NodeId,
    /// Next edge ID to assign.
    next_edge_id: EdgeId,
    /// Cached topological order (invalidated on structural changes).
    #[serde(skip)]
    cached_order: Option<Vec<NodeId>>,
}

impl Default for AudioGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioGraph {
    /// Create a new empty audio graph with default Input and Output nodes.
    pub fn new() -> Self {
        let mut graph = Self {
            nodes: HashMap::new(),
            edges: HashMap::new(),
            next_node_id: 0,
            next_edge_id: 0,
            cached_order: None,
        };

        // Create default Input and Output nodes
        graph.add_node(NodeKind::Input, "Input".into());
        graph.add_node(NodeKind::Output, "Output".into());

        // Connect Input → Output by default
        graph.connect(0, 1).ok();

        graph
    }

    /// Create a serial chain graph from rack slot indices and names.
    ///
    /// Builds: Input → Plugin[0] → Plugin[1] → ... → Plugin[N] → Output
    #[allow(dead_code)]
    pub fn from_serial_chain(slots: &[(usize, String)]) -> Self {
        let mut graph = Self {
            nodes: HashMap::new(),
            edges: HashMap::new(),
            next_node_id: 0,
            next_edge_id: 0,
            cached_order: None,
        };

        let input_id = graph.add_node(NodeKind::Input, "Input".into());
        let output_id = graph.add_node(NodeKind::Output, "Output".into());

        if slots.is_empty() {
            graph.connect(input_id, output_id).ok();
            return graph;
        }

        let mut prev_id = input_id;
        for (slot_index, name) in slots {
            let node_id = graph.add_node(
                NodeKind::Plugin {
                    slot_index: *slot_index,
                },
                name.clone(),
            );
            graph.connect(prev_id, node_id).ok();
            prev_id = node_id;
        }
        graph.connect(prev_id, output_id).ok();

        graph
    }

    // ── Node Operations ─────────────────────────────────────────────────

    /// Add a node to the graph. Returns its ID.
    pub fn add_node(&mut self, kind: NodeKind, label: String) -> NodeId {
        let id = self.next_node_id;
        self.next_node_id += 1;

        let node_count = self.nodes.len() as f32;
        let node = AudioNode {
            id,
            kind,
            label,
            bypassed: false,
            x: 0.1 + (node_count * 0.15).min(0.8),
            y: 0.5,
        };
        self.nodes.insert(id, node);
        self.cached_order = None;
        id
    }

    /// Remove a node and all its connected edges.
    ///
    /// Returns `true` if the node existed.
    /// Cannot remove the Input or Output nodes.
    pub fn remove_node(&mut self, node_id: NodeId) -> bool {
        if let Some(node) = self.nodes.get(&node_id) {
            match node.kind {
                NodeKind::Input | NodeKind::Output => return false,
                _ => {}
            }
        } else {
            return false;
        }

        self.nodes.remove(&node_id);

        // Remove all edges connected to this node
        let edges_to_remove: Vec<EdgeId> = self
            .edges
            .iter()
            .filter(|(_, e)| e.from_node == node_id || e.to_node == node_id)
            .map(|(id, _)| *id)
            .collect();

        for edge_id in edges_to_remove {
            self.edges.remove(&edge_id);
        }

        self.cached_order = None;
        true
    }

    /// Get a reference to a node by ID.
    pub fn node(&self, id: NodeId) -> Option<&AudioNode> {
        self.nodes.get(&id)
    }

    /// Get a mutable reference to a node by ID.
    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut AudioNode> {
        self.nodes.get_mut(&id)
    }

    /// Get all nodes.
    pub fn nodes(&self) -> impl Iterator<Item = &AudioNode> {
        self.nodes.values()
    }

    /// Get the number of nodes.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Find the Input node ID.
    pub fn input_node(&self) -> Option<NodeId> {
        self.nodes
            .values()
            .find(|n| n.kind == NodeKind::Input)
            .map(|n| n.id)
    }

    /// Find the Output node ID.
    pub fn output_node(&self) -> Option<NodeId> {
        self.nodes
            .values()
            .find(|n| n.kind == NodeKind::Output)
            .map(|n| n.id)
    }

    /// Find all plugin node IDs.
    pub fn plugin_nodes(&self) -> Vec<NodeId> {
        self.nodes
            .values()
            .filter(|n| matches!(n.kind, NodeKind::Plugin { .. }))
            .map(|n| n.id)
            .collect()
    }

    /// Find the node for a given rack slot index.
    pub fn node_for_slot(&self, slot_index: usize) -> Option<NodeId> {
        self.nodes
            .values()
            .find(|n| matches!(n.kind, NodeKind::Plugin { slot_index: s } if s == slot_index))
            .map(|n| n.id)
    }

    // ── Edge Operations ─────────────────────────────────────────────────

    /// Connect two nodes with a directed edge.
    ///
    /// Returns the edge ID on success, or an error if the connection
    /// would create a cycle or if the nodes don't exist.
    pub fn connect(&mut self, from: NodeId, to: NodeId) -> Result<EdgeId, GraphError> {
        if !self.nodes.contains_key(&from) {
            return Err(GraphError::NodeNotFound(from));
        }
        if !self.nodes.contains_key(&to) {
            return Err(GraphError::NodeNotFound(to));
        }
        if from == to {
            return Err(GraphError::SelfLoop(from));
        }

        // Check for duplicate edge
        if self
            .edges
            .values()
            .any(|e| e.from_node == from && e.to_node == to)
        {
            return Err(GraphError::DuplicateEdge(from, to));
        }

        // Tentatively add the edge and check for cycles
        let edge_id = self.next_edge_id;
        let edge = AudioEdge {
            id: edge_id,
            from_node: from,
            to_node: to,
        };
        self.edges.insert(edge_id, edge);
        self.next_edge_id += 1;

        if self.has_cycle() {
            self.edges.remove(&edge_id);
            self.next_edge_id -= 1;
            return Err(GraphError::CycleDetected);
        }

        self.cached_order = None;
        Ok(edge_id)
    }

    /// Disconnect two nodes (remove the edge between them).
    ///
    /// Returns `true` if an edge was removed.
    pub fn disconnect(&mut self, from: NodeId, to: NodeId) -> bool {
        let edge_id = self
            .edges
            .iter()
            .find(|(_, e)| e.from_node == from && e.to_node == to)
            .map(|(id, _)| *id);

        if let Some(id) = edge_id {
            self.edges.remove(&id);
            self.cached_order = None;
            true
        } else {
            false
        }
    }

    /// Remove an edge by ID.
    pub fn remove_edge(&mut self, edge_id: EdgeId) -> bool {
        let removed = self.edges.remove(&edge_id).is_some();
        if removed {
            self.cached_order = None;
        }
        removed
    }

    /// Get all edges.
    pub fn edges(&self) -> impl Iterator<Item = &AudioEdge> {
        self.edges.values()
    }

    /// Get the number of edges.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Get incoming edges for a node.
    pub fn incoming_edges(&self, node_id: NodeId) -> Vec<&AudioEdge> {
        self.edges
            .values()
            .filter(|e| e.to_node == node_id)
            .collect()
    }

    /// Get outgoing edges from a node.
    pub fn outgoing_edges(&self, node_id: NodeId) -> Vec<&AudioEdge> {
        self.edges
            .values()
            .filter(|e| e.from_node == node_id)
            .collect()
    }

    /// Get predecessor node IDs (nodes with edges pointing to `node_id`).
    pub fn predecessors(&self, node_id: NodeId) -> Vec<NodeId> {
        self.edges
            .values()
            .filter(|e| e.to_node == node_id)
            .map(|e| e.from_node)
            .collect()
    }

    /// Get successor node IDs (nodes with edges from `node_id`).
    pub fn successors(&self, node_id: NodeId) -> Vec<NodeId> {
        self.edges
            .values()
            .filter(|e| e.from_node == node_id)
            .map(|e| e.to_node)
            .collect()
    }

    // ── Topological Sort ────────────────────────────────────────────────

    /// Return the processing order (topological sort).
    ///
    /// Uses Kahn's algorithm. Returns `Err` if the graph contains a cycle
    /// (which shouldn't happen if `connect()` is used correctly).
    pub fn topological_order(&mut self) -> Result<Vec<NodeId>, GraphError> {
        if let Some(ref cached) = self.cached_order {
            return Ok(cached.clone());
        }

        let order = self.compute_topological_order()?;
        self.cached_order = Some(order.clone());
        Ok(order)
    }

    /// Compute topological order using Kahn's algorithm.
    fn compute_topological_order(&self) -> Result<Vec<NodeId>, GraphError> {
        let mut in_degree: HashMap<NodeId, usize> = HashMap::new();
        for &id in self.nodes.keys() {
            in_degree.insert(id, 0);
        }
        for edge in self.edges.values() {
            *in_degree.entry(edge.to_node).or_insert(0) += 1;
        }

        let mut queue: VecDeque<NodeId> = VecDeque::new();
        for (&id, &deg) in &in_degree {
            if deg == 0 {
                queue.push_back(id);
            }
        }

        let mut order = Vec::with_capacity(self.nodes.len());
        while let Some(node_id) = queue.pop_front() {
            order.push(node_id);

            for edge in self.edges.values() {
                if edge.from_node == node_id {
                    let deg = in_degree.get_mut(&edge.to_node).unwrap();
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(edge.to_node);
                    }
                }
            }
        }

        if order.len() != self.nodes.len() {
            return Err(GraphError::CycleDetected);
        }

        Ok(order)
    }

    // ── Cycle Detection ─────────────────────────────────────────────────

    /// Check if the graph contains a cycle (DFS-based).
    pub fn has_cycle(&self) -> bool {
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();

        for &node_id in self.nodes.keys() {
            if self.dfs_cycle_check(node_id, &mut visited, &mut rec_stack) {
                return true;
            }
        }
        false
    }

    /// DFS helper for cycle detection.
    fn dfs_cycle_check(
        &self,
        node_id: NodeId,
        visited: &mut HashSet<NodeId>,
        rec_stack: &mut HashSet<NodeId>,
    ) -> bool {
        if rec_stack.contains(&node_id) {
            return true;
        }
        if visited.contains(&node_id) {
            return false;
        }

        visited.insert(node_id);
        rec_stack.insert(node_id);

        for edge in self.edges.values() {
            if edge.from_node == node_id && self.dfs_cycle_check(edge.to_node, visited, rec_stack) {
                return true;
            }
        }

        rec_stack.remove(&node_id);
        false
    }

    // ── Serial Chain Helpers ────────────────────────────────────────────

    /// Check if the graph is a simple serial chain
    /// (Input → Plugin* → Output with no branching).
    pub fn is_serial_chain(&self) -> bool {
        for node in self.nodes.values() {
            match node.kind {
                NodeKind::Input => {
                    if self.outgoing_edges(node.id).len() != 1 {
                        return false;
                    }
                }
                NodeKind::Output => {
                    if self.incoming_edges(node.id).len() != 1 {
                        return false;
                    }
                }
                NodeKind::Plugin { .. } => {
                    if self.incoming_edges(node.id).len() != 1
                        || self.outgoing_edges(node.id).len() != 1
                    {
                        return false;
                    }
                }
                NodeKind::Split | NodeKind::Mix => return false,
            }
        }
        true
    }

    /// Get the serial chain order of plugin slot indices.
    ///
    /// Returns `None` if the graph is not a serial chain.
    pub fn serial_chain_slots(&mut self) -> Option<Vec<usize>> {
        if !self.is_serial_chain() {
            return None;
        }

        let order = self.topological_order().ok()?;
        let slots: Vec<usize> = order
            .iter()
            .filter_map(|id| {
                self.nodes.get(id).and_then(|n| match n.kind {
                    NodeKind::Plugin { slot_index } => Some(slot_index),
                    _ => None,
                })
            })
            .collect();

        Some(slots)
    }

    /// Rebuild the graph as a serial chain from the given rack slot order.
    ///
    /// Preserves Input/Output nodes but removes all other nodes and edges,
    /// then rebuilds a chain: Input → slots[0] → slots[1] → ... → Output.
    pub fn rebuild_serial_chain(&mut self, slots: &[(usize, String)]) {
        // Remove all non-Input/Output nodes and all edges
        let to_remove: Vec<NodeId> = self
            .nodes
            .iter()
            .filter(|(_, n)| !matches!(n.kind, NodeKind::Input | NodeKind::Output))
            .map(|(id, _)| *id)
            .collect();
        for id in to_remove {
            self.nodes.remove(&id);
        }
        self.edges.clear();
        self.cached_order = None;

        let input_id = self.input_node().expect("Input node must exist");
        let output_id = self.output_node().expect("Output node must exist");

        if slots.is_empty() {
            self.connect(input_id, output_id).ok();
            return;
        }

        let mut prev_id = input_id;
        for (slot_index, name) in slots {
            let node_id = self.add_node(
                NodeKind::Plugin {
                    slot_index: *slot_index,
                },
                name.clone(),
            );
            self.connect(prev_id, node_id).ok();
            prev_id = node_id;
        }
        self.connect(prev_id, output_id).ok();
    }

    /// Insert a plugin node into the serial chain at a given position.
    ///
    /// Position 0 = after Input, position N = before Output.
    pub fn insert_in_chain(&mut self, position: usize, slot_index: usize, name: String) -> NodeId {
        let order = self.topological_order().unwrap_or_default();

        let new_id = self.add_node(NodeKind::Plugin { slot_index }, name);

        // Find the nodes that should be before and after the new node
        let plugin_order: Vec<NodeId> = order
            .iter()
            .filter(|id| {
                self.nodes
                    .get(id)
                    .map(|n| !matches!(n.kind, NodeKind::Input | NodeKind::Output))
                    .unwrap_or(false)
            })
            .copied()
            .collect();

        let input_id = self.input_node().unwrap();
        let output_id = self.output_node().unwrap();

        let before_id = if position == 0 {
            input_id
        } else if position <= plugin_order.len() {
            plugin_order[position - 1]
        } else {
            *plugin_order.last().unwrap_or(&input_id)
        };

        let after_id = if position >= plugin_order.len() {
            output_id
        } else {
            plugin_order[position]
        };

        // Remove the edge between before and after
        self.disconnect(before_id, after_id);

        // Insert new node
        self.connect(before_id, new_id).ok();
        self.connect(new_id, after_id).ok();

        new_id
    }

    /// Update slot indices after a rack slot is removed.
    ///
    /// All plugin nodes with `slot_index > removed_index` have their
    /// index decremented by 1.
    pub fn adjust_slot_indices_after_remove(&mut self, removed_index: usize) {
        for node in self.nodes.values_mut() {
            if let NodeKind::Plugin { ref mut slot_index } = node.kind {
                if *slot_index > removed_index {
                    *slot_index -= 1;
                }
            }
        }
    }
}

// ── Error Types ─────────────────────────────────────────────────────────

/// Errors that can occur during graph operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphError {
    /// The specified node was not found.
    NodeNotFound(NodeId),
    /// Adding this edge would create a cycle.
    CycleDetected,
    /// Cannot create a self-loop.
    SelfLoop(NodeId),
    /// An edge between these nodes already exists.
    DuplicateEdge(NodeId, NodeId),
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NodeNotFound(id) => write!(f, "Node {} not found", id),
            Self::CycleDetected => write!(f, "Connection would create a cycle"),
            Self::SelfLoop(id) => write!(f, "Cannot connect node {} to itself", id),
            Self::DuplicateEdge(a, b) => {
                write!(f, "Edge from {} to {} already exists", a, b)
            }
        }
    }
}

impl std::error::Error for GraphError {}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_graph_has_input_output() {
        let graph = AudioGraph::new();
        assert!(graph.input_node().is_some());
        assert!(graph.output_node().is_some());
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edge_count(), 1); // Input → Output
    }

    #[test]
    fn test_add_node() {
        let mut graph = AudioGraph::new();
        let id = graph.add_node(NodeKind::Plugin { slot_index: 0 }, "MyPlugin".into());
        assert!(graph.node(id).is_some());
        assert_eq!(graph.node(id).unwrap().label, "MyPlugin");
        assert_eq!(graph.node_count(), 3);
    }

    #[test]
    fn test_remove_node() {
        let mut graph = AudioGraph::new();
        let id = graph.add_node(NodeKind::Plugin { slot_index: 0 }, "ToRemove".into());
        assert!(graph.remove_node(id));
        assert!(graph.node(id).is_none());
    }

    #[test]
    fn test_cannot_remove_input_output() {
        let mut graph = AudioGraph::new();
        let input = graph.input_node().unwrap();
        let output = graph.output_node().unwrap();
        assert!(!graph.remove_node(input));
        assert!(!graph.remove_node(output));
    }

    #[test]
    fn test_connect_and_disconnect() {
        let mut graph = AudioGraph::new();
        let a = graph.add_node(NodeKind::Plugin { slot_index: 0 }, "A".into());
        let b = graph.add_node(NodeKind::Plugin { slot_index: 1 }, "B".into());

        let edge_id = graph.connect(a, b).unwrap();
        assert!(graph.edges().any(|e| e.id == edge_id));

        assert!(graph.disconnect(a, b));
        assert!(!graph.edges().any(|e| e.id == edge_id));
    }

    #[test]
    fn test_cycle_detection_rejects_cycle() {
        let mut graph = AudioGraph::new();
        let a = graph.add_node(NodeKind::Plugin { slot_index: 0 }, "A".into());
        let b = graph.add_node(NodeKind::Plugin { slot_index: 1 }, "B".into());

        graph.connect(a, b).unwrap();
        let result = graph.connect(b, a);
        assert_eq!(result, Err(GraphError::CycleDetected));
    }

    #[test]
    fn test_self_loop_rejected() {
        let mut graph = AudioGraph::new();
        let a = graph.add_node(NodeKind::Plugin { slot_index: 0 }, "A".into());
        assert_eq!(graph.connect(a, a), Err(GraphError::SelfLoop(a)));
    }

    #[test]
    fn test_duplicate_edge_rejected() {
        let mut graph = AudioGraph::new();
        let a = graph.add_node(NodeKind::Plugin { slot_index: 0 }, "A".into());
        let b = graph.add_node(NodeKind::Plugin { slot_index: 1 }, "B".into());

        graph.connect(a, b).unwrap();
        assert_eq!(graph.connect(a, b), Err(GraphError::DuplicateEdge(a, b)));
    }

    #[test]
    fn test_topological_order_serial() {
        let mut graph =
            AudioGraph::from_serial_chain(&[(0, "A".into()), (1, "B".into()), (2, "C".into())]);

        let order = graph.topological_order().unwrap();
        let input = graph.input_node().unwrap();
        let output = graph.output_node().unwrap();

        // Input must come before all plugins, Output after all plugins
        let input_pos = order.iter().position(|&id| id == input).unwrap();
        let output_pos = order.iter().position(|&id| id == output).unwrap();
        assert!(input_pos < output_pos);

        // All plugin nodes must be between Input and Output
        for node in graph.plugin_nodes() {
            let pos = order.iter().position(|&id| id == node).unwrap();
            assert!(pos > input_pos);
            assert!(pos < output_pos);
        }
    }

    #[test]
    fn test_topological_order_preserves_chain_sequence() {
        let mut graph = AudioGraph::from_serial_chain(&[
            (0, "First".into()),
            (1, "Second".into()),
            (2, "Third".into()),
        ]);

        let order = graph.topological_order().unwrap();
        let nodes: Vec<_> = order
            .iter()
            .filter_map(|id| graph.node(*id))
            .filter(|n| matches!(n.kind, NodeKind::Plugin { .. }))
            .collect();

        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0].label, "First");
        assert_eq!(nodes[1].label, "Second");
        assert_eq!(nodes[2].label, "Third");
    }

    #[test]
    fn test_serial_chain_detection() {
        let graph = AudioGraph::from_serial_chain(&[(0, "A".into()), (1, "B".into())]);
        assert!(graph.is_serial_chain());
    }

    #[test]
    fn test_serial_chain_slots() {
        let mut graph =
            AudioGraph::from_serial_chain(&[(0, "A".into()), (1, "B".into()), (2, "C".into())]);
        let slots = graph.serial_chain_slots().unwrap();
        assert_eq!(slots, vec![0, 1, 2]);
    }

    #[test]
    fn test_empty_serial_chain() {
        let mut graph = AudioGraph::from_serial_chain(&[]);
        assert!(graph.is_serial_chain());
        assert_eq!(graph.serial_chain_slots(), Some(vec![]));
    }

    #[test]
    fn test_rebuild_serial_chain() {
        let mut graph = AudioGraph::from_serial_chain(&[(0, "A".into()), (1, "B".into())]);

        graph.rebuild_serial_chain(&[(0, "X".into()), (1, "Y".into()), (2, "Z".into())]);

        let slots = graph.serial_chain_slots().unwrap();
        assert_eq!(slots, vec![0, 1, 2]);
    }

    #[test]
    fn test_insert_in_chain() {
        let mut graph = AudioGraph::from_serial_chain(&[(0, "A".into()), (1, "B".into())]);

        graph.insert_in_chain(1, 2, "C".into());

        // Should be: Input → A → C → B → Output
        let order = graph.topological_order().unwrap();
        let labels: Vec<_> = order
            .iter()
            .filter_map(|id| graph.node(*id))
            .filter(|n| matches!(n.kind, NodeKind::Plugin { .. }))
            .map(|n| n.label.as_str())
            .collect();
        assert_eq!(labels, vec!["A", "C", "B"]);
    }

    #[test]
    fn test_remove_node_reconnects_nothing() {
        let mut graph = AudioGraph::from_serial_chain(&[(0, "A".into()), (1, "B".into())]);

        let a_id = graph.node_for_slot(0).unwrap();
        graph.remove_node(a_id);

        // A is gone, edges to/from A are gone
        assert!(graph.node(a_id).is_none());
        assert_eq!(graph.plugin_nodes().len(), 1);
    }

    #[test]
    fn test_node_for_slot() {
        let graph = AudioGraph::from_serial_chain(&[(0, "A".into()), (3, "D".into())]);

        assert!(graph.node_for_slot(0).is_some());
        assert!(graph.node_for_slot(3).is_some());
        assert!(graph.node_for_slot(99).is_none());
    }

    #[test]
    fn test_predecessors_and_successors() {
        let graph = AudioGraph::from_serial_chain(&[(0, "A".into()), (1, "B".into())]);

        let a = graph.node_for_slot(0).unwrap();
        let b = graph.node_for_slot(1).unwrap();
        let input = graph.input_node().unwrap();
        let output = graph.output_node().unwrap();

        assert_eq!(graph.predecessors(a), vec![input]);
        assert_eq!(graph.successors(a), vec![b]);
        assert_eq!(graph.predecessors(b), vec![a]);
        assert_eq!(graph.successors(b), vec![output]);
    }

    #[test]
    fn test_has_no_cycle_in_valid_graph() {
        let graph = AudioGraph::from_serial_chain(&[(0, "A".into()), (1, "B".into())]);
        assert!(!graph.has_cycle());
    }

    #[test]
    fn test_parallel_routing() {
        let mut graph = AudioGraph::new();
        let input = graph.input_node().unwrap();
        let output = graph.output_node().unwrap();

        // Remove default Input → Output edge
        graph.disconnect(input, output);

        let split = graph.add_node(NodeKind::Split, "Split".into());
        let mix = graph.add_node(NodeKind::Mix, "Mix".into());
        let a = graph.add_node(NodeKind::Plugin { slot_index: 0 }, "EQ".into());
        let b = graph.add_node(NodeKind::Plugin { slot_index: 1 }, "Comp".into());

        // Input → Split → {A, B} → Mix → Output
        graph.connect(input, split).unwrap();
        graph.connect(split, a).unwrap();
        graph.connect(split, b).unwrap();
        graph.connect(a, mix).unwrap();
        graph.connect(b, mix).unwrap();
        graph.connect(mix, output).unwrap();

        assert!(!graph.has_cycle());
        assert!(!graph.is_serial_chain());

        let order = graph.topological_order().unwrap();
        let input_pos = order.iter().position(|&id| id == input).unwrap();
        let split_pos = order.iter().position(|&id| id == split).unwrap();
        let mix_pos = order.iter().position(|&id| id == mix).unwrap();
        let output_pos = order.iter().position(|&id| id == output).unwrap();

        assert!(input_pos < split_pos);
        assert!(split_pos < mix_pos);
        assert!(mix_pos < output_pos);
    }

    #[test]
    fn test_adjust_slot_indices() {
        let mut graph =
            AudioGraph::from_serial_chain(&[(0, "A".into()), (1, "B".into()), (2, "C".into())]);

        graph.adjust_slot_indices_after_remove(0);

        // Slot 0 is now invalid, slots 1→0, 2→1
        let mut slots: Vec<usize> = graph
            .plugin_nodes()
            .iter()
            .filter_map(|id| match graph.node(*id)?.kind {
                NodeKind::Plugin { slot_index } => Some(slot_index),
                _ => None,
            })
            .collect();
        slots.sort();
        assert_eq!(slots, vec![0, 0, 1]); // original 0 stays 0, 1→0, 2→1
    }

    #[test]
    fn test_graph_error_display() {
        assert_eq!(GraphError::NodeNotFound(5).to_string(), "Node 5 not found");
        assert_eq!(
            GraphError::CycleDetected.to_string(),
            "Connection would create a cycle"
        );
        assert_eq!(
            GraphError::SelfLoop(3).to_string(),
            "Cannot connect node 3 to itself"
        );
        assert_eq!(
            GraphError::DuplicateEdge(1, 2).to_string(),
            "Edge from 1 to 2 already exists"
        );
    }

    #[test]
    fn test_graph_serialization() {
        let graph = AudioGraph::from_serial_chain(&[(0, "A".into()), (1, "B".into())]);

        let json = serde_json::to_string(&graph).unwrap();
        let restored: AudioGraph = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.node_count(), graph.node_count());
        assert_eq!(restored.edge_count(), graph.edge_count());
    }

    #[test]
    fn test_node_bypass_toggle() {
        let mut graph = AudioGraph::new();
        let id = graph.add_node(NodeKind::Plugin { slot_index: 0 }, "A".into());
        assert!(!graph.node(id).unwrap().bypassed);
        graph.node_mut(id).unwrap().bypassed = true;
        assert!(graph.node(id).unwrap().bypassed);
    }

    #[test]
    fn test_connect_nonexistent_node() {
        let mut graph = AudioGraph::new();
        assert_eq!(graph.connect(999, 0), Err(GraphError::NodeNotFound(999)));
    }

    #[test]
    fn test_remove_nonexistent_node() {
        let mut graph = AudioGraph::new();
        assert!(!graph.remove_node(999));
    }

    #[test]
    fn test_remove_edge_by_id() {
        let mut graph = AudioGraph::new();
        let a = graph.add_node(NodeKind::Plugin { slot_index: 0 }, "A".into());
        let b = graph.add_node(NodeKind::Plugin { slot_index: 1 }, "B".into());
        let edge_id = graph.connect(a, b).unwrap();

        assert!(graph.remove_edge(edge_id));
        assert!(!graph.remove_edge(edge_id)); // already removed
    }
}
