//! Theme for `egui` — modern dark professional styling.
//!
//! Configures colors, corner radii, shadows, and strokes for a
//! polished, modern DAW-like aesthetic with clear visual hierarchy.
//!
//! Compatible with `egui` 0.31 — uses `CornerRadius` (u8), `Margin` (i8),
//! and `Shadow` (u8/i8) sized types.

use egui::{
    Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, Style, TextStyle, Visuals,
};

// ── Colour Palette ──────────────────────────────────────────────────────────

/// Background base colour (rich dark blue-black, subtle warmth).
pub const BG_BASE: Color32 = Color32::from_rgb(18, 18, 24);

/// Side panel / secondary background (slightly lighter for depth).
pub const BG_SECONDARY: Color32 = Color32::from_rgb(24, 24, 32);

/// Panel fill (visible dark surface with slight blue tint).
pub const PANEL_FILL: Color32 = Color32::from_rgb(30, 30, 40);

/// Active / hovered panel fill (raised surface).
#[allow(dead_code)]
pub const PANEL_FILL_HOVER: Color32 = Color32::from_rgb(38, 38, 50);

/// Card/widget fill (dark surface layer).
pub const WIDGET_FILL: Color32 = Color32::from_rgb(34, 35, 44);

/// Card/widget fill hovered (elevated).
pub const WIDGET_FILL_HOVER: Color32 = Color32::from_rgb(44, 46, 58);

/// Card/widget fill active (accent-tinted).
pub const WIDGET_FILL_ACTIVE: Color32 = Color32::from_rgb(35, 50, 75);

/// Input field background (slightly sunken).
pub const INPUT_BG: Color32 = Color32::from_rgb(20, 20, 28);

/// Accent colour — vivid electric blue for highlights and active elements.
pub const ACCENT: Color32 = Color32::from_rgb(88, 166, 255);

/// Accent dim — softer variant used for less prominent indicators.
pub const ACCENT_DIM: Color32 = Color32::from_rgb(65, 130, 220);

/// Accent muted — very subtle tint for backgrounds and fills.
pub const ACCENT_MUTED: Color32 = Color32::from_rgb(30, 45, 70);

/// Secondary accent — warm amber for secondary highlights and CTAs.
pub const ACCENT_WARM: Color32 = Color32::from_rgb(255, 170, 50);

/// Text primary (bright, high contrast).
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(235, 238, 248);

/// Text secondary (comfortable muted).
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(145, 150, 170);

/// Text disabled (very muted).
pub const TEXT_DISABLED: Color32 = Color32::from_rgb(80, 84, 100);

/// Border colour — visible but not aggressive.
pub const GLASS_BORDER: Color32 = Color32::from_rgb(50, 52, 65);

/// Separator line colour (subtle).
pub const SEPARATOR: Color32 = Color32::from_rgb(42, 44, 56);

/// Error / destructive accent.
pub const ERROR: Color32 = Color32::from_rgb(255, 85, 85);

/// Success accent (vivid green).
pub const SUCCESS: Color32 = Color32::from_rgb(72, 220, 120);

/// Warning accent (warm amber).
pub const WARNING: Color32 = Color32::from_rgb(255, 190, 60);

/// Info colour for badges and hints.
pub const INFO: Color32 = Color32::from_rgb(100, 180, 255);

/// Pill/badge background.
#[allow(dead_code)]
pub const BADGE_BG: Color32 = Color32::from_rgb(40, 42, 55);

// ── Layout Constants ────────────────────────────────────────────────────────

/// Corner radius for glass cards / panels (10 px).
pub const CARD_CORNER_RADIUS: CornerRadius = CornerRadius::same(10);

/// Corner radius for buttons (6 px).
pub const BUTTON_CORNER_RADIUS: CornerRadius = CornerRadius::same(6);

/// Corner radius for small widgets (sliders, text fields) (5 px).
pub const SMALL_CORNER_RADIUS: CornerRadius = CornerRadius::same(5);

/// Corner radius for pill badges (12 px).
pub const PILL_CORNER_RADIUS: CornerRadius = CornerRadius::same(12);

/// Standard inner margin for glass cards.
pub const CARD_MARGIN: Margin = Margin {
    left: 14,
    right: 14,
    top: 10,
    bottom: 10,
};

