//! The look: one palette, one type scale, one spacing and radius scale. Nothing else in the crate
//! names a colour or a size directly.
//!
//! Radius rule: pills are fully round, controls (buttons, fields, switches) use [`R_CONTROL`],
//! tiles and rows [`R_TILE`], dialogs [`R_DIALOG`].

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, Visuals,
};
use std::sync::Arc;

/// Garnet on near-black. Text colours keep at least 4.5:1 against [`BG`] and [`SURFACE`].
pub mod color {
    use super::Color32;

    /// Window background.
    pub const BG: Color32 = Color32::from_rgb(0x0E, 0x0A, 0x0B);
    /// Tiles, rows, fields.
    pub const SURFACE: Color32 = Color32::from_rgb(0x17, 0x10, 0x12);
    /// Secondary buttons, the selected tab, hovered rows.
    pub const RAISED: Color32 = Color32::from_rgb(0x21, 0x16, 0x1A);
    pub const RAISED_HOVER: Color32 = Color32::from_rgb(0x2B, 0x1D, 0x21);
    /// Hairlines between areas and around tiles.
    pub const LINE: Color32 = Color32::from_rgb(0x2F, 0x20, 0x24);
    /// Outlines that must read as an edge (selected tab, focused field).
    pub const LINE_STRONG: Color32 = Color32::from_rgb(0x4A, 0x2B, 0x31);

    pub const TEXT: Color32 = Color32::from_rgb(0xF6, 0xED, 0xEE);
    /// Secondary text (6.9:1 on BG).
    pub const MUTED: Color32 = Color32::from_rgb(0xA8, 0x94, 0x9A);
    /// Disabled text only.
    pub const FAINT: Color32 = Color32::from_rgb(0x74, 0x63, 0x68);

    /// The one accent: primary buttons. White text on it is 7.6:1.
    pub const ACCENT: Color32 = Color32::from_rgb(0xA3, 0x18, 0x2F);
    pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0xB9, 0x1F, 0x39);
    pub const ACCENT_PRESS: Color32 = Color32::from_rgb(0x8C, 0x13, 0x27);
    /// The accent's light tone: selection, focus rings, the tab indicator, progress.
    pub const ROSE: Color32 = Color32::from_rgb(0xF0, 0x6A, 0x7C);
    /// Background of a selected tile.
    pub const SELECTED: Color32 = Color32::from_rgb(0x1E, 0x12, 0x16);

    /// States. Used only for state, never for decoration.
    pub const OK: Color32 = Color32::from_rgb(0x7F, 0xD9, 0xAE);
    pub const OK_BG: Color32 = Color32::from_rgb(0x15, 0x24, 0x1E);
    pub const WARN: Color32 = Color32::from_rgb(0xE9, 0xBC, 0x6A);
    pub const WARN_BG: Color32 = Color32::from_rgb(0x2A, 0x20, 0x12);
    pub const DANGER: Color32 = Color32::from_rgb(0xFF, 0x8A, 0x94);
    pub const DANGER_BG: Color32 = Color32::from_rgb(0x2E, 0x12, 0x16);
    pub const NEUTRAL_BG: Color32 = Color32::from_rgb(0x22, 0x1A, 0x1C);

    /// Dimmed backdrop behind a dialog (alpha applied when drawn).
    pub const SCRIM: Color32 = Color32::from_rgb(0x05, 0x03, 0x04);
}

// Spacing scale (points).
pub const S1: f32 = 4.0;
pub const S2: f32 = 8.0;
pub const S3: f32 = 12.0;
pub const S4: f32 = 16.0;
pub const S5: f32 = 24.0;
pub const S6: f32 = 32.0;
pub const S7: f32 = 48.0;

pub const R_CONTROL: u8 = 8;
pub const R_TILE: u8 = 12;
pub const R_DIALOG: u8 = 14;

pub fn radius(r: u8) -> CornerRadius {
    CornerRadius::same(r)
}

/// Widest the content column gets; the window can be wider.
pub const CONTENT_MAX: f32 = 880.0;
/// Height of the header with the tabs.
pub const HEADER_H: f32 = 60.0;

