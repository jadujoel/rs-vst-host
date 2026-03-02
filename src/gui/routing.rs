//! Visual routing graph editor — node-based editor panel using `egui`.
//!
//! Renders the audio routing graph as a visual node editor where:
//! - Nodes appear as styled cards with plugin name, bypass toggle, and status
//! - Edges are drawn as bezier curves between node output/input ports
//! - Users can toggle between simple rack view and advanced routing editor
//!
//! The simple rack view shows the serial chain order. The advanced view
//! shows the full graph with drag-to-connect and node positioning.

use crate::audio::graph::{AudioGraph, AudioNode, NodeId, NodeKind};
use crate::gui::theme;
use eframe::egui;

/// Visual configuration for the routing editor.
const NODE_WIDTH: f32 = 140.0;
const NODE_HEIGHT: f32 = 50.0;
const PORT_RADIUS: f32 = 5.0;
const CURVE_TENSION: f32 = 60.0;

/// State for an in-progress drag connection.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DragConnection {
    /// The source node ID.
    pub from_node: NodeId,
    /// Current mouse position.
    pub mouse_pos: egui::Pos2,
}

/// Render the routing graph overview in the rack panel.
///
/// Shows a compact serial chain visualization:
/// `Input → [Plugin A] → [Plugin B] → Output`
///
/// When `show_advanced` is true, shows the full node editor instead.
pub fn show_routing_overview(
    ui: &mut egui::Ui,
    graph: &mut AudioGraph,
    active_slot: Option<usize>,
) {
    let order = match graph.topological_order() {
        Ok(o) => o,
        Err(_) => {
            ui.label(
                egui::RichText::new("⚠ Routing error — graph has cycle")
                    .color(theme::ERROR)
                    .small(),
            );
            return;
        }
    };

    ui.horizontal_wrapped(|ui| {
        for (i, &node_id) in order.iter().enumerate() {
            let node = match graph.node(node_id) {
                Some(n) => n,
                None => continue,
            };

            let (label, color) = match &node.kind {
                NodeKind::Input => ("▸ IN".to_string(), theme::TEXT_DISABLED),
                NodeKind::Output => ("OUT ▸".to_string(), theme::TEXT_DISABLED),
                NodeKind::Plugin { slot_index } => {
                    let is_active = active_slot == Some(*slot_index);
                    let color = if node.bypassed {
                        theme::WARNING
                    } else if is_active {
                        theme::SUCCESS
                    } else {
                        theme::ACCENT
                    };
                    (node.label.clone(), color)
                }
                NodeKind::Split => ("⑂ Split".to_string(), theme::INFO),
                NodeKind::Mix => ("⊕ Mix".to_string(), theme::INFO),
            };

            // Node pill
            match &node.kind {
                NodeKind::Input | NodeKind::Output => {
                    ui.label(egui::RichText::new(&label).color(color).small().monospace());
                }
                _ => {
                    let frame = egui::Frame {
                        inner_margin: egui::Margin::symmetric(8, 3),
                        corner_radius: theme::PILL_CORNER_RADIUS,
                        fill: egui::Color32::from_rgb(
                            (color.r() as u16 * 25 / 255) as u8 + 20,
                            (color.g() as u16 * 25 / 255) as u8 + 20,
                            (color.b() as u16 * 25 / 255) as u8 + 24,
                        ),
                        stroke: egui::Stroke::new(1.0, color),
                        ..Default::default()
                    };
                    frame.show(ui, |ui| {
                        ui.label(egui::RichText::new(&label).color(color).small().strong());
                    });
                }
            }

            // Arrow between nodes
            if i < order.len() - 1 {
                ui.label(egui::RichText::new("→").color(theme::TEXT_DISABLED).small());
            }
        }
    });
}

