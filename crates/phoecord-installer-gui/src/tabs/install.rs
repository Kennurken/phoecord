//! The Install tab: pick a Discord, one main action, and the step-by-step progress of an install.

use crate::app::{App, Dialog, Flow, PState, button_row};
use crate::backend::ClientStatus;
use crate::motion::{self, dur};
use crate::theme::{self, color};
use crate::widgets::{self, Button, Kind, Tone};
use eframe::egui::{self, Align2, Id, Rect, Sense, StrokeKind, pos2, vec2};
use egui_phosphor::regular as icon;
use phoecord_installer::Channel;
use phoecord_installer::install::Phase;
use phoecord_installer::system::Os;

impl App {
    pub(crate) fn install_tab(&mut self, ui: &mut egui::Ui) {
        match &self.flow {
            Flow::Idle => self.install_idle(ui),
            Flow::Installing { .. } | Flow::Failed { .. } => self.install_progress(ui),
            Flow::Ready { .. } => self.install_ready(ui),
            Flow::Uninstalling | Flow::Removed(_) | Flow::RemoveFailed(_) => {
                self.uninstall_view(ui)
            }
        }
    }

    pub(crate) fn start_install(&mut self, channel: Option<Channel>, repair: bool) {
        let ctx = self.backend.ctx_clone();
        for i in 0..3 {
            motion::forget(&ctx, Id::new(("phase-done", i)));
        }
        motion::forget(&ctx, Id::new("ready-burst"));
        self.flow = Flow::Installing {
            channel,
            repair,
            phases: [PState::Waiting; 3],
        };
        self.backend.install(channel);
    }

    fn install_idle(&mut self, ui: &mut egui::Ui) {
        let Some(status) = self.status.clone() else {
            self.skeleton(ui);
            return;
        };
        let ctx = ui.ctx().clone();
        if !status.proxy_exe_found {
            widgets::banner(
                ui,
                Tone::Danger,
                icon::WARNING_CIRCLE,
                &self.tr.t("gui-missing-binary-title"),
                &self.tr.f(
                    "gui-missing-binary",
                    &[("path", &status.proxy_exe.display().to_string())],
                ),
                None,
            );
            ui.add_space(theme::S5);
        } else if status.needs_repair()
            && widgets::banner(
                ui,
                Tone::Warn,
                icon::WRENCH,
                &self.tr.t("gui-repair-banner-title"),
                &self.tr.t("gui-repair-banner"),
                Some(&self.tr.t("gui-repair")),
            )
        {
            self.start_install(self.selected.filter(|c| status.is_on(*c)), true);
            return;
        }
        if status.needs_repair() {
            ui.add_space(theme::S5);
        }

        let installed = status.installed;
        widgets::h1(
            ui,
            self.tr.t(if installed {
                "gui-install-title-on"
            } else {
                "gui-install-title"
            }),
        );
        ui.add_space(theme::S1);
        ui.scope(|ui| {
            ui.set_max_width(560.0f32.min(ui.available_width()));
            widgets::muted(
                ui,
                self.tr.t(if installed {
                    "gui-install-sub-on"
                } else {
                    "gui-install-sub"
                }),
            );
        });
        ui.add_space(theme::S5);

        if status.clients.is_empty() {
            self.no_clients(ui, installed);
        } else {
            self.client_tiles(ui, &status.clients, &status);
            ui.add_space(theme::S5);
            let sel = self.selected;
            let sel_name = sel.map(|c| self.channel_name(c)).unwrap_or_default();
            let on = sel.is_some_and(|c| status.is_on(c));
            let main = if !installed {
                self.tr.t("gui-install-action")
            } else if on {
                self.tr.f("gui-open-action", &[("name", &sel_name)])
            } else {
                self.tr.f("gui-add-action", &[("name", &sel_name)])
            };
            let main_icon = if on {
                icon::PLAY
            } else {
                icon::DOWNLOAD_SIMPLE
            };
            let ready = status.proxy_exe_found && sel.is_some();
            let mut buttons = vec![
                Button::new(&main, Kind::Primary)
                    .icon(main_icon)
                    .big()
                    .busy(on && self.launching)
                    .enabled(ready),
            ];
            let repair = self.tr.t("gui-repair");
            let uninstall = self.tr.t("gui-uninstall");
            if installed {
                buttons.push(Button::new(&repair, Kind::Ghost).icon(icon::WRENCH).big());
                buttons.push(
                    Button::new(&uninstall, Kind::Danger)
                        .icon(icon::TRASH)
                        .big(),
                );
            }
            match button_row(ui, buttons) {
                Some(0) if on => {
                    if let Some(c) = sel {
                        self.open_discord(c);
                    }
                }
                Some(0) => self.start_install(sel, false),
                Some(1) => self.start_install(sel.filter(|c| status.is_on(*c)), true),
                Some(2) => self.open_dialog(Dialog::Uninstall),
                _ => {}
            }
            ui.add_space(theme::S2);
            let hint = if !on {
                "gui-install-hint"
            } else if Os::current() == Os::MacOs {
                "gui-open-hint-mac"
            } else {
                "gui-open-hint"
            };
            widgets::caption(ui, self.tr.t(hint));
            ui.add_space(theme::S4);
            self.elsewhere_row(ui);
        }
        ui.add_space(theme::S7);
        self.facts(ui);
        let _ = ctx;
    }