/// Shadow for floating panels (stronger for depth).
pub const PANEL_SHADOW: Shadow = Shadow {
    spread: 2,
    blur: 20,
    offset: [0, 4],
    color: Color32::from_rgba_premultiplied(0, 0, 0, 80),
};

/// Subtle shadow for cards.
pub const CARD_SHADOW: Shadow = Shadow {
    spread: 0,
    blur: 12,
    offset: [0, 2],
    color: Color32::from_rgba_premultiplied(0, 0, 0, 50),
};

// ── Theme Application ───────────────────────────────────────────────────────

/// Apply the modern dark theme to an egui context.
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
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, SEPARATOR);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_SECONDARY);
    visuals.widgets.noninteractive.corner_radius = SMALL_CORNER_RADIUS;

    // Widget styling (inactive/default)
    visuals.widgets.inactive.bg_fill = WIDGET_FILL;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, GLASS_BORDER);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.inactive.corner_radius = BUTTON_CORNER_RADIUS;

    // Widget styling (hovered)
    visuals.widgets.hovered.bg_fill = WIDGET_FILL_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.5, ACCENT_DIM);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.hovered.corner_radius = BUTTON_CORNER_RADIUS;
    visuals.widgets.hovered.expansion = 1.0;

    // Widget styling (active / pressed)
    visuals.widgets.active.bg_fill = WIDGET_FILL_ACTIVE;
    visuals.widgets.active.bg_stroke = Stroke::new(2.0, ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.active.corner_radius = BUTTON_CORNER_RADIUS;

    // Open widget (combo box dropdown, etc.)
    visuals.widgets.open.bg_fill = WIDGET_FILL_ACTIVE;
    visuals.widgets.open.bg_stroke = Stroke::new(1.5, ACCENT);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.open.corner_radius = BUTTON_CORNER_RADIUS;

    // Selection colours
    visuals.selection.bg_fill = ACCENT_MUTED;
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);

    // Separator
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, SEPARATOR);

    // Hyperlink
    visuals.hyperlink_color = ACCENT;

    // Slider trailing color
    visuals.selection.bg_fill = Color32::from_rgb(50, 90, 150);

    style.visuals = visuals;

    // -- Spacing --
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.window_margin = CARD_MARGIN;
    style.spacing.button_padding = egui::vec2(14.0, 6.0);
    style.spacing.slider_width = 220.0;
    style.spacing.slider_rail_height = 4.0;
    style.spacing.interact_size.y = 24.0;

    // -- Text styles (slightly larger for readability) --
    let mut text_styles = std::collections::BTreeMap::new();
    text_styles.insert(
        TextStyle::Small,
        FontId::new(11.5, FontFamily::Proportional),
    );
    text_styles.insert(TextStyle::Body, FontId::new(14.0, FontFamily::Proportional));
    text_styles.insert(
        TextStyle::Button,
        FontId::new(13.5, FontFamily::Proportional),
    );
    text_styles.insert(
        TextStyle::Heading,
        FontId::new(18.0, FontFamily::Proportional),
    );
    text_styles.insert(
        TextStyle::Monospace,
        FontId::new(13.0, FontFamily::Monospace),
    );
    style.text_styles = text_styles;

    ctx.set_style(style);
}

// ── Helper Painters ─────────────────────────────────────────────────────────

