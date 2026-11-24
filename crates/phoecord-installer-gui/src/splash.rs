//! The opening animation, about 1.4 s, played once per start. The flame lights up, then the window
//! opens out of it like an iris:
//!
//! | Element        | Role       | Starts | Length | Motion                                          |
//! |----------------|------------|--------|--------|-------------------------------------------------|
//! | Glow           | decoration | 0.00   | 0.55   | blooms out from the middle, fade only           |
//! | Mark (flame)   | lead       | 0.05   | 0.50   | rises 28 px, 0.82 → 1 with a 4 % overshoot,     |
//! |                |            |        |        | garnet → white                                  |
//! | Ignition flash | decoration | 0.42   | 0.33   | a rose bloom at the moment the mark lands       |
//! | Shock rings    | decoration | 0.42   | 0.53   | two rings, the second 60 ms behind and thinner  |
//! | Embers         | decoration | 0.40   | ≤ 0.70 | sparks lift off the flame, drift and go out     |
//! | Name           | support    | 0.30   | 0.65   | letters rise in a wave, 35 ms apart             |
//! | Iris           | hand-off   | 0.95   | 0.45   | a hole opens from the flame to the corners,     |
//! |                |            |        |        | with a soft garnet rim; mark grows and fades    |
//! | Header         | support    | 0.95   | 0.32   | fades in, rises 8 px inside the iris            |
//! | Window content | lead       | 1.00   | 0.42   | fades in, rises 16 px                           |
//!
//! Any click or key jumps to the iris. With "Reduce motion" none of it is shown.
//!
//! The clock is the splash's own: it advances by each frame's time but at most [`MAX_STEP`], so the
//! first slow frames (the window and the GPU starting up) do not eat the animation, which is why
//! a wall-clock version was over before it could be seen.

use crate::motion::{self, out_back, out_cubic, out_quint};
use crate::theme::{self, color};
use eframe::egui::{self, Color32, Id, Order, Pos2, Rect, Sense, Stroke, epaint::Mesh, pos2, vec2};

const GLOW: (f32, f32) = (0.0, 0.55);
const MARK: (f32, f32) = (0.05, 0.55);
/// The mark lands (its overshoot peaks) here: the flash, the rings and the embers start with it.
const IGNITE: f32 = 0.42;
const FLASH: (f32, f32) = (IGNITE, 0.75);
const RING_A: (f32, f32) = (IGNITE, 0.95);
const RING_B: (f32, f32) = (IGNITE + 0.06, 0.95);
/// First letter of the name, the gap between letters and each letter's length.
const NAME_AT: f32 = 0.30;
const NAME_STEP: f32 = 0.035;
const NAME_LEN: f32 = 0.38;
const NAME: &str = "PhoeCord";
/// When the iris starts opening, unless skipped earlier.
const HOLD_UNTIL: f32 = 0.95;
/// How long the iris takes to reach the corners.
const OUT: f32 = 0.45;
/// The mark and name fade out over the first part of the iris.
const ITEMS_OUT: f32 = 0.24;
/// The header comes in with the iris; the content a beat later and a little slower.
const HEADER_IN: (f32, f32) = (0.0, 0.32);
const CONTENT_IN: (f32, f32) = (0.05, 0.47);
/// The soft edge of the iris, px.
const FEATHER: f32 = 36.0;
const EMBERS: u32 = 40;
/// The longest step one frame may move the clock (two 60 Hz frames).
const MAX_STEP: f32 = 1.0 / 30.0;

fn ramp(t: f32, (from, to): (f32, f32)) -> f32 {
    ((t - from) / (to - from)).clamp(0.0, 1.0)
}

/// When the whole thing is over, counted from the start of the iris.
fn tail() -> f32 {
    OUT.max(CONTENT_IN.1)
}