    /// Loading: the shape of the tiles, shimmering.
    fn skeleton(&self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let w = ui.available_width();
        let t = if motion::reduced(&ctx) {
            0.0
        } else {
            (ctx.input(|i| i.time) as f32 * 0.8).fract()
        };
        let bar = |ui: &mut egui::Ui, size: egui::Vec2| {
            let (r, _) = ui.allocate_exact_size(size, Sense::hover());
            ui.painter()
                .rect_filled(r, theme::radius(theme::R_CONTROL), color::SURFACE);
            let sx = r.left() + (r.width() + 160.0) * t - 160.0;
            let band =
                Rect::from_x_y_ranges(sx.max(r.left())..=(sx + 160.0).min(r.right()), r.y_range());
            if band.width() > 0.0 {
                ui.painter().rect_filled(
                    band,
                    theme::radius(theme::R_CONTROL),
                    theme::fade(color::RAISED, 0.8),
                );
            }
        };
        bar(ui, vec2(w * 0.55, 34.0));
        ui.add_space(theme::S2);
        bar(ui, vec2(w * 0.7, 18.0));
        ui.add_space(theme::S5);
        ui.horizontal(|ui| {
            let tw = (w - theme::S3) / 2.0;
            bar(ui, vec2(tw, 118.0));
            ui.add_space(theme::S3 - ui.spacing().item_spacing.x);
            bar(ui, vec2(tw, 118.0));
        });
        ctx.request_repaint();
    }

