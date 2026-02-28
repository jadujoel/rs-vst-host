//! Theme for `egui` — glassmorphism-inspired styling.
//!
//! Configures colors, corner radii, shadows, and strokes to achieve
//! the frosted-glass aesthetic described in the design document.
//!
//! Compatible with `egui` 0.31 — uses `CornerRadius` (u8), `Margin` (i8),
//! and `Shadow` (u8/i8) sized types.

use egui::{
    Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, Style, TextStyle, Visuals,
};

// ── Colour Palette ──────────────────────────────────────────────────────────

/// Background base colour (deep blue-black).
pub const BG_BASE: Color32 = Color32::from_rgb(14, 17, 28);

/// Panel fill (translucent white).
pub const PANEL_FILL: Color32 = Color32::from_rgba_premultiplied(18, 18, 18, 18);

/// Active / hovered panel fill (slightly brighter).
#[allow(dead_code)]
pub const PANEL_FILL_HOVER: Color32 = Color32::from_rgba_premultiplied(30, 30, 30, 30);

/// Card/widget fill.
pub const WIDGET_FILL: Color32 = Color32::from_rgba_premultiplied(12, 12, 12, 12);

/// Card/widget fill hovered.
pub const WIDGET_FILL_HOVER: Color32 = Color32::from_rgba_premultiplied(24, 24, 24, 24);

/// Card/widget fill active.
pub const WIDGET_FILL_ACTIVE: Color32 = Color32::from_rgba_premultiplied(16, 25, 40, 40);

/// Accent colour — electric blue for highlights and active elements.
pub const ACCENT: Color32 = Color32::from_rgb(80, 160, 255);

/// Accent dim — softer variant used for less prominent indicators.
pub const ACCENT_DIM: Color32 = Color32::from_rgb(60, 120, 220);

/// Text primary.
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(230, 235, 245);

/// Text secondary (muted).
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(160, 165, 180);

/// Text disabled.
pub const TEXT_DISABLED: Color32 = Color32::from_rgb(90, 95, 110);

/// Glass border — very subtle light edge.
pub const GLASS_BORDER: Color32 = Color32::from_rgba_premultiplied(30, 30, 30, 30);

/// Separator line colour.
pub const SEPARATOR: Color32 = Color32::from_rgba_premultiplied(15, 15, 15, 15);

/// Error / destructive accent.
#[allow(dead_code)]
pub const ERROR: Color32 = Color32::from_rgb(255, 90, 90);

/// Success accent.
#[allow(dead_code)]
pub const SUCCESS: Color32 = Color32::from_rgb(80, 220, 130);

/// Warning accent.
#[allow(dead_code)]
pub const WARNING: Color32 = Color32::from_rgb(255, 190, 60);

// ── Layout Constants ────────────────────────────────────────────────────────

/// Corner radius for glass cards / panels (12 px).
pub const CARD_CORNER_RADIUS: CornerRadius = CornerRadius::same(12);

/// Corner radius for buttons (8 px).
pub const BUTTON_CORNER_RADIUS: CornerRadius = CornerRadius::same(8);

/// Corner radius for small widgets (sliders, text fields) (6 px).
pub const SMALL_CORNER_RADIUS: CornerRadius = CornerRadius::same(6);

/// Standard inner margin for glass cards.
pub const CARD_MARGIN: Margin = Margin {
    left: 16,
    right: 16,
    top: 12,
    bottom: 12,
};

/// Shadow for floating panels.
pub const PANEL_SHADOW: Shadow = Shadow {
    spread: 0,
    blur: 24,
    offset: [0, 4],
    color: Color32::from_rgba_premultiplied(0, 0, 0, 60),
};

// ── Theme Application ───────────────────────────────────────────────────────

