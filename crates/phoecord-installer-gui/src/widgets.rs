//! Our own widgets, painted by hand so hover, press, focus and state changes animate. Every one
//! is keyboard reachable (Tab, Enter / Space) and reports itself to screen readers.

use crate::motion::{self, dur};
use crate::theme::{self, color};
use eframe::egui::{
    self, Align2, Color32, Id, Pos2, Rect, Response, RichText, Sense, StrokeKind, Ui, Vec2,
    WidgetInfo, WidgetType, pos2, vec2,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The one main action of a screen.
    Primary,
    Secondary,
    /// Text-only, for actions next to a main one.
    Ghost,
    /// Ghost that turns red on hover: remove, uninstall.
    Danger,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Normal,
    Big,
}

pub struct Button<'a> {
    pub label: &'a str,
    pub icon: Option<&'a str>,
    pub kind: Kind,
    pub size: Size,
    pub enabled: bool,
    /// Show a spinner instead of the icon (the action is running).
    pub busy: bool,
}

impl<'a> Button<'a> {
    pub fn new(label: &'a str, kind: Kind) -> Self {
        Button {
            label,
            icon: None,
            kind,
            size: Size::Normal,
            enabled: true,
            busy: false,
        }
    }
    pub fn icon(mut self, i: &'a str) -> Self {
        self.icon = Some(i);
        self
    }
    pub fn big(mut self) -> Self {
        self.size = Size::Big;
        self
    }
    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }
    pub fn busy(mut self, on: bool) -> Self {
        self.busy = on;
        self
    }
    pub fn show(self, ui: &mut Ui) -> Response {
        let (h, font, pad) = match self.size {
            Size::Normal => (36.0, theme::medium(theme::T_BODY), theme::S4),
            Size::Big => (48.0, theme::semibold(theme::T_BUTTON_BIG), 26.0),
        };
        // Text-only buttons sit closer to their neighbours.
        let pad = if matches!(self.kind, Kind::Ghost | Kind::Danger) {
            pad.min(16.0)
        } else {
            pad
        };
        let fg_base = match self.kind {
            Kind::Primary => Color32::WHITE,
            _ => color::TEXT,
        };
        let galley = ui
            .painter()
            .layout_no_wrap(self.label.to_string(), font.clone(), fg_base);
        let icon_w = if self.icon.is_some() || self.busy {
            font.size + 2.0 + theme::S2
        } else {
            0.0
        };
        let w = galley.size().x + icon_w + pad * 2.0;
        let sense = if self.enabled && !self.busy {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, resp) = ui.allocate_exact_size(vec2(w, h), sense);
        resp.widget_info(|| {
            WidgetInfo::labeled(WidgetType::Button, self.enabled && !self.busy, self.label)
        });
        if !ui.is_rect_visible(rect) {
            return resp;
        }
        let ctx = ui.ctx();
        let hover = motion::tween(
            ctx,
            resp.id.with("h"),
            resp.hovered() as u8 as f32,
            dur::HOVER,
            motion::out_cubic,
        );
        let down = resp.is_pointer_button_down_on();
        let press = motion::tween(
            ctx,
            resp.id.with("p"),
            down as u8 as f32,
            if down { 0.06 } else { dur::SMALL },
            motion::out_cubic,
        );
        let live = self.enabled && !self.busy;
        let (bg, stroke, fg) = match self.kind {
            Kind::Primary => {
                let base = theme::lerp_color(color::ACCENT, color::ACCENT_HOVER, hover);
                let bg = theme::lerp_color(base, color::ACCENT_PRESS, press);
                (bg, Color32::TRANSPARENT, Color32::WHITE)
            }
            Kind::Secondary => (
                theme::lerp_color(color::RAISED, color::RAISED_HOVER, hover),
                theme::lerp_color(color::LINE, color::LINE_STRONG, hover),
                color::TEXT,
            ),
            Kind::Ghost => (
                theme::fade(color::RAISED, hover),
                Color32::TRANSPARENT,
                theme::lerp_color(color::MUTED, color::TEXT, hover),
            ),
            Kind::Danger => (
                theme::fade(color::DANGER_BG, hover),
                Color32::TRANSPARENT,
                theme::lerp_color(color::MUTED, color::DANGER, hover),
            ),
        };
        let (bg, fg) = if live {
            (bg, fg)
        } else {
            (
                if self.kind == Kind::Primary {
                    theme::fade(color::ACCENT, 0.45)
                } else {
                    bg
                },
                if self.kind == Kind::Primary {
                    theme::fade(Color32::WHITE, 0.6)
                } else {
                    color::FAINT
                },
            )
        };
        // Pressed buttons sink a little (2 % smaller), like a physical key.
        let r = scale_rect(rect, 1.0 - 0.025 * press);
        let p = ui.painter();
        p.rect(
            r,
            theme::radius(theme::R_CONTROL),
            bg,
            theme::stroke(1.0, stroke),
            StrokeKind::Inside,
        );
        let content_w = galley.size().x + icon_w;
        let mut x = r.center().x - content_w / 2.0;
        if self.busy {
            spinner(
                p,
                pos2(x + font.size / 2.0, r.center().y),
                font.size * 0.42,
                fg,
                ctx,
            );
            x += icon_w;
        } else if let Some(i) = self.icon {
            p.text(
                pos2(x, r.center().y),
                Align2::LEFT_CENTER,
                i,
                theme::icon(font.size + 2.0),
                fg,
            );
            x += icon_w;
        }
        p.galley_with_override_text_color(
            pos2(x, r.center().y - galley.size().y / 2.0),
            galley,
            fg,
        );
        focus_ring(ui, &resp, rect, theme::R_CONTROL);
        resp
    }
}