/// Render the full advanced routing editor.
///
/// Shows nodes as cards positioned in a 2D space with bezier curve
/// connections between them.
pub fn show_routing_editor(ui: &mut egui::Ui, graph: &mut AudioGraph, active_slot: Option<usize>) {
    let available = ui.available_rect_before_wrap();

    // Draw edges first (behind nodes)
    let nodes_snapshot: Vec<(NodeId, AudioNode)> =
        graph.nodes().map(|n| (n.id, n.clone())).collect();

    // Edge drawing
    for edge in graph.edges() {
        let from_node = nodes_snapshot.iter().find(|(id, _)| *id == edge.from_node);
        let to_node = nodes_snapshot.iter().find(|(id, _)| *id == edge.to_node);

        if let (Some((_, from)), Some((_, to))) = (from_node, to_node) {
            let from_pos = node_center(from, available);
            let to_pos = node_center(to, available);

            // Output port position (right side of from node)
            let out_port = egui::pos2(from_pos.x + NODE_WIDTH / 2.0, from_pos.y);
            // Input port position (left side of to node)
            let in_port = egui::pos2(to_pos.x - NODE_WIDTH / 2.0, to_pos.y);

            // Bezier curve
            let cp1 = egui::pos2(out_port.x + CURVE_TENSION, out_port.y);
            let cp2 = egui::pos2(in_port.x - CURVE_TENSION, in_port.y);

            let points = bezier_points(out_port, cp1, cp2, in_port, 20);
            let stroke = egui::Stroke::new(2.0, theme::ACCENT_DIM);

            for window in points.windows(2) {
                ui.painter().line_segment([window[0], window[1]], stroke);
            }

            // Port circles
            ui.painter()
                .circle_filled(out_port, PORT_RADIUS, theme::ACCENT_DIM);
            ui.painter()
                .circle_filled(in_port, PORT_RADIUS, theme::ACCENT_DIM);
        }
    }

    // Draw nodes
    for (node_id, node) in &nodes_snapshot {
        let center = node_center(node, available);
        let node_rect = egui::Rect::from_center_size(center, egui::vec2(NODE_WIDTH, NODE_HEIGHT));

        let (fill, stroke_color) = match &node.kind {
            NodeKind::Input => (theme::WIDGET_FILL, theme::TEXT_DISABLED),
            NodeKind::Output => (theme::WIDGET_FILL, theme::TEXT_DISABLED),
            NodeKind::Plugin { slot_index } => {
                let is_active = active_slot == Some(*slot_index);
                if node.bypassed {
                    (egui::Color32::from_rgb(40, 35, 20), theme::WARNING)
                } else if is_active {
                    (egui::Color32::from_rgb(22, 38, 28), theme::SUCCESS)
                } else {
                    (theme::WIDGET_FILL, theme::ACCENT_DIM)
                }
            }
            NodeKind::Split | NodeKind::Mix => (theme::WIDGET_FILL, theme::INFO),
        };

        // Node card
        ui.painter().rect(
            node_rect,
            theme::CARD_CORNER_RADIUS,
            fill,
            egui::Stroke::new(1.5, stroke_color),
            egui::StrokeKind::Outside,
        );

        // Shadow
        let shadow_rect = node_rect.translate(egui::vec2(2.0, 2.0));
        ui.painter().rect(
            shadow_rect,
            theme::CARD_CORNER_RADIUS,
            egui::Color32::from_black_alpha(40),
            egui::Stroke::NONE,
            egui::StrokeKind::Outside,
        );

        // Re-draw the card on top of shadow
        ui.painter().rect(
            node_rect,
            theme::CARD_CORNER_RADIUS,
            fill,
            egui::Stroke::new(1.5, stroke_color),
            egui::StrokeKind::Outside,
        );

        // Label
        let label = match &node.kind {
            NodeKind::Input => "▸ Input",
            NodeKind::Output => "Output ▸",
            _ => &node.label,
        };

        let label_pos = egui::pos2(node_rect.center().x, node_rect.center().y - 4.0);
        ui.painter().text(
            label_pos,
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(13.0),
            stroke_color,
        );

        // Kind label (smaller, below)
        let kind_str = match &node.kind {
            NodeKind::Input => "",
            NodeKind::Output => "",
            NodeKind::Plugin { slot_index } => {
                if node.bypassed {
                    "BYPASS"
                } else if active_slot == Some(*slot_index) {
                    "ACTIVE"
                } else {
                    "PLUGIN"
                }
            }
            NodeKind::Split => "SPLIT",
            NodeKind::Mix => "MIX",
        };

        if !kind_str.is_empty() {
            let kind_pos = egui::pos2(node_rect.center().x, node_rect.center().y + 12.0);
            ui.painter().text(
                kind_pos,
                egui::Align2::CENTER_CENTER,
                kind_str,
                egui::FontId::proportional(9.0),
                theme::TEXT_DISABLED,
            );
        }

        // Make node interactive (draggable)
        let resp = ui.interact(
            node_rect,
            egui::Id::new(("routing_node", *node_id)),
            egui::Sense::click_and_drag(),
        );

        if resp.dragged() {
            let delta = resp.drag_delta();
            let scale_x = 1.0 / available.width().max(1.0);
            let scale_y = 1.0 / available.height().max(1.0);
            if let Some(n) = graph.node_mut(*node_id) {
                n.x = (n.x + delta.x * scale_x).clamp(0.05, 0.95);
                n.y = (n.y + delta.y * scale_y).clamp(0.05, 0.95);
            }
        }
    }
}