// Type scale.
pub const T_CAPTION: f32 = 12.5;
pub const T_BODY: f32 = 14.0;
pub const T_BUTTON_BIG: f32 = 15.0;
pub const T_TITLE: f32 = 17.0;
pub const T_H1: f32 = 28.0;

const MEDIUM: &str = "onest-medium";
const SEMIBOLD: &str = "onest-semibold";

pub fn regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}
pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MEDIUM.into()))
}
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}
/// Phosphor icons live in the proportional family as a fallback font.
pub fn icon(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

/// Onest (Latin and Cyrillic, SIL OFL 1.1, `LICENSES/OFL-onest.txt`) in three weights, with the
/// Phosphor icon font as a fallback for icon glyphs and egui's own fonts for anything else.
pub fn fonts() -> FontDefinitions {
    let mut f = FontDefinitions::default();
    let add = |f: &mut FontDefinitions, name: &str, bytes: &'static [u8]| {
        f.font_data
            .insert(name.into(), Arc::new(FontData::from_static(bytes)));
    };
    add(
        &mut f,
        "onest-regular",
        include_bytes!("../assets/fonts/Onest-Regular.ttf"),
    );
    add(
        &mut f,
        MEDIUM,
        include_bytes!("../assets/fonts/Onest-Medium.ttf"),
    );
    add(
        &mut f,
        SEMIBOLD,
        include_bytes!("../assets/fonts/Onest-SemiBold.ttf"),
    );
    egui_phosphor::add_to_fonts(&mut f, egui_phosphor::Variant::Regular);
    // `add_to_fonts` appended "phosphor" to Proportional; put Onest first.
    let fallbacks: Vec<String> = f.families[&FontFamily::Proportional].clone();
    let mut prop = vec!["onest-regular".to_string()];
    prop.extend(fallbacks);
    f.families.insert(FontFamily::Proportional, prop.clone());
    for (name, first) in [(MEDIUM, MEDIUM), (SEMIBOLD, SEMIBOLD)] {
        let mut list = vec![first.to_string()];
        list.extend(prop.iter().skip(1).cloned());
        f.families.insert(FontFamily::Name(name.into()), list);
    }
    f
}

/// egui's own widgets (text fields, scroll bars, tooltips) in the same palette.
pub fn apply(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    let mut v = Visuals::dark();
    v.panel_fill = color::BG;
    v.window_fill = color::SURFACE;
    v.extreme_bg_color = color::SURFACE;
    v.faint_bg_color = color::RAISED;
    v.override_text_color = Some(color::TEXT);
    v.hyperlink_color = color::ROSE;
    v.selection.bg_fill = color::ACCENT;
    v.selection.stroke = stroke(1.0, color::TEXT);
    v.text_cursor.stroke = stroke(2.0, color::ROSE);
    v.window_stroke = stroke(1.0, color::LINE);
    v.window_corner_radius = radius(R_DIALOG);
    v.window_shadow = egui::epaint::Shadow::NONE;
    v.popup_shadow = egui::epaint::Shadow::NONE;
    let w = &mut v.widgets;
    for (state, fill, edge) in [
        (&mut w.noninteractive, color::SURFACE, color::LINE),
        (&mut w.inactive, color::SURFACE, color::LINE),
        (&mut w.hovered, color::RAISED, color::LINE_STRONG),
        (&mut w.active, color::RAISED_HOVER, color::ROSE),
        (&mut w.open, color::RAISED, color::LINE_STRONG),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = stroke(1.0, edge);
        state.corner_radius = radius(R_CONTROL);
        state.fg_stroke = stroke(1.0, color::TEXT);
    }
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(S2, S2);
        s.spacing.button_padding = egui::vec2(S4, S2);
        s.spacing.scroll.bar_width = 6.0;
        s.spacing.scroll.floating = true;
        s.interaction.selectable_labels = false;
        s.text_styles.insert(egui::TextStyle::Body, regular(T_BODY));
        s.text_styles
            .insert(egui::TextStyle::Button, medium(T_BODY));
        s.text_styles
            .insert(egui::TextStyle::Small, regular(T_CAPTION));
        s.text_styles
            .insert(egui::TextStyle::Heading, semibold(T_TITLE));
        // Our own widgets animate themselves; egui's are quick.
        s.animation_time = 0.12;
    });
}