pub fn scale_rect(r: Rect, s: f32) -> Rect {
    Rect::from_center_size(r.center(), r.size() * s)
}

/// A 2 px rose outline around a widget that has keyboard focus.
pub fn focus_ring(ui: &Ui, resp: &Response, rect: Rect, r: u8) {
    if resp.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(3.0),
            theme::radius(r + 3),
            theme::stroke(2.0, color::ROSE),
            StrokeKind::Outside,
        );
    }
}

/// Three quarters of a ring turning once a second.
pub fn spinner(p: &egui::Painter, c: Pos2, r: f32, col: Color32, ctx: &egui::Context) {
    let t = ctx.input(|i| i.time) as f32;
    let start = if motion::reduced(ctx) {
        0.0
    } else {
        t * std::f32::consts::TAU
    };
    let n = 24;
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| {
            let a = start + (i as f32 / n as f32) * std::f32::consts::TAU * 0.75;
            c + vec2(a.cos(), a.sin()) * r
        })
        .collect();
    p.add(egui::Shape::line(pts, theme::stroke(r * 0.32, col)));
    ctx.request_repaint();
}

/// A check mark that draws itself as `t` goes 0 → 1.
pub fn check_mark(p: &egui::Painter, rect: Rect, t: f32, col: Color32, width: f32) {
    let a = rect.lerp_inside(vec2(0.22, 0.52));
    let b = rect.lerp_inside(vec2(0.42, 0.72));
    let c = rect.lerp_inside(vec2(0.78, 0.30));
    let l1 = (b - a).length();
    let l2 = (c - b).length();
    let d = (l1 + l2) * t.clamp(0.0, 1.0);
    let mut pts = vec![a];
    if d <= l1 {
        pts.push(a + (b - a) * (d / l1));
    } else {
        pts.push(b);
        pts.push(b + (c - b) * ((d - l1) / l2));
    }
    p.add(egui::Shape::line(pts, theme::stroke(width, col)));
}

