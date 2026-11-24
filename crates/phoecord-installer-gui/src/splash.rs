//! The opening animation, about 1.4 s, played once per start:
//!
//! | Element         | Role       | Starts | Length | Motion                                   |
//! |-----------------|------------|--------|--------|------------------------------------------|
//! | Glow            | decoration | 0.00   | 0.60   | blooms out from the middle, fade only    |
//! | Mark (flame)    | lead       | 0.05   | 0.60   | rises 24 px, 0.86 → 1, garnet → white    |
//! | Ring            | decoration | 0.38   | 0.55   | one thin ring spreads out as it lands    |
//! | Name            | support    | 0.22   | 0.48   | fades in, rises 10 px                    |
//! | Splash layer    | hand-off   | 0.95   | 0.32   | mark grows 1 → 1.06 as the layer fades   |
//! | Header          | support    | 0.95   | 0.32   | fades in, rises 8 px under the layer     |
//! | Window content  | lead       | 1.02   | 0.38   | fades in, rises 16 px                    |
//!
//! Any click or key jumps to the hand-off. With "Reduce motion" none of it is shown.
//!
//! The clock is the splash's own: it advances by each frame's time but at most [`MAX_STEP`], so the
//! first slow frames (the window and the GPU starting up) do not eat the animation, which is why
//! a wall-clock version was over before it could be seen.

use crate::motion::{self, out_cubic, out_quint};
use crate::theme::{self, color};
use eframe::egui::{self, Color32, Id, Order, Pos2, Rect, Sense, Stroke, pos2, vec2};

const GLOW: (f32, f32) = (0.0, 0.6);
const MARK: (f32, f32) = (0.05, 0.65);
const RING: (f32, f32) = (0.38, 0.93);
const NAME: (f32, f32) = (0.22, 0.70);
/// When the hand-off starts, unless skipped earlier.
const HOLD_UNTIL: f32 = 0.95;
/// How long the splash layer takes to fade.
const OUT: f32 = 0.32;
/// The header comes in with the fade; the content a beat later and a little slower.
const HEADER_IN: (f32, f32) = (0.0, 0.32);
const CONTENT_IN: (f32, f32) = (0.07, 0.45);
/// The longest step one frame may move the clock (two 60 Hz frames).
const MAX_STEP: f32 = 1.0 / 30.0;

fn ramp(t: f32, (from, to): (f32, f32)) -> f32 {
    ((t - from) / (to - from)).clamp(0.0, 1.0)
}

#[derive(Clone, Copy)]
struct Clock {
    /// Splash time, seconds.
    t: f32,
    /// The frame that last moved it, so it moves once per frame.
    pass: u64,
    /// When the hand-off starts (moved earlier by a skip).
    out_at: f32,
}

fn clock_id() -> Id {
    Id::new("splash-clock")
}

fn read(ctx: &egui::Context) -> Option<Clock> {
    if motion::reduced(ctx) {
        return None;
    }
    Some(
        ctx.data(|d| d.get_temp::<Clock>(clock_id()))
            .unwrap_or(Clock {
                t: 0.0,
                pass: u64::MAX,
                out_at: HOLD_UNTIL,
            }),
    )
}

/// Seconds since the hand-off started (negative before it), or `None` when it is all over or
/// motion is reduced.
fn since_out(ctx: &egui::Context) -> Option<f32> {
    read(ctx)
        .map(|c| c.t - c.out_at)
        .filter(|s| *s < OUT + CONTENT_IN.1)
}

/// How far the window's own content has come in: 0 under the splash, 1 once it is over.
/// `(opacity, rise in px)` for the header (`content` false) or the rest of the window.
pub fn reveal(ctx: &egui::Context, content: bool) -> (f32, f32) {
    let Some(s) = since_out(ctx) else {
        return (1.0, 0.0);
    };
    let (window, dist) = if content {
        (CONTENT_IN, 16.0)
    } else {
        (HEADER_IN, 8.0)
    };
    let k = out_quint(ramp(s, window));
    (k, dist * (1.0 - k))
}