/// Apply the Glass theme to an egui context.
pub fn apply(ctx: &egui::Context) {
    let mut style = Style::default();

    // -- Visuals --
    let mut visuals = Visuals::dark();
    visuals.override_text_color = Some(TEXT_PRIMARY);

    // Window (panel) styling
    visuals.window_fill = PANEL_FILL;
    visuals.window_stroke = Stroke::new(1.0, GLASS_BORDER);
    visuals.window_shadow = PANEL_SHADOW;
    visuals.window_corner_radius = CARD_CORNER_RADIUS;

    // Panel styling
    visuals.panel_fill = BG_BASE;

    // Widget styling (non-interactive)
    visuals.widgets.noninteractive.bg_fill = WIDGET_FILL;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, GLASS_BORDER);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_SECONDARY);
    visuals.widgets.noninteractive.corner_radius = SMALL_CORNER_RADIUS;

    // Widget styling (inactive/default)
    visuals.widgets.inactive.bg_fill = WIDGET_FILL;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, GLASS_BORDER);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.inactive.corner_radius = BUTTON_CORNER_RADIUS;

    // Widget styling (hovered)
    visuals.widgets.hovered.bg_fill = WIDGET_FILL_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT_DIM);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.hovered.corner_radius = BUTTON_CORNER_RADIUS;

    // Widget styling (active / pressed)
    visuals.widgets.active.bg_fill = WIDGET_FILL_ACTIVE;
    visuals.widgets.active.bg_stroke = Stroke::new(1.5, ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.active.corner_radius = BUTTON_CORNER_RADIUS;

    // Selection colours
    visuals.selection.bg_fill = Color32::from_rgba_premultiplied(19, 38, 60, 60);
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);

    // Separator
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, SEPARATOR);

    // Hyperlink
    visuals.hyperlink_color = ACCENT;

    style.visuals = visuals;

    // -- Spacing --
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.window_margin = CARD_MARGIN;
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    style.spacing.slider_width = 200.0;

    // -- Text styles --
    let mut text_styles = std::collections::BTreeMap::new();
    text_styles.insert(
        TextStyle::Small,
        FontId::new(11.0, FontFamily::Proportional),
    );
    text_styles.insert(TextStyle::Body, FontId::new(14.0, FontFamily::Proportional));
    text_styles.insert(
        TextStyle::Button,
        FontId::new(14.0, FontFamily::Proportional),
    );
    text_styles.insert(
        TextStyle::Heading,
        FontId::new(20.0, FontFamily::Proportional),
    );
    text_styles.insert(
        TextStyle::Monospace,
        FontId::new(13.0, FontFamily::Monospace),
    );
    style.text_styles = text_styles;

    ctx.set_style(style);
}

// ── Helper Painters ─────────────────────────────────────────────────────────

/// Paint a glass-card frame.
///
/// Draws a rounded rectangle with translucent fill and a subtle border,
/// plus a soft outer shadow for the floating-glass look.
pub fn glass_card_frame() -> egui::Frame {
    egui::Frame {
        inner_margin: CARD_MARGIN,
        outer_margin: Margin::same(4),
        corner_radius: CARD_CORNER_RADIUS,
        shadow: PANEL_SHADOW,
        fill: PANEL_FILL,
        stroke: Stroke::new(1.0, GLASS_BORDER),
    }
}

/// Paint a section header frame (no outer shadow, slightly less padding).
#[allow(dead_code)]
pub fn section_frame() -> egui::Frame {
    egui::Frame {
        inner_margin: Margin {
            left: 12,
            right: 12,
            top: 8,
            bottom: 8,
        },
        outer_margin: Margin::ZERO,
        corner_radius: SMALL_CORNER_RADIUS,
        shadow: Shadow::NONE,
        fill: WIDGET_FILL,
        stroke: Stroke::new(1.0, GLASS_BORDER),
    }
}

/// Return an accent-styled stroke for highlighted elements.
pub fn accent_stroke() -> Stroke {
    Stroke::new(1.5, ACCENT)
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bg_base_is_dark() {
        assert!(BG_BASE.r() < 30);
        assert!(BG_BASE.g() < 30);
        assert!(BG_BASE.b() < 40);
    }

    #[test]
    fn test_accent_is_blue_ish() {
        assert!(ACCENT.b() > ACCENT.r());
        assert!(ACCENT.b() > ACCENT.g());
    }

    #[test]
    fn test_text_primary_is_light() {
        assert!(TEXT_PRIMARY.r() > 200);
        assert!(TEXT_PRIMARY.g() > 200);
        assert!(TEXT_PRIMARY.b() > 200);
    }

    #[test]
    fn test_card_corner_radius_uniform() {
        assert_eq!(CARD_CORNER_RADIUS.nw, 12);
        assert_eq!(CARD_CORNER_RADIUS.ne, 12);
        assert_eq!(CARD_CORNER_RADIUS.sw, 12);
        assert_eq!(CARD_CORNER_RADIUS.se, 12);
    }

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn test_panel_shadow_nonzero_blur() {
        assert!(PANEL_SHADOW.blur > 0);
    }

    #[test]
    fn test_glass_card_frame_has_corner_radius_and_fill() {
        let f = glass_card_frame();
        assert_eq!(f.corner_radius.nw, CARD_CORNER_RADIUS.nw);
        assert_eq!(f.fill, PANEL_FILL);
    }

    #[test]
    fn test_section_frame_no_shadow() {
        let f = section_frame();
        assert_eq!(f.shadow, Shadow::NONE);
    }

    #[test]
    fn test_accent_stroke_width() {
        let s = accent_stroke();
        assert!(s.width > 1.0);
        assert_eq!(s.color, ACCENT);
    }

    #[test]
    fn test_apply_does_not_panic() {
        let ctx = egui::Context::default();
        apply(&ctx);
        // Spot-check one property.
        let style = ctx.style();
        assert_eq!(style.visuals.window_fill, PANEL_FILL);
    }

    #[test]
    fn test_panel_fill_is_translucent() {
        // Premultiplied alpha — alpha channel must be small
        assert!(PANEL_FILL.a() < 50);
    }

    #[test]
    fn test_error_success_warning_distinct() {
        assert_ne!(ERROR, SUCCESS);
        assert_ne!(SUCCESS, WARNING);
        assert_ne!(ERROR, WARNING);
    }
}
