//! The opening animation: the mark settles in with a warm glow, the name rises under it, then the
//! whole layer fades away over the window. About a second; any click or key skips to the fade.
//! With reduced motion it is not shown at all.

use crate::motion::{self, out_cubic, out_quint};
use crate::theme::{self, color};
use eframe::egui::{self, Color32, Id, Order, Pos2, Rect, Sense, pos2, vec2};

/// Mark: fade and scale 0.92 → 1.
const MARK: (f32, f32) = (0.0, 0.55);
/// Name: fade and rise 8 px, starting a beat after the mark.
const NAME: (f32, f32) = (0.16, 0.56);
/// When the layer starts fading out, unless skipped earlier.
const HOLD_UNTIL: f32 = 0.9;
/// How long the layer takes to fade out.
const OUT: f32 = 0.26;

fn ramp(t: f32, (from, to): (f32, f32)) -> f32 {
    ((t - from) / (to - from)).clamp(0.0, 1.0)
}

/// Paint the splash over the window while it lasts. Call last in `update`.
pub fn show(ctx: &egui::Context) {
    let age = motion::age(ctx, Id::new("splash"));
    if age == f32::MAX {
        return; // reduced motion
    }
    let skip_id = Id::new("splash-skip");
    let mut out_at = ctx
        .data(|d| d.get_temp::<f32>(skip_id))
        .unwrap_or(HOLD_UNTIL);
    let fade = 1.0 - out_cubic(((age - out_at) / OUT).clamp(0.0, 1.0));
    if fade <= 0.0 {
        return;
    }
    ctx.request_repaint();

    let screen = ctx.content_rect();
    egui::Area::new(Id::new("splash-layer"))
        .order(Order::Foreground)
        .fixed_pos(screen.min)
        .interactable(fade > 0.5)
        .show(ctx, |ui| {
            // While it is mostly opaque the layer takes clicks, so nothing hidden under it is hit.
            let resp = ui.allocate_rect(screen, Sense::click());
            let skip =
                resp.clicked() || ui.input(|i| i.pointer.any_pressed() || !i.keys_down.is_empty());
            if skip && age < out_at {
                out_at = age;
                ctx.data_mut(|d| d.insert_temp(skip_id, out_at));
            }
            let p = ui.painter();
            p.rect_filled(screen, 0.0, theme::fade(color::BG, fade));

            let c = screen.center() - vec2(0.0, 18.0);
            let m = out_quint(ramp(age, MARK));
            let size = 88.0 * (0.92 + 0.08 * m);
            // The glow blooms with the mark and stays soft under it.
            glow(p, c, 150.0, theme::fade(color::ACCENT, 0.34 * m * fade));
            let rect = Rect::from_center_size(c, vec2(size, size));
            theme::paint_mark_tinted(p, rect, Color32::WHITE.gamma_multiply(m * fade));

            let n = out_quint(ramp(age, NAME));
            let name = p.layout_no_wrap(
                "PhoeCord".into(),
                theme::semibold(22.0),
                theme::fade(color::TEXT, n * fade),
            );
            let y = c.y + 44.0 + 22.0 + 8.0 * (1.0 - n);
            p.galley(pos2(c.x - name.size().x / 2.0, y), name, color::TEXT);
        });
}

/// A soft round glow: a mesh fan from `col` in the middle to transparent at `radius`.
fn glow(p: &egui::Painter, c: Pos2, radius: f32, col: Color32) {
    use egui::epaint::Mesh;
    let mut mesh = Mesh::default();
    mesh.colored_vertex(c, col);
    let steps = 48;
    for i in 0..=steps {
        let a = i as f32 / steps as f32 * std::f32::consts::TAU;
        mesh.colored_vertex(c + vec2(a.cos(), a.sin()) * radius, Color32::TRANSPARENT);
    }
    for i in 1..=steps as u32 {
        mesh.add_triangle(0, i, i + 1);
    }
    p.add(mesh);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramps_clamp_to_their_window() {
        assert_eq!(ramp(-1.0, MARK), 0.0);
        assert_eq!(ramp(10.0, MARK), 1.0);
        assert!((ramp(0.36, NAME) - 0.5).abs() < 1e-6);
    }
}