#[derive(Clone, Copy)]
struct Clock {
    /// Splash time, seconds.
    t: f32,
    /// The frame that last moved it, so it moves once per frame.
    pass: u64,
    /// When the iris starts (moved earlier by a skip).
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

/// Seconds since the iris started (negative before it), or `None` when it is all over or motion
/// is reduced.
fn since_out(ctx: &egui::Context) -> Option<f32> {
    read(ctx).map(|c| c.t - c.out_at).filter(|s| *s < tail())
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
    let since = age - clock.out_at;
    if since >= tail() {
        ctx.data_mut(|d| d.insert_temp(clock_id(), clock));
        return;
    }
    ctx.request_repaint();
    let screen = ctx.content_rect();
    egui::Area::new(Id::new("splash-layer"))
        .order(Order::Foreground)
        .fixed_pos(screen.min)
        // Until the iris opens the layer takes clicks, so the click that skips it hits nothing
        // hidden under it; once the window shows through, clicks go to the window.
        .interactable(since < 0.0)
        .show(ctx, |ui| {
            let resp = ui.allocate_rect(screen, Sense::click());
            let skip =
                resp.clicked() || ui.input(|i| i.pointer.any_pressed() || !i.keys_down.is_empty());
            if skip && since < 0.0 {
                clock.out_at = age;
            }
            paint(ui.painter(), screen, age, age - clock.out_at);
        });
    ctx.data_mut(|d| d.insert_temp(clock_id(), clock));
}

fn paint(p: &egui::Painter, screen: Rect, age: f32, since: f32) {
    let c = screen.center() - vec2(0.0, 18.0);
    iris(p, screen, c, since);
    // The mark, name and light leave over the first part of the iris.
    let leave = out_cubic(ramp(since, (0.0, ITEMS_OUT)));
    let a = 1.0 - leave;

    let g = out_quint(ramp(age, GLOW));
    glow(
        p,
        c,
        60.0 + 120.0 * g,
        theme::fade(color::ACCENT, 0.42 * g * a),
    );

    let f = ramp(age, FLASH);
    if f > 0.0 && f < 1.0 {
        let bloom = (1.0 - f).powi(2);
        glow(
            p,
            c,
            90.0 + 150.0 * out_cubic(f),
            theme::fade(color::ROSE, 0.75 * bloom * a),
        );
        // A short white-hot core right where the flame catches.
        glow(
            p,
            c,
            70.0 + 40.0 * f,
            theme::fade(
                Color32::from_rgb(0xFF, 0xE4, 0xD6),
                0.45 * bloom * bloom * a,
            ),
        );
    }
    for (window, from, to, width, col) in [
        (RING_A, 50.0, 170.0, 2.0_f32, color::ROSE),
        (RING_B, 50.0, 120.0, 1.0_f32, color::ACCENT_HOVER),
    ] {
        let r = ramp(age, window);
        if r > 0.0 && r < 1.0 {
            let k = out_cubic(r);
            p.circle_stroke(
                c,
                from + (to - from) * k,
                Stroke::new(width, theme::fade(col, 0.55 * (1.0 - k) * a)),
            );
        }
    }
    embers(p, c, age, a);

    // The flame lifts into place, lands with a small overshoot and catches: garnet while it
    // rises, white once it is there; it grows a little as the iris opens.
    let m = ramp(age, MARK);
    let size = 88.0 * (0.82 + 0.18 * out_back(m)) * (1.0 + 0.12 * leave);
    let rise = 28.0 * (1.0 - out_quint(m));
    let rect = Rect::from_center_size(c + vec2(0.0, rise), vec2(size, size));
    let tint = lerp(
        color::ACCENT,
        Color32::WHITE,
        out_cubic(ramp(age, (0.15, IGNITE))),
    );
    theme::paint_mark_tinted(p, rect, tint.gamma_multiply(out_quint(m).min(1.0) * a));

    name(p, c, age, a);
}

/// The splash background with a round hole of the iris's radius at `c`, its edge feathered and
/// lit by a garnet rim. Before the iris starts it is a plain fill.
fn iris(p: &egui::Painter, screen: Rect, c: Pos2, since: f32) {
    if since <= 0.0 {
        p.rect_filled(screen, 0.0, color::BG);
        return;
    }
    let far = [
        screen.left_top(),
        screen.right_top(),
        screen.left_bottom(),
        screen.right_bottom(),
    ]
    .iter()
    .map(|q| q.distance(c))
    .fold(0.0, f32::max);
    let k = out_cubic(ramp(since, (0.0, OUT)));
    // A little past the corners at the end, so the feather leaves the screen too.
    let r = (far + FEATHER) * k;
    annulus(p, c, r, r + FEATHER, Color32::TRANSPARENT, color::BG);
    annulus(p, c, r + FEATHER, far + FEATHER * 2.0, color::BG, color::BG);
    let rim = 1.0 - k;
    annulus(
        p,
        c,
        r + FEATHER * 0.2,
        r + FEATHER * 0.9,
        theme::fade(color::ACCENT, 0.55 * rim),
        Color32::TRANSPARENT,
    );
    p.circle_stroke(
        c,
        r + FEATHER * 0.25,
        Stroke::new(1.5_f32, theme::fade(color::ROSE, 0.7 * rim)),
    );
}

/// Sparks that lift off the flame when it catches. Each has its own start, life, drift and size,
/// from a fixed hash, so the burst is the same on every start and never uniform.
fn embers(p: &egui::Painter, c: Pos2, age: f32, alpha: f32) {
    for i in 0..EMBERS {
        let h = |k: u32| hash(i * 16 + k);
        let start = IGNITE - 0.02 + 0.22 * h(1);
        let life = 0.42 + 0.28 * h(2);
        let t = age - start;
        if t <= 0.0 || t >= life {
            continue;
        }
        let u = t / life;
        let origin = c + vec2((h(3) - 0.5) * 56.0, 10.0 - h(4) * 30.0);
        let vx = (h(5) - 0.5) * 260.0;
        let vy = -(140.0 + 220.0 * h(6));
        // They slow down as they rise, like sparks over a fire.
        let pos = origin + vec2(vx * t * (1.0 - 0.3 * u), vy * t * (1.0 - 0.35 * u));
        let fade_in = (u / 0.12).min(1.0);
        let a = fade_in * (1.0 - u).powf(1.3) * alpha;
        let size = (1.4 + 2.8 * h(7)) * (1.0 - 0.55 * u);
        let col = lerp(color::ROSE, Color32::from_rgb(0xFF, 0xD2, 0xB8), h(8));
        p.circle_filled(pos, size * 3.6, theme::fade(color::ACCENT_HOVER, 0.38 * a));
        p.circle_filled(pos, size, theme::fade(col, a));
    }
}

/// The name, letter by letter: each one fades in and rises 10 px, 35 ms after the one before.
fn name(p: &egui::Painter, c: Pos2, age: f32, alpha: f32) {
    let font = theme::semibold(22.0);
    let whole = p.layout_no_wrap(NAME.into(), font.clone(), color::TEXT);
    let mut x = c.x - whole.size().x / 2.0;
    let base = c.y + 44.0 + 22.0;
    for (i, ch) in NAME.chars().enumerate() {
        let from = NAME_AT + NAME_STEP * i as f32;
        let n = out_quint(ramp(age, (from, from + NAME_LEN)));
        let g = p.layout_no_wrap(ch.to_string(), font.clone(), color::TEXT);
        let w = g.size().x;
        if n > 0.0 {
            let g = p.layout_no_wrap(
                ch.to_string(),
                font.clone(),
                theme::fade(color::TEXT, n * alpha),
            );
            p.galley(pos2(x, base + 10.0 * (1.0 - n)), g, color::TEXT);
        }
        x += w;
    }
}

/// A ring between radii `r0` and `r1`, coloured `c0` on the inside and `c1` on the outside.
fn annulus(p: &egui::Painter, c: Pos2, r0: f32, r1: f32, c0: Color32, c1: Color32) {
    let mut mesh = Mesh::default();
    let steps = 96u32;
    for i in 0..=steps {
        let a = i as f32 / steps as f32 * std::f32::consts::TAU;
        let d = vec2(a.cos(), a.sin());
        mesh.colored_vertex(c + d * r0, c0);
        mesh.colored_vertex(c + d * r1, c1);
    }
    for i in 0..steps {
        let k = 2 * i;
        mesh.add_triangle(k, k + 1, k + 2);
        mesh.add_triangle(k + 1, k + 3, k + 2);
    }
    p.add(mesh);
}

/// A number in 0..1 from `n`, the same on every run.
fn hash(n: u32) -> f32 {
    let mut x = n.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297A_2D39);
    x ^= x >> 15;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let ch = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(ch(a.r(), b.r()), ch(a.g(), b.g()), ch(a.b(), b.b()))
}

