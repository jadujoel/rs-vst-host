//! Graph-aware audio processing engine — processes multiple plugins in topological order.
//!
//! Wraps multiple [`AudioEngine`] instances and processes them according
//! to the routing graph's topological sort. Intermediate buffers shuttle
//! audio between nodes. Split/Mix nodes handle signal splitting and summing.
//!
//! # Architecture
//!
//! ```text
//! Input (test tone) → [Engine A] → [Engine B] → Output (device)
//! ```
#![allow(dead_code)]
//!
//! For serial chains, each engine's output feeds the next engine's input.
//! For parallel branches, Split copies the signal to multiple paths and
//! Mix sums them back together.

use crate::audio::graph::{AudioGraph, NodeId, NodeKind};
use std::collections::HashMap;

/// Intermediate stereo buffer for passing audio between graph nodes.
#[derive(Debug, Clone)]
pub struct IntermediateBuffer {
    /// Left channel samples.
    pub left: Vec<f32>,
    /// Right channel samples.
    pub right: Vec<f32>,
    /// Current number of valid samples in the buffer.
    pub num_samples: usize,
}

impl IntermediateBuffer {
    /// Create a new intermediate buffer with given capacity.
    pub fn new(max_block_size: usize) -> Self {
        Self {
            left: vec![0.0; max_block_size],
            right: vec![0.0; max_block_size],
            num_samples: 0,
        }
    }

    /// Clear the buffer to silence.
    pub fn clear(&mut self) {
        self.left[..self.num_samples].fill(0.0);
        self.right[..self.num_samples].fill(0.0);
    }

    /// Fill from interleaved stereo data.
    #[allow(clippy::wrong_self_convention)]
    pub fn from_interleaved(&mut self, data: &[f32], channels: usize) {
        if channels == 0 {
            return;
        }
        self.num_samples = data.len() / channels;
        for i in 0..self.num_samples {
            self.left[i] = data[i * channels];
            self.right[i] = if channels > 1 {
                data[i * channels + 1]
            } else {
                data[i * channels]
            };
        }
    }

    /// Write to interleaved stereo data.
    pub fn to_interleaved(&self, data: &mut [f32], channels: usize) {
        if channels == 0 {
            return;
        }
        let n = self.num_samples.min(data.len() / channels);
        for i in 0..n {
            data[i * channels] = self.left[i];
            if channels > 1 {
                data[i * channels + 1] = self.right[i];
            }
        }
    }

    /// Add another buffer's samples to this one (for Mix node summing).
    pub fn mix_add(&mut self, other: &IntermediateBuffer) {
        let n = self.num_samples.min(other.num_samples);
        for i in 0..n {
            self.left[i] += other.left[i];
            self.right[i] += other.right[i];
        }
    }

    /// Copy from another buffer.
    pub fn copy_from(&mut self, other: &IntermediateBuffer) {
        self.num_samples = other.num_samples;
        let n = other.num_samples;
        self.left[..n].copy_from_slice(&other.left[..n]);
        self.right[..n].copy_from_slice(&other.right[..n]);
    }

    /// Scale all samples by a factor (for equal-power mixing).
    pub fn scale(&mut self, factor: f32) {
        let n = self.num_samples;
        for i in 0..n {
            self.left[i] *= factor;
            self.right[i] *= factor;
        }
    }
}

/// A pool of intermediate buffers for graph processing.
///
/// Pre-allocates buffers to avoid real-time allocations during `process()`.
pub struct BufferPool {
    /// Available buffers, keyed by node ID.
    buffers: HashMap<NodeId, IntermediateBuffer>,
    /// Maximum block size for all buffers.
    max_block_size: usize,
}

impl BufferPool {
    /// Create a new buffer pool.
    pub fn new(max_block_size: usize) -> Self {
        Self {
            buffers: HashMap::new(),
            max_block_size,
        }
    }

    /// Ensure a buffer exists for the given node ID.
    pub fn ensure_buffer(&mut self, node_id: NodeId) {
        self.buffers
            .entry(node_id)
            .or_insert_with(|| IntermediateBuffer::new(self.max_block_size));
    }

    /// Get a reference to a node's buffer.
    pub fn get(&self, node_id: NodeId) -> Option<&IntermediateBuffer> {
        self.buffers.get(&node_id)
    }