/// The PhoeCord mark: a rounded square, rose at the top to garnet at the bottom.
pub fn paint_mark(painter: &egui::Painter, rect: egui::Rect) {
    let r = (rect.width() * 0.28) as u8;
    paint_vgradient(
        painter,
        rect,
        radius(r),
        color::ROSE,
        Color32::from_rgb(0x7A, 0x10, 0x22),
    );
}

/// A rounded rectangle with a vertical gradient. egui has no gradient fill: the outline is drawn
/// as an anti-aliased polygon in the middle colour, then a mesh fan with per-vertex colours fills
/// it from just inside the edge.
pub fn paint_vgradient(
    painter: &egui::Painter,
    rect: egui::Rect,
    r: CornerRadius,
    top: Color32,
    bottom: Color32,
) {
    use egui::epaint::{Mesh, PathShape, tessellator::path::rounded_rectangle};
    let mut outline = Vec::new();
    rounded_rectangle(&mut outline, rect, r.into());
    painter.add(PathShape::convex_polygon(
        outline,
        lerp_color(top, bottom, 0.5),
        Stroke::NONE,
    ));
    let mut inner = Vec::new();
    let shrink = 0.6;
    let mut ri: egui::epaint::CornerRadiusF32 = r.into();
    for c in [&mut ri.nw, &mut ri.ne, &mut ri.sw, &mut ri.se] {
        *c = (*c - shrink).max(0.0);
    }
    rounded_rectangle(&mut inner, rect.shrink(shrink), ri);
    let at = |y: f32| lerp_color(top, bottom, (y - rect.top()) / rect.height().max(1.0));
    let mut mesh = Mesh::default();
    mesh.colored_vertex(rect.center(), at(rect.center().y));
    for p in &inner {
        mesh.colored_vertex(*p, at(p.y));
    }
    let n = inner.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    painter.add(mesh);
}

/// A stroke (egui's `Stroke::new` takes `impl Into<f32>`, which leaves float literals untyped).
pub fn stroke(width: f32, c: Color32) -> Stroke {
    Stroke::new(width, c)
}

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        l(a.r(), b.r()),
        l(a.g(), b.g()),
        l(a.b(), b.b()),
        l(a.a(), b.a()),
    )
}

/// `c` at `alpha` (0..1) of its opacity.
pub fn fade(c: Color32, alpha: f32) -> Color32 {
    c.gamma_multiply(alpha.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG relative luminance contrast.
    fn contrast(a: Color32, b: Color32) -> f32 {
        fn lum(c: Color32) -> f32 {
            let ch = |v: u8| {
                let s = v as f32 / 255.0;
                if s <= 0.03928 {
                    s / 12.92
                } else {
                    ((s + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
        }
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn text_colours_are_readable() {
        for bg in [color::BG, color::SURFACE, color::RAISED, color::SELECTED] {
            assert!(contrast(color::TEXT, bg) >= 7.0, "{bg:?}");
            assert!(contrast(color::MUTED, bg) >= 4.5, "{bg:?}");
        }
        assert!(contrast(Color32::WHITE, color::ACCENT) >= 4.5);
        assert!(contrast(Color32::WHITE, color::ACCENT_HOVER) >= 4.5);
        for (fg, bg) in [
            (color::OK, color::OK_BG),
            (color::WARN, color::WARN_BG),
            (color::DANGER, color::DANGER_BG),
            (color::MUTED, color::NEUTRAL_BG),
        ] {
            assert!(contrast(fg, bg) >= 4.5, "{fg:?} on {bg:?}");
        }
    }

    #[test]
    fn lerp_ends() {
        assert_eq!(lerp_color(color::BG, color::TEXT, 0.0), color::BG);
        assert_eq!(lerp_color(color::BG, color::TEXT, 1.0), color::TEXT);
    }
}