/// A soft round glow: a mesh fan from `col` in the middle to transparent at `radius`.
fn glow(p: &egui::Painter, c: Pos2, radius: f32, col: Color32) {
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
        assert!((ramp(0.30, MARK) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn fits_the_owner_s_length() {
        // The whole thing, intro and iris, stays within 1–1.5 s.
        let total = HOLD_UNTIL + tail();
        assert!((1.0..=1.5).contains(&total), "{total}");
        // Everything in the intro has landed before the iris opens.
        let last_letter = NAME_AT + NAME_STEP * (NAME.len() - 1) as f32 + NAME_LEN;
        let last_ember = IGNITE - 0.02 + 0.22 + 0.42 + 0.28;
        for end in [GLOW.1, MARK.1, FLASH.1, RING_A.1, RING_B.1, last_letter] {
            assert!(end <= HOLD_UNTIL, "{end}");
        }
        // Embers may still be going out while the iris opens, but not after it.
        assert!(last_ember <= HOLD_UNTIL + OUT, "{last_ember}");
    }

    #[test]
    fn embers_are_spread_not_uniform() {
        let v: Vec<f32> = (0..64).map(hash).collect();
        assert!(v.iter().all(|x| (0.0..1.0).contains(x)));
        let mean = v.iter().sum::<f32>() / v.len() as f32;
        assert!((0.35..0.65).contains(&mean), "{mean}");
        assert!(v.windows(2).all(|w| w[0] != w[1]));
    }

    #[test]
    fn colours_blend_end_to_end() {
        assert_eq!(lerp(color::ACCENT, Color32::WHITE, 0.0), color::ACCENT);
        assert_eq!(lerp(color::ACCENT, Color32::WHITE, 1.0), Color32::WHITE);
    }
}