    /// Get a mutable reference to a node's buffer.
    pub fn get_mut(&mut self, node_id: NodeId) -> Option<&mut IntermediateBuffer> {
        self.buffers.get_mut(&node_id)
    }

    /// Remove a buffer for a node.
    pub fn remove(&mut self, node_id: NodeId) {
        self.buffers.remove(&node_id);
    }

    /// Clear all buffers.
    pub fn clear_all(&mut self) {
        for buf in self.buffers.values_mut() {
            buf.clear();
        }
    }

    /// Number of buffers currently allocated.
    pub fn len(&self) -> usize {
        self.buffers.len()
    }

    /// Whether the pool is empty.
    pub fn is_empty(&self) -> bool {
        self.buffers.is_empty()
    }
}

/// Process a routing graph using intermediate buffers.
///
/// This function implements the graph processing logic:
/// 1. Compute topological order
/// 2. For each node in order:
///    - Input: fill from test tone / source
///    - Plugin: process using the slot's audio engine
///    - Split: copy input to all outputs
///    - Mix: sum all inputs
///    - Output: write to device buffer
///
/// # Arguments
/// * `graph` - The routing graph
/// * `pool` - Pre-allocated intermediate buffer pool
/// * `output` - The interleaved device output buffer
/// * `channels` - Number of output channels
/// * `process_slot` - Callback to process a single plugin slot: `fn(slot_index, input_left, input_right, output_left, output_right, num_samples)`
/// * `input_generator` - Callback to generate input signal: `fn(buffer_left, buffer_right, num_samples)`
pub fn process_graph<F, G>(
    graph: &mut AudioGraph,
    pool: &mut BufferPool,
    output: &mut [f32],
    channels: usize,
    mut process_slot: F,
    mut input_generator: G,
) where
    F: FnMut(usize, &[f32], &[f32], &mut [f32], &mut [f32], usize) -> bool,
    G: FnMut(&mut [f32], &mut [f32], usize),
{
    if channels == 0 || output.is_empty() {
        return;
    }

    let num_samples = output.len() / channels;
    if num_samples == 0 {
        return;
    }

    // Get topological order
    let order = match graph.topological_order() {
        Ok(o) => o,
        Err(_) => {
            output.fill(0.0);
            return;
        }
    };

    // Ensure buffers exist for all nodes
    for &node_id in &order {
        pool.ensure_buffer(node_id);
    }

    // Set num_samples on all buffers
    for buf in pool.buffers.values_mut() {
        buf.num_samples = num_samples;
        buf.clear();
    }

    let output_node = graph.output_node();

    // Process each node in topological order
    for &node_id in &order {
        let node = match graph.node(node_id) {
            Some(n) => n.clone(),
            None => continue,
        };

        match &node.kind {
            NodeKind::Input => {
                // Generate input signal (test tone, etc.)
                if let Some(buf) = pool.get_mut(node_id) {
                    input_generator(
                        &mut buf.left[..num_samples],
                        &mut buf.right[..num_samples],
                        num_samples,
                    );
                }
            }

            NodeKind::Plugin { slot_index } => {
                let slot_idx = *slot_index;

                // Gather input from predecessors
                let preds = graph.predecessors(node_id);
                if let Some(buf) = pool.get_mut(node_id) {
                    buf.clear();
                }

                // Sum all predecessor outputs into this node's buffer
                // We need to collect predecessor data first to avoid borrow conflicts
                let mut input_left = vec![0.0f32; num_samples];
                let mut input_right = vec![0.0f32; num_samples];

                for &pred_id in &preds {
                    if let Some(pred_buf) = pool.get(pred_id) {
                        let n = num_samples.min(pred_buf.num_samples);
                        for i in 0..n {
                            input_left[i] += pred_buf.left[i];
                            input_right[i] += pred_buf.right[i];
                        }
                    }
                }

                // Process through the plugin (or bypass)
                if node.bypassed {
                    if let Some(buf) = pool.get_mut(node_id) {
                        buf.left[..num_samples].copy_from_slice(&input_left);
                        buf.right[..num_samples].copy_from_slice(&input_right);
                    }
                } else {
                    let mut out_left = vec![0.0f32; num_samples];
                    let mut out_right = vec![0.0f32; num_samples];

                    let success = process_slot(
                        slot_idx,
                        &input_left,
                        &input_right,
                        &mut out_left,
                        &mut out_right,
                        num_samples,
                    );

                    if let Some(buf) = pool.get_mut(node_id) {
                        if success {
                            buf.left[..num_samples].copy_from_slice(&out_left);
                            buf.right[..num_samples].copy_from_slice(&out_right);
                        } else {
                            // Plugin failed — pass through input
                            buf.left[..num_samples].copy_from_slice(&input_left);
                            buf.right[..num_samples].copy_from_slice(&input_right);
                        }
                    }
                }
            }

            NodeKind::Split => {
                // Copy predecessor output to this node's buffer (which successors will read)
                let preds = graph.predecessors(node_id);
                let mut input_left = vec![0.0f32; num_samples];
                let mut input_right = vec![0.0f32; num_samples];

                for &pred_id in &preds {
                    if let Some(pred_buf) = pool.get(pred_id) {
                        let n = num_samples.min(pred_buf.num_samples);
                        for i in 0..n {
                            input_left[i] += pred_buf.left[i];
                            input_right[i] += pred_buf.right[i];
                        }
                    }
                }

                if let Some(buf) = pool.get_mut(node_id) {
                    buf.left[..num_samples].copy_from_slice(&input_left);
                    buf.right[..num_samples].copy_from_slice(&input_right);
                }
            }

            NodeKind::Mix => {
                // Sum all predecessor outputs
                let preds = graph.predecessors(node_id);
                let mut sum_left = vec![0.0f32; num_samples];
                let mut sum_right = vec![0.0f32; num_samples];
                let num_inputs = preds.len().max(1) as f32;

                for &pred_id in &preds {
                    if let Some(pred_buf) = pool.get(pred_id) {
                        let n = num_samples.min(pred_buf.num_samples);
                        for i in 0..n {
                            sum_left[i] += pred_buf.left[i];
                            sum_right[i] += pred_buf.right[i];
                        }
                    }
                }

                // Scale by 1/num_inputs for equal power
                let scale = 1.0 / num_inputs;
                for i in 0..num_samples {
                    sum_left[i] *= scale;
                    sum_right[i] *= scale;
                }

                if let Some(buf) = pool.get_mut(node_id) {
                    buf.left[..num_samples].copy_from_slice(&sum_left);
                    buf.right[..num_samples].copy_from_slice(&sum_right);
                }
            }

            NodeKind::Output => {
                // Sum all predecessor outputs → device output
                let preds = graph.predecessors(node_id);
                let mut sum_left = vec![0.0f32; num_samples];
                let mut sum_right = vec![0.0f32; num_samples];

                for &pred_id in &preds {
                    if let Some(pred_buf) = pool.get(pred_id) {
                        let n = num_samples.min(pred_buf.num_samples);
                        for i in 0..n {
                            sum_left[i] += pred_buf.left[i];
                            sum_right[i] += pred_buf.right[i];
                        }
                    }
                }

                // Write to interleaved output
                let n = num_samples.min(output.len() / channels);
                for i in 0..n {
                    output[i * channels] = sum_left[i];
                    if channels > 1 {
                        output[i * channels + 1] = sum_right[i];
                    }
                }
            }
        }
    }

    // If no output node was processed (shouldn't happen), ensure silence
    if output_node.is_none() {
        output.fill(0.0);
    }
}

