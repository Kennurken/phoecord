//! Motion: interruptible tweens with our own easing curves, and the durations used everywhere.
//!
//! A tween is keyed by an [`Id`]. Asking for a new target while one is running starts from the
//! current value (no jump), so every animation can be interrupted by the next input. Nothing here
//! blocks input: values are computed per frame, and a repaint is requested only while something
//! moves. With "Reduce motion" on (Settings) every tween returns its target at once.

use eframe::egui::{Context, Id};

/// Durations, seconds. Exits are faster than entrances.
pub mod dur {
    /// Hover and press colour changes.
    pub const HOVER: f32 = 0.12;
    /// Switch knobs, radio dots, small state changes.
    pub const SMALL: f32 = 0.18;
    /// The tab indicator and tab content.
    pub const TAB: f32 = 0.26;
    /// Dialogs and toasts coming in.
    pub const ENTER: f32 = 0.22;
    /// Dialogs and toasts going away.
    pub const EXIT: f32 = 0.14;
    /// A list row appearing or collapsing.
    pub const ROW: f32 = 0.24;
    /// A check mark drawing itself.
    pub const CHECK: f32 = 0.32;
}

pub type Ease = fn(f32) -> f32;

#[cfg(test)]
pub fn linear(t: f32) -> f32 {
    t
}

/// Fast start, long soft landing: things arriving or answering a click.
pub fn out_quint(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(5)
}

pub fn out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

/// Things moving from one place to another on screen (the tab indicator).
pub fn in_out_cubic(t: f32) -> f32 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// A light overshoot (about 4 %), for switch knobs and the selected radio dot: reads as a spring
/// without a physics step.
pub fn out_back(t: f32) -> f32 {
    let c1 = 1.1;
    let c3 = c1 + 1.0;
    1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
}

/// Things leaving.
pub fn in_cubic(t: f32) -> f32 {
    t * t * t
}

#[derive(Clone, Copy, Debug)]
struct Tween {
    from: f32,
    to: f32,
    start: f64,
    dur: f32,
}

impl Tween {
    fn progress(&self, now: f64) -> f32 {
        if self.dur <= 0.0 {
            1.0
        } else {
            (((now - self.start) as f32) / self.dur).clamp(0.0, 1.0)
        }
    }

    fn sample(&self, now: f64, ease: Ease) -> f32 {
        self.from + (self.to - self.from) * ease(self.progress(now))
    }
}

fn reduce_id() -> Id {
    Id::new("phoecord-reduce-motion")
}

/// Turn every animation off (or back on).
pub fn set_reduced(ctx: &Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(reduce_id(), on));
}

pub fn reduced(ctx: &Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(reduce_id()))
        .unwrap_or(false)
}

/// The value of the tween `id` heading for `target`. A tween seen for the first time starts at
/// `target` (no animation on first paint).
pub fn tween(ctx: &Context, id: Id, target: f32, dur: f32, ease: Ease) -> f32 {
    tween_from(ctx, id, target, target, dur, ease)
}

/// [`tween`], but a tween seen for the first time starts at `initial` and animates to `target`
/// (rows and dialogs appearing).
pub fn tween_from(ctx: &Context, id: Id, initial: f32, target: f32, dur: f32, ease: Ease) -> f32 {
    let now = ctx.input(|i| i.time);
    let reduce = reduced(ctx);
    let stored = ctx.data(|d| d.get_temp::<Tween>(id));
    let tw = match stored {
        Some(t) if t.to == target => t,
        Some(t) => Tween {
            from: t.sample(now, ease),
            to: target,
            start: now,
            dur,
        },
        None => Tween {
            from: initial,
            to: target,
            start: now,
            dur,
        },
    };
    if stored.is_none_or(|s| s.to != target) {
        ctx.data_mut(|d| d.insert_temp(id, tw));
    }
    if reduce {
        return target;
    }
    let p = tw.progress(now);
    if p < 1.0 {
        ctx.request_repaint();
    }
    tw.sample(now, ease)
}

/// 0 → 1 while `on`, 1 → 0 when it goes off. `enter` and `exit` are the two durations.
pub fn presence(ctx: &Context, id: Id, on: bool, enter: f32, exit: f32) -> f32 {
    let (target, d, e): (f32, f32, Ease) = if on {
        (1.0, enter, out_quint)
    } else {
        (0.0, exit, in_cubic)
    };
    tween_from(ctx, id, 0.0, target, d, e)
}

/// Seconds since the first frame on which `id` was asked about (for one-shot effects such as a
/// check mark drawing itself). Forget it with [`forget`].
pub fn age(ctx: &Context, id: Id) -> f32 {
    let now = ctx.input(|i| i.time);
    let start = ctx.data_mut(|d| *d.get_temp_mut_or_insert_with(id, || now));
    let a = (now - start) as f32;
    if reduced(ctx) { f32::MAX } else { a }
}

pub fn forget(ctx: &Context, id: Id) {
    ctx.data_mut(|d| {
        d.remove::<f64>(id);
        d.remove::<Tween>(id);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_start_at_0_and_end_at_1() {
        for f in [
            linear,
            out_quint,
            out_cubic,
            in_out_cubic,
            out_back,
            in_cubic,
        ] {
            assert!(f(0.0).abs() < 1e-5);
            assert!((f(1.0) - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn out_back_overshoots_a_little() {
        let peak = (0..=100)
            .map(|i| out_back(i as f32 / 100.0))
            .fold(0.0, f32::max);
        assert!(peak > 1.0 && peak < 1.08, "{peak}");
    }

    #[test]
    fn retarget_starts_from_the_current_value() {
        let tw = Tween {
            from: 0.0,
            to: 100.0,
            start: 0.0,
            dur: 1.0,
        };
        let mid = tw.sample(0.5, linear);
        assert!((mid - 50.0).abs() < 1e-3);
        assert_eq!(tw.sample(2.0, linear), 100.0);
    }

    #[test]
    fn tween_settles_on_target_and_reduce_skips() {
        let ctx = Context::default();
        let id = Id::new("t");
        let _ = ctx.run(Default::default(), |ctx| {
            assert_eq!(tween(ctx, id, 5.0, 0.2, linear), 5.0);
            // A new target animates from 5.
            let v = tween(ctx, id, 10.0, 0.2, linear);
            assert!((5.0..=10.0).contains(&v));
            set_reduced(ctx, true);
            assert_eq!(tween(ctx, id, 20.0, 0.2, linear), 20.0);
        });
    }
}