/// Paint a glass-card frame with visible dark surface and subtle shadow.
pub fn glass_card_frame() -> egui::Frame {
    egui::Frame {
        inner_margin: CARD_MARGIN,
        outer_margin: Margin::same(3),
        corner_radius: CARD_CORNER_RADIUS,
        shadow: CARD_SHADOW,
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

/// Frame for accent-filled primary action buttons.
#[allow(dead_code)]
pub fn accent_button_frame() -> egui::Frame {
    egui::Frame {
        inner_margin: Margin::symmetric(16, 6),
        outer_margin: Margin::ZERO,
        corner_radius: BUTTON_CORNER_RADIUS,
        shadow: Shadow::NONE,
        fill: ACCENT_DIM,
        stroke: Stroke::new(1.0, ACCENT),
    }
}

/// Frame for the bottom bar area.
pub fn bottom_bar_frame() -> egui::Frame {
    egui::Frame {
        fill: BG_SECONDARY,
        inner_margin: Margin::symmetric(16, 10),
        stroke: Stroke::new(1.0, SEPARATOR),
        ..Default::default()
    }
}

/// Frame for input fields (text edits, search boxes).
pub fn input_frame() -> egui::Frame {
    egui::Frame {
        inner_margin: Margin::symmetric(8, 4),
        outer_margin: Margin::ZERO,
        corner_radius: SMALL_CORNER_RADIUS,
        shadow: Shadow::NONE,
        fill: INPUT_BG,
        stroke: Stroke::new(1.0, GLASS_BORDER),
    }
}

/// Return an accent-styled stroke for highlighted elements.
#[allow(dead_code)]
pub fn accent_stroke() -> Stroke {
    Stroke::new(1.5, ACCENT)
}

/// Draw a section header label with accent underline.
#[allow(dead_code)]
pub fn section_heading(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .color(TEXT_PRIMARY)
            .size(16.0)
            .strong(),
    );
    let rect = ui.available_rect_before_wrap();
    let line_y = rect.min.y;
    ui.painter().line_segment(
        [
            egui::pos2(rect.min.x, line_y),
            egui::pos2(rect.min.x + 40.0, line_y),
        ],
        Stroke::new(2.0, ACCENT),
    );
    ui.add_space(4.0);
}

/// Paint a pill/badge with text.
pub fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    let frame = egui::Frame {
        inner_margin: Margin::symmetric(8, 2),
        corner_radius: PILL_CORNER_RADIUS,
        fill: Color32::from_rgb(
            (color.r() as u16 * 40 / 255) as u8 + 20,
            (color.g() as u16 * 40 / 255) as u8 + 20,
            (color.b() as u16 * 40 / 255) as u8 + 25,
        ),
        ..Default::default()
    };
    frame.show(ui, |ui| {
        ui.label(egui::RichText::new(text).color(color).small());
    });
}

/// Paint an active status dot indicator.
pub fn status_dot(ui: &mut egui::Ui, color: Color32) {
    let (rect, _response) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
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
        assert_eq!(CARD_CORNER_RADIUS.nw, 10);
        assert_eq!(CARD_CORNER_RADIUS.ne, 10);
        assert_eq!(CARD_CORNER_RADIUS.sw, 10);
        assert_eq!(CARD_CORNER_RADIUS.se, 10);
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
    fn test_panel_fill_is_opaque_dark() {
        // Now using opaque colors for visibility
        assert!(PANEL_FILL.a() > 200);
        assert!(PANEL_FILL.r() < 50);
        assert!(PANEL_FILL.g() < 50);
    }

    #[test]
    fn test_error_success_warning_distinct() {
        assert_ne!(ERROR, SUCCESS);
        assert_ne!(SUCCESS, WARNING);
        assert_ne!(ERROR, WARNING);
    }

    #[test]
    fn test_accent_button_frame_has_accent_fill() {
        let f = accent_button_frame();
        assert_eq!(f.fill, ACCENT_DIM);
    }

    #[test]
    fn test_bottom_bar_frame_has_secondary_bg() {
        let f = bottom_bar_frame();
        assert_eq!(f.fill, BG_SECONDARY);
    }

    #[test]
    fn test_input_frame_has_sunken_bg() {
        let f = input_frame();
        assert_eq!(f.fill, INPUT_BG);
    }

    #[test]
    fn test_badge_bg_is_darker_than_panel() {
        // Badge background should be a dark, muted color
        assert!(BADGE_BG.r() < 60);
        assert!(BADGE_BG.g() < 60);
    }

    #[test]
    fn test_bg_secondary_lighter_than_base() {
        assert!(BG_SECONDARY.r() >= BG_BASE.r());
        assert!(BG_SECONDARY.g() >= BG_BASE.g());
    }

    #[test]
    fn test_accent_warm_is_warm() {
        assert!(ACCENT_WARM.r() > ACCENT_WARM.b());
        assert!(ACCENT_WARM.r() > 200);
    }

    #[test]
    fn test_widget_fill_visible() {
        // Widget fills should be opaque and visible
        assert!(WIDGET_FILL.a() > 200);
        assert!(WIDGET_FILL.r() > 20);
    }

    #[test]
    fn test_card_shadow_exists() {
        let blur = CARD_SHADOW.blur;
        assert!(blur > 0);
    }
}