/// An X drawn in two strokes.
pub fn cross(p: &egui::Painter, rect: Rect, col: Color32, width: f32) {
    let r = rect.shrink(rect.width() * 0.3);
    p.line_segment([r.left_top(), r.right_bottom()], theme::stroke(width, col));
    p.line_segment([r.right_top(), r.left_bottom()], theme::stroke(width, col));
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Ok,
    Warn,
    Danger,
    Neutral,
}

impl Tone {
    pub fn colors(self) -> (Color32, Color32) {
        match self {
            Tone::Ok => (color::OK, color::OK_BG),
            Tone::Warn => (color::WARN, color::WARN_BG),
            Tone::Danger => (color::DANGER, color::DANGER_BG),
            Tone::Neutral => (color::MUTED, color::NEUTRAL_BG),
        }
    }
}

/// A small status pill. Colour changes cross-fade.
pub fn pill(ui: &mut Ui, id: Id, text: &str, tone: Tone) -> Response {
    let font = theme::medium(theme::T_CAPTION);
    let (fg, bg) = tone.colors();
    let ctx = ui.ctx().clone();
    let anim = |c: Color32, k: &str| {
        let r = motion::tween(
            &ctx,
            id.with(k).with(0),
            c.r() as f32,
            dur::SMALL,
            motion::out_cubic,
        );
        let g = motion::tween(
            &ctx,
            id.with(k).with(1),
            c.g() as f32,
            dur::SMALL,
            motion::out_cubic,
        );
        let b = motion::tween(
            &ctx,
            id.with(k).with(2),
            c.b() as f32,
            dur::SMALL,
            motion::out_cubic,
        );
        Color32::from_rgb(r as u8, g as u8, b as u8)
    };
    let (fg, bg) = (anim(fg, "fg"), anim(bg, "bg"));
    let g = ui.painter().layout_no_wrap(text.to_string(), font, fg);
    let size = vec2(g.size().x + 18.0, 22.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, text));
    let p = ui.painter();
    p.rect_filled(rect, theme::radius(11), bg);
    p.galley(
        pos2(rect.left() + 9.0, rect.center().y - g.size().y / 2.0),
        g,
        fg,
    );
    resp
}

/// An on/off switch. The knob slides with a light overshoot; the track colour follows.
pub fn switch(ui: &mut Ui, on: bool, label: &str) -> Response {
    let size = vec2(40.0, 24.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, on, label));
    if resp.clicked() {
        resp.mark_changed();
    }
    let ctx = ui.ctx();
    let t = motion::tween(
        ctx,
        resp.id.with("k"),
        on as u8 as f32,
        dur::SMALL,
        motion::out_back,
    );
    let hover = motion::tween(
        ctx,
        resp.id.with("h"),
        resp.hovered() as u8 as f32,
        dur::HOVER,
        motion::out_cubic,
    );
    let tc = t.clamp(0.0, 1.0);
    let off_track = theme::lerp_color(color::RAISED, color::RAISED_HOVER, hover);
    let track = theme::lerp_color(off_track, color::ACCENT, tc);
    let p = ui.painter();
    p.rect(
        rect,
        theme::radius(12),
        track,
        theme::stroke(
            1.0,
            theme::lerp_color(color::LINE_STRONG, color::ACCENT, tc),
        ),
        StrokeKind::Inside,
    );
    let r = 9.0;
    let x = egui::lerp((rect.left() + 3.0 + r)..=(rect.right() - 3.0 - r), t);
    let knob = theme::lerp_color(color::MUTED, Color32::WHITE, tc);
    // The knob stretches a little while pressed.
    let w = if resp.is_pointer_button_down_on() {
        r + 2.0
    } else {
        r
    };
    p.rect_filled(
        Rect::from_center_size(pos2(x, rect.center().y), vec2(w * 2.0, r * 2.0)),
        theme::radius(9),
        knob,
    );
    focus_ring(ui, &resp, rect, 12);
    resp
}