    pub(crate) fn no_clients(&mut self, ui: &mut egui::Ui, installed: bool) {
        egui::Frame::new()
            .fill(color::SURFACE)
            .stroke(theme::stroke(1.0, color::LINE))
            .corner_radius(theme::radius(theme::R_TILE))
            .inner_margin(egui::Margin::same(24))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    egui::RichText::new(icon::MAGNIFYING_GLASS)
                        .font(theme::icon(28.0))
                        .color(color::MUTED),
                );
                ui.add_space(theme::S2);
                widgets::title(ui, self.tr.t("gui-no-clients-title"));
                widgets::muted(ui, self.tr.t("gui-no-clients"));
                ui.add_space(theme::S3);
                widgets::caption(ui, self.tr.t("gui-no-clients-looked"));
                ui.add_space(theme::S1);
                for place in looked_in() {
                    ui.label(
                        egui::RichText::new(place)
                            .font(egui::FontId::monospace(theme::T_CAPTION))
                            .color(color::MUTED),
                    );
                }
                ui.add_space(theme::S4);
                let choose = self.tr.t("gui-choose-folder");
                let download = self.tr.t("gui-download-discord");
                let again = self.tr.t("gui-check-again");
                let anyway = self.tr.t("gui-install-anyway");
                let mut b = vec![
                    Button::new(&choose, Kind::Primary).icon(icon::FOLDER_OPEN),
                    Button::new(&download, Kind::Secondary).icon(icon::ARROW_SQUARE_OUT),
                    Button::new(&again, Kind::Ghost).icon(icon::ARROWS_CLOCKWISE),
                ];
                if !installed {
                    b.push(Button::new(&anyway, Kind::Ghost));
                }
                match button_row(ui, b) {
                    Some(0) => self.choose_client_folder(ui.ctx()),
                    Some(1) => ui.ctx().open_url(egui::OpenUrl::new_tab(DISCORD_DOWNLOAD)),
                    Some(2) => self.backend.refresh(),
                    Some(3) => self.start_install(None, false),
                    _ => {}
                }
            });
    }

    /// "Discord installed somewhere else? [Choose folder…]", the caption centred on the button.
    pub(crate) fn elsewhere_row(&mut self, ui: &mut egui::Ui) {
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), 36.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_height(36.0);
                widgets::caption(ui, self.tr.t("gui-elsewhere"));
                let choose = self.tr.t("gui-choose-folder");
                if Button::new(&choose, Kind::Ghost)
                    .icon(icon::FOLDER_OPEN)
                    .show(ui)
                    .clicked()
                {
                    self.choose_client_folder(ui.ctx());
                }
            },
        );
    }

    /// "Choose folder…": a Discord installed outside the default folders. The folder is kept in
    /// `clients` in `config.toml` only when it holds a client, so `launch` finds it too.
    pub fn choose_client_folder(&mut self, ctx: &egui::Context) {
        let Some(dir) = rfd::FileDialog::new()
            .set_title(self.tr.t("gui-choose-folder-title"))
            .pick_folder()
        else {
            return;
        };
        let found = phoecord_installer::discover::clients_at(&dir);
        let shown = dir.display().to_string();
        if found.is_empty() {
            let t = self.tr.f("gui-folder-no-discord", &[("path", &shown)]);
            self.toast(ctx, t, Tone::Danger);
            return;
        }
        let config_path = self.backend.mode.paths().config;
        let saved = (|| -> anyhow::Result<()> {
            let mut cfg = phoecord_proxy::config::Config::load_or_init(&config_path)?;
            if !cfg.clients.contains(&dir) {
                cfg.clients.push(dir.clone());
                cfg.save(&config_path)?;
            }
            Ok(())
        })();
        match saved {
            Ok(()) => {
                let names: Vec<String> =
                    found.iter().map(|c| self.channel_name(c.channel)).collect();
                self.selected = found.first().map(|c| c.channel);
                let t = self
                    .tr
                    .f("gui-folder-added", &[("name", &names.join(", "))]);
                self.toast(ctx, t, Tone::Ok);
                self.backend.refresh();
            }
            Err(e) => {
                let t = self
                    .tr
                    .f("gui-folder-save-failed", &[("error", &format!("{e:#}"))]);
                self.toast(ctx, t, Tone::Danger);
            }
        }
    }

    pub(crate) fn client_tiles(
        &mut self,
        ui: &mut egui::Ui,
        clients: &[ClientStatus],
        status: &crate::backend::Status,
    ) {
        let w = ui.available_width();
        let gap = theme::S3;
        let cols = if w >= 640.0 {
            clients.len().min(3)
        } else if w >= 420.0 {
            clients.len().min(2)
        } else {
            1
        }
        .max(1);
        let tile_w = (w - gap * (cols as f32 - 1.0)) / cols as f32;
        let ctx = ui.ctx().clone();
        for row in clients.chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for c in row {
                    let ch = c.client.channel;
                    let selected = self.selected == Some(ch);
                    let (rect, resp) = ui.allocate_exact_size(vec2(tile_w, 112.0), Sense::click());
                    let name = self.channel_name(ch);
                    resp.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::RadioButton,
                            true,
                            selected,
                            &name,
                        )
                    });
                    if resp.clicked() {
                        self.selected = Some(ch);
                    }
                    let id = resp.id;
                    let s = motion::tween(
                        &ctx,
                        id.with("sel"),
                        selected as u8 as f32,
                        dur::SMALL,
                        motion::out_cubic,
                    );
                    let h = motion::tween(
                        &ctx,
                        id.with("hov"),
                        resp.hovered() as u8 as f32,
                        dur::HOVER,
                        motion::out_cubic,
                    );
                    let p = ui.painter();
                    let border = theme::lerp_color(
                        theme::lerp_color(color::LINE, color::LINE_STRONG, h),
                        color::ROSE,
                        s,
                    );
                    if s > 0.0 {
                        // A soft halo around the selected tile.
                        p.rect_stroke(
                            rect.expand(1.5),
                            theme::radius(theme::R_TILE + 2),
                            theme::stroke(3.0, theme::fade(color::ROSE, 0.14 * s)),
                            StrokeKind::Outside,
                        );
                    }
                    p.rect(
                        rect,
                        theme::radius(theme::R_TILE),
                        theme::lerp_color(
                            theme::lerp_color(color::SURFACE, color::RAISED, h * 0.5),
                            color::SELECTED,
                            s,
                        ),
                        theme::stroke(1.0, border),
                        StrokeKind::Inside,
                    );
                    let inner = rect.shrink(16.0);
                    p.text(
                        inner.left_top(),
                        Align2::LEFT_TOP,
                        &name,
                        theme::semibold(theme::T_BODY + 1.0),
                        color::TEXT,
                    );
                    let ver = c
                        .client
                        .version
                        .as_deref()
                        .map(|v| self.tr.f("gui-version", &[("version", v)]))
                        .unwrap_or_else(|| self.tr.t("gui-version-unknown"));
                    p.text(
                        inner.left_top() + vec2(0.0, 22.0),
                        Align2::LEFT_TOP,
                        ver,
                        theme::regular(theme::T_CAPTION),
                        color::MUTED,
                    );
                    // Radio: ring, with a dot that springs in.
                    let rc = pos2(inner.right() - 9.0, inner.top() + 9.0);
                    let dot = motion::tween(
                        &ctx,
                        id.with("dot"),
                        selected as u8 as f32,
                        dur::SMALL,
                        motion::out_back,
                    );
                    p.circle_stroke(
                        rc,
                        9.0,
                        theme::stroke(1.5, theme::lerp_color(color::LINE_STRONG, color::ROSE, s)),
                    );
                    if dot > 0.01 {
                        p.circle_filled(rc, 4.5 * dot, color::ROSE);
                    }
                    // Status pill, bottom left.
                    let (txt, tone) = if status.is_on(ch) && status.needs_repair() {
                        (self.tr.t("gui-state-repair"), Tone::Warn)
                    } else if status.is_on(ch) {
                        (self.tr.t("gui-state-on"), Tone::Ok)
                    } else {
                        (self.tr.t("gui-state-off"), Tone::Neutral)
                    };
                    let pill_rect = Rect::from_min_size(
                        pos2(inner.left(), inner.bottom() - 22.0),
                        vec2(inner.width(), 22.0),
                    );
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(pill_rect));
                    widgets::pill(&mut child, id.with("pill"), &txt, tone);
                    widgets::focus_ring(ui, &resp, rect, theme::R_TILE);
                }
            });
            ui.add_space(gap - ui.spacing().item_spacing.y);
        }
    }

    /// Two short facts under the main action: what PhoeCord adds, what it never does.
    fn facts(&self, ui: &mut egui::Ui) {
        let w = ui.available_width();
        let two = w >= 560.0;
        let col = if two { (w - theme::S5) / 2.0 } else { w };
        let fact = |ui: &mut egui::Ui, title: String, body: String| {
            ui.vertical(|ui| {
                ui.set_width(col);
                let (r, _) = ui.allocate_exact_size(vec2(col, 1.0), Sense::hover());
                ui.painter().rect_filled(r, 0.0, color::LINE);
                ui.add_space(theme::S3);
                widgets::text(
                    ui,
                    title,
                    theme::semibold(theme::T_CAPTION + 0.5),
                    color::TEXT,
                );
                widgets::caption(ui, body);
            });
        };
        let a = (self.tr.t("gui-fact-adds-title"), self.tr.t("gui-fact-adds"));
        let b = (
            self.tr.t("gui-fact-never-title"),
            self.tr.t("gui-fact-never"),
        );
        if two {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = theme::S5;
                fact(ui, a.0, a.1);
                fact(ui, b.0, b.1);
            });
        } else {
            fact(ui, a.0, a.1);
            ui.add_space(theme::S3);
            fact(ui, b.0, b.1);
        }
    }

    fn install_progress(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let (channel, phases, failed, error, repair) = match &self.flow {
            Flow::Installing {
                channel,
                phases,
                repair,
            } => (*channel, *phases, None, None, *repair),
            Flow::Failed {
                channel,
                phases,
                failed,
                error,
            } => (*channel, *phases, *failed, Some(error.clone()), false),
            _ => return,
        };
        let name = channel.map(|c| self.channel_name(c));
        let title = match (&error, repair, &name) {
            (Some(_), _, _) => self.tr.t("gui-progress-failed-title"),
            (None, true, _) => self.tr.t("gui-progress-repair-title"),
            (None, false, Some(n)) => self.tr.f("gui-progress-title", &[("name", n)]),
            (None, false, None) => self.tr.t("gui-progress-title-plain"),
        };
        widgets::h1(ui, title);
        ui.add_space(theme::S1);
        widgets::muted(
            ui,
            self.tr.t(if error.is_some() {
                "gui-progress-failed-sub"
            } else {
                "gui-progress-sub"
            }),
        );
        ui.add_space(theme::S5);

        // Overall progress.
        let done = phases
            .iter()
            .filter(|p| matches!(p, PState::Done | PState::Skipped))
            .count() as f32;
        let frac = motion::tween(&ctx, Id::new("overall"), done / 3.0, 0.4, motion::out_cubic);
        let (bar, _) =
            ui.allocate_exact_size(vec2(ui.available_width().min(520.0), 4.0), Sense::hover());
        ui.painter()
            .rect_filled(bar, theme::radius(2), color::RAISED);
        let fill = Rect::from_min_size(bar.min, vec2(bar.width() * frac, bar.height()));
        ui.painter().rect_filled(
            fill,
            theme::radius(2),
            if error.is_some() {
                color::DANGER
            } else {
                color::ROSE
            },
        );
        ui.add_space(theme::S4);

        for (i, ph) in Phase::ALL.iter().enumerate() {
            self.phase_row(ui, i, *ph, phases[i], channel);
        }

        if let Some(err) = error {
            ui.add_space(theme::S5);
            let (t, b) = self.install_error(&err, failed);
            widgets::banner(ui, Tone::Danger, icon::WARNING_CIRCLE, &t, &b, None);
            ui.add_space(theme::S4);
            let again = self.tr.t("gui-try-again");
            let copy = self.tr.t("gui-copy-details");
            let back = self.tr.t("gui-back");
            match button_row(
                ui,
                vec![
                    Button::new(&again, Kind::Primary)
                        .icon(icon::ARROWS_CLOCKWISE)
                        .big(),
                    Button::new(&copy, Kind::Ghost).icon(icon::COPY).big(),
                    Button::new(&back, Kind::Ghost).big(),
                ],
            ) {
                Some(0) => self.start_install(channel, false),
                Some(1) => {
                    ctx.copy_text(err.clone());
                    let t = self.tr.t("gui-toast-copied");
                    self.toast(&ctx, t, Tone::Neutral);
                }
                Some(2) => self.flow = Flow::Idle,
                _ => {}
            }
        }
    }

    /// The title and caption of one install step (`skipped`: the shortcut step was skipped).
    pub(crate) fn phase_text(
        &self,
        ph: Phase,
        skipped: bool,
        channel: Option<Channel>,
    ) -> (String, String) {
        match ph {
            Phase::Certificate => (self.tr.t("gui-phase-cert"), self.tr.t("gui-phase-cert-sub")),
            Phase::Autostart => (
                self.tr.t("gui-phase-autostart"),
                self.tr.t("gui-phase-autostart-sub"),
            ),
            Phase::Shortcut => {
                let cap = if skipped {
                    if Os::current() == Os::MacOs && channel.is_some() {
                        self.tr.t("gui-phase-shortcut-mac")
                    } else {
                        self.tr.t("gui-phase-shortcut-none")
                    }
                } else {
                    self.tr.f(
                        "gui-phase-shortcut-sub",
                        &[(
                            "name",
                            &channel.map(|c| self.channel_name(c)).unwrap_or_default(),
                        )],
                    )
                };
                (self.tr.t("gui-phase-shortcut"), cap)
            }
        }
    }

    /// One install step: a dot that becomes a spinner, then a check that draws itself.
    fn phase_row(
        &self,
        ui: &mut egui::Ui,
        i: usize,
        ph: Phase,
        st: PState,
        channel: Option<Channel>,
    ) {
        let ctx = ui.ctx().clone();
        let (label, caption) = self.phase_text(ph, st == PState::Skipped, channel);
        ui.horizontal_top(|ui| {
            let (dot, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
            let p = ui.painter();
            let c = dot.center();
            match st {
                PState::Waiting => {
                    p.circle_stroke(c, 10.5, theme::stroke(1.5, color::LINE_STRONG));
                }
                PState::Running => {
                    p.circle_stroke(c, 10.5, theme::stroke(1.5, theme::fade(color::ROSE, 0.25)));
                    widgets::spinner(p, c, 10.5, color::ROSE, &ctx);
                }
                PState::Done => {
                    // Checks land one after another even when the steps finish together.
                    let age = motion::age(&ctx, Id::new(("phase-done", i))) - i as f32 * 0.09;
                    let fill = motion::out_back((age / dur::SMALL).clamp(0.0, 1.0));
                    let check = motion::out_cubic(((age - 0.06) / dur::CHECK).clamp(0.0, 1.0));
                    if age < dur::CHECK + 0.1 {
                        ctx.request_repaint();
                    }
                    p.circle_stroke(c, 10.5, theme::stroke(1.5, color::LINE_STRONG));
                    if fill > 0.0 {
                        p.circle_filled(c, 11.0 * fill, color::ACCENT);
                        widgets::check_mark(p, dot.shrink(4.0), check, egui::Color32::WHITE, 2.2);
                    }
                }
                PState::Skipped => {
                    p.circle_stroke(c, 10.5, theme::stroke(1.5, color::LINE_STRONG));
                    p.line_segment(
                        [c - vec2(4.0, 0.0), c + vec2(4.0, 0.0)],
                        theme::stroke(1.8, color::MUTED),
                    );
                }
                PState::Failed => {
                    p.circle_filled(c, 11.0, color::DANGER_BG);
                    p.circle_stroke(c, 10.5, theme::stroke(1.5, color::DANGER));
                    widgets::cross(p, dot, color::DANGER, 2.0);
                }
            }
            ui.add_space(theme::S2);
            ui.vertical(|ui| {
                let col = if st == PState::Waiting {
                    color::MUTED
                } else {
                    color::TEXT
                };
                widgets::text(ui, label, theme::medium(theme::T_BODY), col);
                widgets::caption(ui, caption);
                if st == PState::Running {
                    ui.add_space(theme::S1);
                    // We cannot know how long a step takes (a system prompt may wait for the
                    // user), so the bar runs instead of filling.
                    let (r, _) = ui.allocate_exact_size(vec2(260.0, 3.0), Sense::hover());
                    ui.painter().rect_filled(r, theme::radius(2), color::RAISED);
                    let t = if motion::reduced(&ctx) {
                        0.3
                    } else {
                        (ctx.input(|i| i.time) as f32 * 0.9).fract()
                    };
                    let x0 = r.left() + (r.width() + 90.0) * motion::in_out_cubic(t) - 90.0;
                    let seg = Rect::from_x_y_ranges(
                        x0.max(r.left())..=(x0 + 90.0).min(r.right()),
                        r.y_range(),
                    );
                    if seg.width() > 0.0 {
                        ui.painter().rect_filled(seg, theme::radius(2), color::ROSE);
                    }
                    ctx.request_repaint();
                }
            });
        });
        ui.add_space(theme::S3);
    }

    /// Title and text for a failed install: what happened and what to do next.
    fn install_error(&self, err: &str, failed: Option<Phase>) -> (String, String) {
        if err.contains("missing-binary") {
            let s = self
                .status
                .as_ref()
                .map(|s| s.proxy_exe.display().to_string())
                .unwrap_or_default();
            return (
                self.tr.t("gui-missing-binary-title"),
                self.tr.f("gui-missing-binary", &[("path", &s)]),
            );
        }
        let key = match failed {
            Some(Phase::Certificate) => "gui-err-cert",
            Some(Phase::Autostart) => "gui-err-autostart",
            Some(Phase::Shortcut) => "gui-err-shortcut",
            None => "gui-err-generic",
        };
        (self.tr.t(&format!("{key}-title")), self.tr.t(key))
    }

    fn install_ready(&mut self, ui: &mut egui::Ui) {
        let Flow::Ready { channel, repair } = self.flow else {
            return;
        };
        let ctx = ui.ctx().clone();
        let age = motion::age(&ctx, Id::new("ready-burst"));
        let ring = motion::out_back((age / 0.35).clamp(0.0, 1.0));
        let check = motion::out_cubic(((age - 0.15) / dur::CHECK).clamp(0.0, 1.0));
        if age < 0.6 {
            ctx.request_repaint();
        }
        let (r, _) = ui.allocate_exact_size(vec2(56.0, 56.0), Sense::hover());
        let p = ui.painter();
        p.circle_filled(r.center(), 28.0 * ring, color::ACCENT);
        p.circle_stroke(
            r.center(),
            28.0 * ring + 5.0,
            theme::stroke(2.0, theme::fade(color::ROSE, 0.25 * ring)),
        );
        widgets::check_mark(p, r.shrink(12.0), check, egui::Color32::WHITE, 3.0);
        ui.add_space(theme::S4);
        widgets::h1(
            ui,
            self.tr.t(if repair {
                "gui-ready-title-repair"
            } else {
                "gui-ready-title"
            }),
        );
        ui.add_space(theme::S1);
        let name = channel.map(|c| self.channel_name(c));
        widgets::muted(
            ui,
            match &name {
                Some(n) if Os::current() == Os::MacOs => {
                    self.tr.f("gui-ready-sub-mac", &[("name", n)])
                }
                Some(n) => self.tr.f("gui-ready-sub", &[("name", n)]),
                None => self.tr.t("gui-ready-sub-plain"),
            },
        );
        ui.add_space(theme::S5);
        let open = name
            .as_ref()
            .map(|n| self.tr.f("gui-open-action", &[("name", n)]))
            .unwrap_or_default();
        let back = self.tr.t("gui-back");
        let mut b = Vec::new();
        if channel.is_some() {
            b.push(Button::new(&open, Kind::Primary).icon(icon::PLAY).big());
        }
        b.push(
            Button::new(
                &back,
                if channel.is_some() {
                    Kind::Ghost
                } else {
                    Kind::Primary
                },
            )
            .big(),
        );
        let has_open = channel.is_some();
        match button_row(ui, b) {
            Some(0) if has_open => {
                if let Some(c) = channel {
                    self.open_discord(c);
                }
                self.flow = Flow::Idle;
            }
            Some(_) => self.flow = Flow::Idle,
            None => {}
        }
    }

    fn uninstall_view(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        match &self.flow {
            Flow::Uninstalling => {
                let (r, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
                widgets::spinner(ui.painter(), r.center(), 16.0, color::ROSE, &ctx);
                ui.add_space(theme::S4);
                widgets::h1(ui, self.tr.t("gui-removing-title"));
                widgets::muted(ui, self.tr.t("gui-removing-sub"));
            }
            Flow::Removed(rep) => {
                let clean = rep.is_clean();
                widgets::h1(
                    ui,
                    self.tr.t(if clean {
                        "gui-removed-title"
                    } else {
                        "gui-removed-partly-title"
                    }),
                );
                ui.add_space(theme::S1);
                widgets::muted(
                    ui,
                    self.tr.f(
                        "gui-removed-sub",
                        &[("count", &rep.removed.len().to_string())],
                    ),
                );
                if !clean {
                    ui.add_space(theme::S4);
                    let mut lines: Vec<String> = rep
                        .failed
                        .iter()
                        .map(|(s, why)| format!("{}: {why}", s.describe()))
                        .collect();
                    if let Some(w) = &rep.stop_failed {
                        lines.push(w.clone());
                    }
                    widgets::banner(
                        ui,
                        Tone::Danger,
                        icon::WARNING_CIRCLE,
                        &self.tr.t("gui-removed-failed-title"),
                        &format!("{}\n{}", self.tr.t("gui-removed-failed"), lines.join("\n")),
                        None,
                    );
                }
                ui.add_space(theme::S5);
                let done = self.tr.t("gui-done");
                let again = self.tr.t("gui-try-again");
                let mut b = vec![Button::new(&done, Kind::Primary).big()];
                if !clean {
                    b.push(
                        Button::new(&again, Kind::Ghost)
                            .icon(icon::ARROWS_CLOCKWISE)
                            .big(),
                    );
                }
                match button_row(ui, b) {
                    Some(0) => self.flow = Flow::Idle,
                    Some(1) => {
                        self.flow = Flow::Uninstalling;
                        self.backend.uninstall();
                    }
                    _ => {}
                }
            }
            Flow::RemoveFailed(e) => {
                let e = e.clone();
                widgets::h1(ui, self.tr.t("gui-removed-failed-title"));
                ui.add_space(theme::S3);
                widgets::banner(
                    ui,
                    Tone::Danger,
                    icon::WARNING_CIRCLE,
                    &self.tr.t("gui-removed-failed"),
                    &e,
                    None,
                );
                ui.add_space(theme::S5);
                let again = self.tr.t("gui-try-again");
                let back = self.tr.t("gui-back");
                match button_row(
                    ui,
                    vec![
                        Button::new(&again, Kind::Primary).big(),
                        Button::new(&back, Kind::Ghost).big(),
                    ],
                ) {
                    Some(0) => {
                        self.flow = Flow::Uninstalling;
                        self.backend.uninstall();
                    }
                    Some(1) => self.flow = Flow::Idle,
                    _ => {}
                }
            }
            _ => {}
        }
    }

    pub(crate) fn confirm_uninstall(&mut self) {
        self.flow = Flow::Uninstalling;
        self.backend.uninstall();
    }
}

const DISCORD_DOWNLOAD: &str = "https://discord.com/download";

/// Where discovery looks on this OS, for the "nothing found" screen.
fn looked_in() -> Vec<String> {
    let roots = phoecord_installer::discover::Roots::system();
    // The home folder as `~`, so the list stays short (and screenshots keep the user's name out).
    let home = dirs::home_dir();
    let show = |p: &std::path::Path| match home.as_deref().and_then(|h| p.strip_prefix(h).ok()) {
        Some(rest) => std::path::Path::new("~").join(rest).display().to_string(),
        None => p.display().to_string(),
    };
    match Os::current() {
        Os::Windows => roots
            .local_app_data
            .iter()
            .flat_map(|d| ["Discord", "DiscordPTB", "DiscordCanary"].map(|c| show(&d.join(c))))
            .collect(),
        Os::MacOs => roots.applications.iter().map(|p| show(p)).collect(),
        _ => {
            let mut v = vec!["$PATH".to_string()];
            v.extend(roots.flatpak_roots.iter().map(|p| show(p)));
            v.extend(roots.snap_roots.iter().map(|p| show(p)));
            v
        }
    }
}