/// Paint the splash over the window while it lasts. Call last in `update`.
pub fn show(ctx: &egui::Context) {
    let Some(mut clock) = read(ctx) else {
        return;
    };
    let pass = ctx.cumulative_pass_nr();
    if clock.pass != pass {
        if clock.pass != u64::MAX {
            clock.t += ctx.input(|i| i.unstable_dt).min(MAX_STEP);
        }
        clock.pass = pass;
    }
    let age = clock.t;
    if age - clock.out_at >= OUT + CONTENT_IN.1 {
        ctx.data_mut(|d| d.insert_temp(clock_id(), clock));
        return;
    }
    ctx.request_repaint();
    let out = out_cubic(ramp(age - clock.out_at, (0.0, OUT)));
    let fade = 1.0 - out;

    let screen = ctx.content_rect();
    if fade > 0.0 {
        egui::Area::new(Id::new("splash-layer"))
            .order(Order::Foreground)
            .fixed_pos(screen.min)
            .interactable(fade > 0.5)
            .show(ctx, |ui| {
                // While it is mostly opaque the layer takes clicks, so nothing hidden under it is
                // hit by the click that skips it.
                let resp = ui.allocate_rect(screen, Sense::click());
                let skip = resp.clicked()
                    || ui.input(|i| i.pointer.any_pressed() || !i.keys_down.is_empty());
                if skip && age < clock.out_at {
                    clock.out_at = age;
                }
                paint(ui.painter(), screen, age, fade, out);
            });
    }
    ctx.data_mut(|d| d.insert_temp(clock_id(), clock));
}

fn paint(p: &egui::Painter, screen: Rect, age: f32, fade: f32, out: f32) {
    p.rect_filled(screen, 0.0, theme::fade(color::BG, fade));
    let c = screen.center() - vec2(0.0, 18.0);

    let g = out_quint(ramp(age, GLOW));
    glow(
        p,
        c,
        60.0 + 110.0 * g,
        theme::fade(color::ACCENT, 0.42 * g * fade),
    );

    let r = ramp(age, RING);
    if r > 0.0 && r < 1.0 {
        let k = out_cubic(r);
        p.circle_stroke(
            c,
            52.0 + 70.0 * k,
            Stroke::new(1.5_f32, theme::fade(color::ROSE, 0.45 * (1.0 - k) * fade)),
        );
    }

    // The flame lifts into place and catches: garnet while it rises, white once it is there.
    let m = out_quint(ramp(age, MARK));
    let size = 88.0 * (0.86 + 0.14 * m) * (1.0 + 0.06 * out);
    let rect = Rect::from_center_size(c + vec2(0.0, 24.0 * (1.0 - m)), vec2(size, size));
    let tint = lerp(
        color::ACCENT,
        Color32::WHITE,
        out_cubic(ramp(age, (0.15, 0.6))),
    );
    theme::paint_mark_tinted(p, rect, tint.gamma_multiply(m * fade));

    let n = out_quint(ramp(age, NAME));
    let name = p.layout_no_wrap(
        "PhoeCord".into(),
        theme::semibold(22.0),
        theme::fade(color::TEXT, n * fade),
    );
    let y = c.y + 44.0 + 22.0 + 10.0 * (1.0 - n);
    p.galley(pos2(c.x - name.size().x / 2.0, y), name, color::TEXT);
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let ch = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(ch(a.r(), b.r()), ch(a.g(), b.g()), ch(a.b(), b.b()))
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
        assert!((ramp(0.46, NAME) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn fits_the_owner_s_length() {
        // The whole thing, intro and hand-off, stays within 1–1.5 s.
        let total = HOLD_UNTIL + OUT.max(CONTENT_IN.1);
        assert!((1.0..=1.5).contains(&total), "{total}");
        // Everything in the intro has landed before the hand-off.
        for w in [GLOW, MARK, RING, NAME] {
            assert!(w.1 <= HOLD_UNTIL, "{w:?}");
        }
    }

    #[test]
    fn colours_blend_end_to_end() {
        assert_eq!(lerp(color::ACCENT, Color32::WHITE, 0.0), color::ACCENT);
        assert_eq!(lerp(color::ACCENT, Color32::WHITE, 1.0), Color32::WHITE);
    }
}