/// Segmented tabs: the selected one sits on a raised pill that slides between them, with a rose
/// underline. Returns the newly picked index when the user changes it.
pub fn tabs(ui: &mut Ui, id: Id, labels: &[String], selected: usize, pad: f32) -> Option<usize> {
    let font = theme::medium(theme::T_BODY);
    let h = 30.0;
    let widths: Vec<f32> = labels
        .iter()
        .map(|l| {
            ui.painter()
                .layout_no_wrap(l.clone(), font.clone(), color::TEXT)
                .size()
                .x
                + pad * 2.0
        })
        .collect();
    let total = widths.iter().sum::<f32>() + 6.0;
    let (outer, _) = ui.allocate_exact_size(vec2(total, h + 6.0), Sense::hover());
    let p = ui.painter().clone();
    p.rect(
        outer,
        theme::radius(10),
        color::SURFACE,
        theme::stroke(1.0, color::LINE),
        StrokeKind::Inside,
    );
    let mut x = outer.left() + 3.0;
    let mut rects = Vec::new();
    for w in &widths {
        rects.push(Rect::from_min_size(pos2(x, outer.top() + 3.0), vec2(*w, h)));
        x += w;
    }
    let ctx = ui.ctx().clone();
    let target = rects[selected.min(rects.len() - 1)];
    let l = motion::tween(
        &ctx,
        id.with("l"),
        target.left(),
        dur::TAB,
        motion::in_out_cubic,
    );
    let r = motion::tween(
        &ctx,
        id.with("r"),
        target.right(),
        dur::TAB,
        motion::in_out_cubic,
    );
    let ind = Rect::from_x_y_ranges(l..=r, target.y_range());
    p.rect(
        ind,
        theme::radius(7),
        color::RAISED,
        theme::stroke(1.0, color::LINE_STRONG),
        StrokeKind::Inside,
    );
    let line = Rect::from_x_y_ranges(
        (l + pad - 2.0)..=(r - pad + 2.0),
        (ind.bottom() - 1.5)..=(ind.bottom() + 0.5),
    );
    p.rect_filled(line, theme::radius(1), color::ROSE);
    let mut picked = None;
    for (i, (rect, label)) in rects.iter().zip(labels).enumerate() {
        let resp = ui.interact(*rect, id.with(i), Sense::click());
        resp.widget_info(|| {
            WidgetInfo::selected(WidgetType::SelectableLabel, true, i == selected, label)
        });
        if resp.clicked() && i != selected {
            picked = Some(i);
        }
        let on = motion::tween(
            &ctx,
            id.with(("t", i)),
            (i == selected) as u8 as f32,
            dur::TAB,
            motion::out_cubic,
        );
        let hov = motion::tween(
            &ctx,
            id.with(("h", i)),
            resp.hovered() as u8 as f32,
            dur::HOVER,
            motion::out_cubic,
        );
        let col = theme::lerp_color(
            theme::lerp_color(color::MUTED, color::TEXT, hov * 0.6),
            color::TEXT,
            on,
        );
        p.text(
            rect.center(),
            Align2::CENTER_CENTER,
            label,
            font.clone(),
            col,
        );
        focus_ring(ui, &resp, *rect, 7);
    }
    // Left / right arrows move between tabs when one has focus.
    let focused = (0..labels.len()).any(|i| ctx.memory(|m| m.has_focus(id.with(i))));
    if focused {
        let (left, right) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
            )
        });
        if left && selected > 0 {
            picked = Some(selected - 1);
        }
        if right && selected + 1 < labels.len() {
            picked = Some(selected + 1);
        }
        if let Some(n) = picked {
            ctx.memory_mut(|m| m.request_focus(id.with(n)));
        }
    }
    picked
}

/// Text in one of the theme fonts.
pub fn text(ui: &mut Ui, s: impl Into<String>, font: egui::FontId, col: Color32) -> Response {
    ui.add(egui::Label::new(RichText::new(s).font(font).color(col)).wrap())
}

pub fn h1(ui: &mut Ui, s: impl Into<String>) -> Response {
    text(ui, s, theme::semibold(theme::T_H1), color::TEXT)
}