/// Information about the processing performance of a graph traversal.
#[derive(Debug, Clone, Default)]
pub struct GraphProcessStats {
    /// Number of nodes processed.
    pub nodes_processed: usize,
    /// Number of plugin nodes processed.
    pub plugins_processed: usize,
    /// Whether any plugin failed during processing.
    pub any_failure: bool,
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::graph::AudioGraph;

    #[test]
    fn test_intermediate_buffer_new() {
        let buf = IntermediateBuffer::new(256);
        assert_eq!(buf.left.len(), 256);
        assert_eq!(buf.right.len(), 256);
        assert_eq!(buf.num_samples, 0);
    }

    #[test]
    fn test_intermediate_buffer_from_interleaved() {
        let mut buf = IntermediateBuffer::new(4);
        let data = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        buf.from_interleaved(&data, 2);
        assert_eq!(buf.num_samples, 4);
        assert_eq!(buf.left[0], 1.0);
        assert_eq!(buf.right[0], 2.0);
        assert_eq!(buf.left[1], 3.0);
        assert_eq!(buf.right[1], 4.0);
    }

    #[test]
    fn test_intermediate_buffer_to_interleaved() {
        let mut buf = IntermediateBuffer::new(4);
        buf.num_samples = 3;
        buf.left[0] = 1.0;
        buf.right[0] = 2.0;
        buf.left[1] = 3.0;
        buf.right[1] = 4.0;
        buf.left[2] = 5.0;
        buf.right[2] = 6.0;

        let mut out = vec![0.0f32; 6];
        buf.to_interleaved(&mut out, 2);
        assert_eq!(out, vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn test_intermediate_buffer_mix_add() {
        let mut buf_a = IntermediateBuffer::new(4);
        buf_a.num_samples = 2;
        buf_a.left[0] = 1.0;
        buf_a.right[0] = 2.0;
        buf_a.left[1] = 3.0;
        buf_a.right[1] = 4.0;

        let mut buf_b = IntermediateBuffer::new(4);
        buf_b.num_samples = 2;
        buf_b.left[0] = 10.0;
        buf_b.right[0] = 20.0;
        buf_b.left[1] = 30.0;
        buf_b.right[1] = 40.0;

        buf_a.mix_add(&buf_b);
        assert_eq!(buf_a.left[0], 11.0);
        assert_eq!(buf_a.right[0], 22.0);
        assert_eq!(buf_a.left[1], 33.0);
        assert_eq!(buf_a.right[1], 44.0);
    }

    #[test]
    fn test_intermediate_buffer_copy_from() {
        let mut src = IntermediateBuffer::new(4);
        src.num_samples = 2;
        src.left[0] = 1.0;
        src.right[0] = 2.0;
        src.left[1] = 3.0;
        src.right[1] = 4.0;

        let mut dst = IntermediateBuffer::new(4);
        dst.copy_from(&src);
        assert_eq!(dst.num_samples, 2);
        assert_eq!(dst.left[0], 1.0);
        assert_eq!(dst.right[0], 2.0);
    }

    #[test]
    fn test_intermediate_buffer_scale() {
        let mut buf = IntermediateBuffer::new(4);
        buf.num_samples = 2;
        buf.left[0] = 4.0;
        buf.right[0] = 6.0;
        buf.left[1] = 8.0;
        buf.right[1] = 10.0;

        buf.scale(0.5);
        assert_eq!(buf.left[0], 2.0);
        assert_eq!(buf.right[0], 3.0);
        assert_eq!(buf.left[1], 4.0);
        assert_eq!(buf.right[1], 5.0);
    }

    #[test]
    fn test_buffer_pool_new() {
        let pool = BufferPool::new(128);
        assert!(pool.is_empty());
        assert_eq!(pool.len(), 0);
    }

    #[test]
    fn test_buffer_pool_ensure_and_get() {
        let mut pool = BufferPool::new(128);
        pool.ensure_buffer(0);
        pool.ensure_buffer(1);
        assert_eq!(pool.len(), 2);
        assert!(pool.get(0).is_some());
        assert!(pool.get(1).is_some());
        assert!(pool.get(99).is_none());
    }

    #[test]
    fn test_buffer_pool_remove() {
        let mut pool = BufferPool::new(128);
        pool.ensure_buffer(0);
        pool.remove(0);
        assert!(pool.get(0).is_none());
    }

    #[test]
    fn test_process_graph_empty() {
        let mut graph = AudioGraph::new();
        let mut pool = BufferPool::new(128);
        let mut output = vec![0.0f32; 256];

        process_graph(
            &mut graph,
            &mut pool,
            &mut output,
            2,
            |_, _, _, _, _, _| true,
            |left, right, n| {
                for i in 0..n {
                    left[i] = 0.5;
                    right[i] = 0.5;
                }
            },
        );

        // With Input → Output and tone=0.5, output should have signal
        assert!(output[0].abs() > 0.0);
    }

    #[test]
    fn test_process_graph_serial_passthrough() {
        // Input → [Plugin 0 (passthrough)] → Output
        let mut graph = AudioGraph::from_serial_chain(&[(0, "Pass".into())]);
        let mut pool = BufferPool::new(128);
        let mut output = vec![0.0f32; 8]; // 4 samples, stereo

        process_graph(
            &mut graph,
            &mut pool,
            &mut output,
            2,
            |_slot, in_l, in_r, out_l, out_r, n| {
                // Passthrough
                out_l[..n].copy_from_slice(&in_l[..n]);
                out_r[..n].copy_from_slice(&in_r[..n]);
                true
            },
            |left, right, n| {
                for i in 0..n {
                    left[i] = 1.0;
                    right[i] = -1.0;
                }
            },
        );

        // Output should be the input signal passthrough
        assert!((output[0] - 1.0).abs() < 0.001);
        assert!((output[1] - (-1.0)).abs() < 0.001);
    }

    #[test]
    fn test_process_graph_serial_two_plugins() {
        // Input → [Plugin 0: double] → [Plugin 1: halve] → Output
        let mut graph = AudioGraph::from_serial_chain(&[(0, "Double".into()), (1, "Halve".into())]);
        let mut pool = BufferPool::new(128);
        let mut output = vec![0.0f32; 4]; // 2 samples, stereo

        process_graph(
            &mut graph,
            &mut pool,
            &mut output,
            2,
            |slot, in_l, in_r, out_l, out_r, n| {
                match slot {
                    0 => {
                        // Double
                        for i in 0..n {
                            out_l[i] = in_l[i] * 2.0;
                            out_r[i] = in_r[i] * 2.0;
                        }
                    }
                    1 => {
                        // Halve
                        for i in 0..n {
                            out_l[i] = in_l[i] * 0.5;
                            out_r[i] = in_r[i] * 0.5;
                        }
                    }
                    _ => {}
                }
                true
            },
            |left, right, n| {
                for i in 0..n {
                    left[i] = 1.0;
                    right[i] = 1.0;
                }
            },
        );

        // 1.0 * 2.0 * 0.5 = 1.0
        assert!((output[0] - 1.0).abs() < 0.001);
        assert!((output[1] - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_process_graph_bypass_node() {
        // Input → [Plugin 0 bypassed] → Output — should pass through input
        let mut graph = AudioGraph::from_serial_chain(&[(0, "Bypassed".into())]);
        let plugin_node = graph.node_for_slot(0).unwrap();
        graph.node_mut(plugin_node).unwrap().bypassed = true;

        let mut pool = BufferPool::new(128);
        let mut output = vec![0.0f32; 4];

        process_graph(
            &mut graph,
            &mut pool,
            &mut output,
            2,
            |_slot, _in_l, _in_r, out_l, out_r, n| {
                // This should NOT be called for bypassed nodes
                for i in 0..n {
                    out_l[i] = 999.0;
                    out_r[i] = 999.0;
                }
                true
            },
            |left, right, n| {
                for i in 0..n {
                    left[i] = 0.75;
                    right[i] = 0.25;
                }
            },
        );

        // Should be passthrough (0.75, 0.25), not the 999.0 from the ignored plugin
        assert!((output[0] - 0.75).abs() < 0.001);
        assert!((output[1] - 0.25).abs() < 0.001);
    }

    #[test]
    fn test_process_graph_parallel_split_mix() {
        // Input → Split → [Plugin A: *2] → Mix → Output
        //                → [Plugin B: *3] ↗
        let mut graph = AudioGraph::new();
        let input = graph.input_node().unwrap();
        let output = graph.output_node().unwrap();
        graph.disconnect(input, output);

        let split = graph.add_node(NodeKind::Split, "Split".into());
        let mix = graph.add_node(NodeKind::Mix, "Mix".into());
        let a = graph.add_node(NodeKind::Plugin { slot_index: 0 }, "Double".into());
        let b = graph.add_node(NodeKind::Plugin { slot_index: 1 }, "Triple".into());

        graph.connect(input, split).unwrap();
        graph.connect(split, a).unwrap();
        graph.connect(split, b).unwrap();
        graph.connect(a, mix).unwrap();
        graph.connect(b, mix).unwrap();
        graph.connect(mix, output).unwrap();

        let mut pool = BufferPool::new(128);
        let mut out = vec![0.0f32; 4];

        process_graph(
            &mut graph,
            &mut pool,
            &mut out,
            2,
            |slot, in_l, _in_r, out_l, out_r, n| {
                let factor = if slot == 0 { 2.0 } else { 3.0 };
                for i in 0..n {
                    out_l[i] = in_l[i] * factor;
                    out_r[i] = in_l[i] * factor;
                }
                true
            },
            |left, right, n| {
                for i in 0..n {
                    left[i] = 1.0;
                    right[i] = 1.0;
                }
            },
        );

        // Plugin A: 1.0 * 2.0 = 2.0
        // Plugin B: 1.0 * 3.0 = 3.0
        // Mix: (2.0 + 3.0) / 2 = 2.5
        assert!((out[0] - 2.5).abs() < 0.01, "left = {}", out[0]);
    }

    #[test]
    fn test_process_graph_plugin_failure() {
        let mut graph = AudioGraph::from_serial_chain(&[(0, "Fail".into())]);
        let mut pool = BufferPool::new(128);
        let mut output = vec![0.0f32; 4];

        process_graph(
            &mut graph,
            &mut pool,
            &mut output,
            2,
            |_slot, _in_l, _in_r, _out_l, _out_r, _n| {
                false // Plugin fails
            },
            |left, right, n| {
                for i in 0..n {
                    left[i] = 0.5;
                    right[i] = 0.5;
                }
            },
        );

        // Failed plugin should pass through input
        assert!((output[0] - 0.5).abs() < 0.001);
        assert!((output[1] - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_process_graph_zero_channels() {
        let mut graph = AudioGraph::new();
        let mut pool = BufferPool::new(128);
        let mut output = vec![0.0f32; 0];

        // Should not panic
        process_graph(
            &mut graph,
            &mut pool,
            &mut output,
            0,
            |_, _, _, _, _, _| true,
            |_, _, _| {},
        );
    }

    #[test]
    fn test_process_graph_mono_output() {
        let mut graph = AudioGraph::new();
        let mut pool = BufferPool::new(128);
        let mut output = vec![0.0f32; 4]; // 4 samples, mono

        process_graph(
            &mut graph,
            &mut pool,
            &mut output,
            1,
            |_, _, _, _, _, _| true,
            |left, _right, n| {
                for sample in left.iter_mut().take(n) {
                    *sample = 0.75;
                }
            },
        );

        assert!((output[0] - 0.75).abs() < 0.001);
    }

    #[test]
    fn test_intermediate_buffer_from_interleaved_mono() {
        let mut buf = IntermediateBuffer::new(4);
        let data = [1.0, 2.0, 3.0, 4.0];
        buf.from_interleaved(&data, 1);
        assert_eq!(buf.num_samples, 4);
        // Mono: both channels get the same value
        assert_eq!(buf.left[0], 1.0);
        assert_eq!(buf.right[0], 1.0);
    }

    #[test]
    fn test_buffer_pool_clear_all() {
        let mut pool = BufferPool::new(4);
        pool.ensure_buffer(0);
        pool.ensure_buffer(1);

        if let Some(buf) = pool.get_mut(0) {
            buf.num_samples = 2;
            buf.left[0] = 1.0;
        }

        pool.clear_all();

        if let Some(buf) = pool.get(0) {
            assert_eq!(buf.left[0], 0.0);
        }
    }

    #[test]
    fn test_graph_process_stats_default() {
        let stats = GraphProcessStats::default();
        assert_eq!(stats.nodes_processed, 0);
        assert_eq!(stats.plugins_processed, 0);
        assert!(!stats.any_failure);
    }

    #[test]
    fn test_process_graph_three_plugins_serial() {
        // Input → [A: +0.1] → [B: +0.2] → [C: +0.3] → Output
        let mut graph =
            AudioGraph::from_serial_chain(&[(0, "A".into()), (1, "B".into()), (2, "C".into())]);
        let mut pool = BufferPool::new(128);
        let mut output = vec![0.0f32; 4];

        process_graph(
            &mut graph,
            &mut pool,
            &mut output,
            2,
            |slot, in_l, in_r, out_l, out_r, n| {
                let add = match slot {
                    0 => 0.1,
                    1 => 0.2,
                    2 => 0.3,
                    _ => 0.0,
                };
                for i in 0..n {
                    out_l[i] = in_l[i] + add;
                    out_r[i] = in_r[i] + add;
                }
                true
            },
            |left, right, n| {
                for i in 0..n {
                    left[i] = 0.0;
                    right[i] = 0.0;
                }
            },
        );

        // 0.0 + 0.1 + 0.2 + 0.3 = 0.6
        assert!((output[0] - 0.6).abs() < 0.01, "left = {}", output[0]);
        assert!((output[1] - 0.6).abs() < 0.01, "right = {}", output[1]);
    }
}