/// Compute the center position of a node in screen space.
fn node_center(node: &AudioNode, rect: egui::Rect) -> egui::Pos2 {
    egui::pos2(
        rect.min.x + node.x * rect.width(),
        rect.min.y + node.y * rect.height(),
    )
}

/// Generate points along a cubic bezier curve.
fn bezier_points(
    p0: egui::Pos2,
    p1: egui::Pos2,
    p2: egui::Pos2,
    p3: egui::Pos2,
    num_points: usize,
) -> Vec<egui::Pos2> {
    (0..=num_points)
        .map(|i| {
            let t = i as f32 / num_points as f32;
            let t2 = t * t;
            let t3 = t2 * t;
            let mt = 1.0 - t;
            let mt2 = mt * mt;
            let mt3 = mt2 * mt;

            egui::pos2(
                mt3 * p0.x + 3.0 * mt2 * t * p1.x + 3.0 * mt * t2 * p2.x + t3 * p3.x,
                mt3 * p0.y + 3.0 * mt2 * t * p1.y + 3.0 * mt * t2 * p2.y + t3 * p3.y,
            )
        })
        .collect()
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_center_computation() {
        let node = AudioNode {
            id: 0,
            kind: NodeKind::Input,
            label: "Input".into(),
            bypassed: false,
            x: 0.5,
            y: 0.5,
        };
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
        let center = node_center(&node, rect);
        assert!((center.x - 400.0).abs() < 0.01);
        assert!((center.y - 300.0).abs() < 0.01);
    }

    #[test]
    fn test_node_center_with_offset() {
        let node = AudioNode {
            id: 0,
            kind: NodeKind::Input,
            label: "Input".into(),
            bypassed: false,
            x: 0.0,
            y: 1.0,
        };
        let rect = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(800.0, 600.0));
        let center = node_center(&node, rect);
        assert!((center.x - 100.0).abs() < 0.01); // x=0 maps to rect.min.x
        assert!((center.y - 650.0).abs() < 0.01); // y=1 maps to rect.max.y
    }

    #[test]
    fn test_bezier_points_count() {
        let points = bezier_points(
            egui::pos2(0.0, 0.0),
            egui::pos2(50.0, 0.0),
            egui::pos2(100.0, 100.0),
            egui::pos2(150.0, 100.0),
            10,
        );
        assert_eq!(points.len(), 11); // 0..=10
    }

    #[test]
    fn test_bezier_endpoints() {
        let p0 = egui::pos2(0.0, 0.0);
        let p3 = egui::pos2(100.0, 200.0);
        let points = bezier_points(p0, egui::pos2(30.0, 0.0), egui::pos2(70.0, 200.0), p3, 20);
        // First point should be at p0
        assert!((points[0].x - p0.x).abs() < 0.01);
        assert!((points[0].y - p0.y).abs() < 0.01);
        // Last point should be at p3
        let last = points.last().unwrap();
        assert!((last.x - p3.x).abs() < 0.01);
        assert!((last.y - p3.y).abs() < 0.01);
    }

    #[test]
    fn test_drag_connection_struct() {
        let dc = DragConnection {
            from_node: 5,
            mouse_pos: egui::pos2(100.0, 200.0),
        };
        assert_eq!(dc.from_node, 5);
    }
}