pub fn title(ui: &mut Ui, s: impl Into<String>) -> Response {
    text(ui, s, theme::semibold(theme::T_TITLE), color::TEXT)
}

pub fn body(ui: &mut Ui, s: impl Into<String>) -> Response {
    text(ui, s, theme::regular(theme::T_BODY), color::TEXT)
}

pub fn muted(ui: &mut Ui, s: impl Into<String>) -> Response {
    text(ui, s, theme::regular(theme::T_BODY), color::MUTED)
}

pub fn caption(ui: &mut Ui, s: impl Into<String>) -> Response {
    text(ui, s, theme::regular(theme::T_CAPTION), color::MUTED)
}

/// A square icon-only button (with a tooltip and a screen-reader label).
pub fn icon_button(ui: &mut Ui, icon: &str, label: &str, danger: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::click());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    let ctx = ui.ctx();
    let hover = motion::tween(
        ctx,
        resp.id.with("h"),
        resp.hovered() as u8 as f32,
        dur::HOVER,
        motion::out_cubic,
    );
    let down = resp.is_pointer_button_down_on() as u8 as f32;
    let press = motion::tween(ctx, resp.id.with("p"), down, 0.08, motion::out_cubic);
    let (bg, fg) = if danger {
        (
            theme::fade(color::DANGER_BG, hover),
            theme::lerp_color(color::MUTED, color::DANGER, hover),
        )
    } else {
        (
            theme::fade(color::RAISED, hover),
            theme::lerp_color(color::MUTED, color::TEXT, hover),
        )
    };
    let r = scale_rect(rect, 1.0 - 0.06 * press);
    ui.painter()
        .rect_filled(r, theme::radius(theme::R_CONTROL), bg);
    ui.painter().text(
        r.center(),
        Align2::CENTER_CENTER,
        icon,
        theme::icon(18.0),
        fg,
    );
    focus_ring(ui, &resp, rect, theme::R_CONTROL);
    resp.on_hover_text(label)
}

/// A full-width notice: icon, title, text, and an optional action button on the right.
pub fn banner(
    ui: &mut Ui,
    tone: Tone,
    icon: &str,
    title: &str,
    body: &str,
    action: Option<&str>,
) -> bool {
    let (fg, bg) = tone.colors();
    let mut clicked = false;
    egui::Frame::new()
        .fill(bg)
        .corner_radius(theme::radius(theme::R_TILE))
        .inner_margin(egui::Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.add_space(0.0);
                ui.label(RichText::new(icon).font(theme::icon(20.0)).color(fg));
                ui.add_space(theme::S2);
                let text_w = ui.available_width() - if action.is_some() { 150.0 } else { 0.0 };
                ui.vertical(|ui| {
                    ui.set_max_width(text_w.max(120.0));
                    text(ui, title, theme::semibold(theme::T_BODY), color::TEXT);
                    text(ui, body, theme::regular(theme::T_CAPTION), color::MUTED);
                });
                if let Some(a) = action {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        clicked = Button::new(a, Kind::Secondary).show(ui).clicked();
                    });
                }
            });
        });
    clicked
}

/// A one-line text field in the theme, with a label above it.
pub fn field(ui: &mut Ui, label: &str, value: &mut String, hint: &str) -> Response {
    text(ui, label, theme::medium(theme::T_CAPTION), color::MUTED);
    ui.add_space(2.0);
    let resp = ui.add(
        egui::TextEdit::singleline(value)
            .hint_text(RichText::new(hint).color(color::FAINT))
            .font(theme::regular(theme::T_BODY))
            .margin(egui::Margin::symmetric(12, 9))
            .desired_width(ui.available_width()),
    );
    if resp.has_focus() {
        ui.painter().rect_stroke(
            resp.rect,
            theme::radius(theme::R_CONTROL),
            theme::stroke(1.5, color::ROSE),
            StrokeKind::Inside,
        );
    }
    resp
}
