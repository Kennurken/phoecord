//! Settings (language, motion, upstream proxy, folders), About, and the terms screen.

use crate::app::{App, button_row};
use crate::motion;
use crate::text::Tr;
use crate::theme::{self, color};
use crate::widgets::{self, Button, Kind, Tone};
use eframe::egui::{self, Id, Rect, Sense, vec2};
use egui_phosphor::regular as icon;
use phoecord_proxy::config::Config;

const REPO: &str = "https://github.com/Nanda070/phoecord";

impl App {
    /// A settings group: title and hint on the left, the control below on narrow windows.
    fn setting(
        &mut self,
        ui: &mut egui::Ui,
        title: &str,
        hint: &str,
        control: impl FnOnce(&mut App, &mut egui::Ui),
    ) {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
        ui.painter().rect_filled(r, 0.0, color::LINE);
        ui.add_space(theme::S4);
        let wide = ui.available_width() >= 620.0;
        if wide {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(260.0);
                    widgets::text(ui, title, theme::semibold(theme::T_BODY), color::TEXT);
                    widgets::caption(ui, hint);
                });
                ui.add_space(theme::S5);
                ui.vertical(|ui| control(self, ui));
            });
        } else {
            widgets::text(ui, title, theme::semibold(theme::T_BODY), color::TEXT);
            widgets::caption(ui, hint);
            ui.add_space(theme::S2);
            control(self, ui);
        }
        ui.add_space(theme::S5);
    }

    pub(crate) fn settings_tab(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        widgets::h1(ui, self.tr.t("gui-settings-title"));
        ui.add_space(theme::S6);

        // Language.
        let t = self.tr.t("gui-settings-lang");
        let h = self.tr.t("gui-settings-lang-hint");
        self.setting(ui, &t, &h, |app, ui| {
            let opts = [
                (None, app.tr.t("gui-settings-lang-system")),
                (Some("en"), "English".to_string()),
                (Some("ru"), "Русский".to_string()),
            ];
            let cur = app.config.language.clone();
            let idx = opts
                .iter()
                .position(|(v, _)| v.map(str::to_string) == cur)
                .unwrap_or(0);
            let labels: Vec<String> = opts.iter().map(|o| o.1.clone()).collect();
            if let Some(i) = widgets::tabs(ui, Id::new("lang-seg"), &labels, idx, 16.0) {
                let v = opts[i].0.map(str::to_string);
                match app.save_config(|c| c.language = v.clone()) {
                    Ok(()) => app.tr = Tr::new(v.as_deref()),
                    Err(e) => {
                        let t = app.tr.f("gui-pkg-err-io", &[("why", &e)]);
                        app.toast(ui.ctx(), t, Tone::Danger);
                    }
                }
            }
        });

        // Motion.
        let t = self.tr.t("gui-settings-motion");
        let h = self.tr.t("gui-settings-motion-hint");
        self.setting(ui, &t, &h, |app, ui| {
            ui.horizontal(|ui| {
                let label = app.tr.t("gui-settings-motion-switch");
                if widgets::switch(ui, app.reduce_motion, &label).clicked() {
                    app.reduce_motion = !app.reduce_motion;
                    motion::set_reduced(ui.ctx(), app.reduce_motion);
                }
                ui.add_space(theme::S2);
                widgets::body(ui, label);
            });
        });

        // Upstream proxy.
        let t = self.tr.t("gui-settings-upstream");
        let h = self.tr.t("gui-settings-upstream-hint");
        self.setting(ui, &t, &h, |app, ui| {
            let label = app.tr.t("gui-settings-upstream-label");
            let mut v = std::mem::take(&mut app.upstream);
            widgets::field(ui, &label, &mut v, "socks5://127.0.0.1:1080");
            app.upstream = v;
            if let Some((tone, msg)) = &app.upstream_msg {
                ui.add_space(theme::S1);
                let col = tone.colors().0;
                widgets::text(ui, msg.clone(), theme::regular(theme::T_CAPTION), col);
            }
            ui.add_space(theme::S2);
            let save = app.tr.t("gui-save");
            if button_row(ui, vec![Button::new(&save, Kind::Secondary)]) == Some(0) {
                let raw = app.upstream.trim().to_string();
                let parsed = if raw.is_empty() {
                    Ok(None)
                } else {
                    phoecord_proxy::egress::Upstream::parse(&raw).map(|_| Some(raw.clone()))
                };
                app.upstream_msg = Some(match parsed {
                    Ok(v) => match app.save_config(|c| c.upstream = v.clone()) {
                        Ok(()) => (Tone::Ok, app.tr.t("gui-settings-upstream-saved")),
                        Err(e) => (Tone::Danger, app.tr.f("gui-pkg-err-io", &[("why", &e)])),
                    },
                    Err(_) => (Tone::Danger, app.tr.t("gui-settings-upstream-bad")),
                });
            }
        });

        // Folders.
        let t = self.tr.t("gui-settings-files");
        let h = self.tr.t("gui-settings-files-hint");
        self.setting(ui, &t, &h, |app, ui| {
            let data = app.backend.mode.paths().data;
            let cfg = app.store.config.clone();
            let a = app.tr.t("gui-settings-open-data");
            let b = app.tr.t("gui-settings-open-config");
            let hit = button_row(
                ui,
                vec![
                    Button::new(&a, Kind::Secondary).icon(icon::FOLDER_OPEN),
                    Button::new(&b, Kind::Secondary).icon(icon::FILE_TEXT),
                ],
            );
            let target = match hit {
                Some(0) => Some(data),
                Some(1) => cfg.parent().map(|p| p.to_path_buf()),
                _ => None,
            };
            if let Some(p) = target {
                let _ = std::fs::create_dir_all(&p);
                if let Err(e) = crate::backend::open_path(&p) {
                    let t = app.tr.f("gui-pkg-err-io", &[("why", &format!("{e:#}"))]);
                    app.toast(ui.ctx(), t, Tone::Danger);
                }
            }
        });
        let _ = ctx;
    }

    /// Change and save `config.toml` (the proxy picks it up live).
    fn save_config(&mut self, f: impl FnOnce(&mut Config)) -> Result<(), String> {
        let mut c = Config::load_or_init(&self.store.config).map_err(|e| format!("{e:#}"))?;
        f(&mut c);
        c.save(&self.store.config).map_err(|e| format!("{e:#}"))?;
        self.config = c;
        Ok(())
    }

    pub(crate) fn about_tab(&mut self, ui: &mut egui::Ui) {
        let (r, _) = ui.allocate_exact_size(vec2(56.0, 56.0), Sense::hover());
        theme::paint_mark(ui.painter(), r);
        ui.add_space(theme::S4);
        widgets::h1(ui, "PhoeCord");
        widgets::muted(
            ui,
            self.tr.f(
                "gui-about-version",
                &[("version", env!("CARGO_PKG_VERSION"))],
            ),
        );
        ui.add_space(theme::S3);
        ui.scope(|ui| {
            ui.set_max_width(560.0f32.min(ui.available_width()));
            widgets::body(ui, self.tr.t("gui-about-body"));
        });
        ui.add_space(theme::S5);
        let source = self.tr.t("gui-about-source");
        let issue = self.tr.t("gui-about-report");
        let terms = self.tr.t("gui-about-terms");
        match button_row(
            ui,
            vec![
                Button::new(&source, Kind::Secondary).icon(icon::GITHUB_LOGO),
                Button::new(&issue, Kind::Secondary).icon(icon::BUG),
                Button::new(&terms, Kind::Secondary).icon(icon::FILE_TEXT),
            ],
        ) {
            Some(0) => ui.ctx().open_url(egui::OpenUrl::new_tab(REPO)),
            Some(1) => ui
                .ctx()
                .open_url(egui::OpenUrl::new_tab(format!("{REPO}/issues"))),
            Some(2) => {
                self.terms_viewing = true;
                self.terms_at_end = false;
            }
            _ => {}
        }
        ui.add_space(theme::S6);
        let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
        ui.painter().rect_filled(line, 0.0, color::LINE);
        ui.add_space(theme::S3);
        widgets::caption(ui, self.tr.t("gui-about-license"));
        widgets::caption(ui, self.tr.t("gui-about-security"));
        ui.add_space(theme::S6);
        self.credits(ui);
    }

    /// Contributors (from the README's contributors block, filled from GitHub's list), Vencord's
    /// attribution, and the font, icon and library licenses.
    fn credits(&mut self, ui: &mut egui::Ui) {
        widgets::title(ui, self.tr.t("gui-credits"));
        ui.add_space(theme::S3);
        widgets::caption(ui, self.tr.t("gui-credits-contributors"));
        ui.add_space(theme::S1);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(theme::S2, theme::S2);
            for login in contributors() {
                if Button::new(login, Kind::Secondary)
                    .icon(icon::GITHUB_LOGO)
                    .show(ui)
                    .clicked()
                {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(format!(
                        "https://github.com/{login}"
                    )));
                }
            }
        });
        ui.add_space(theme::S4);
        ui.scope(|ui| {
            ui.set_max_width(560.0f32.min(ui.available_width()));
            for key in [
                "gui-credits-vencord",
                "gui-credits-gpui",
                "gui-credits-font",
                "gui-credits-icons",
                "gui-credits-egui",
                "gui-credits-affiliation",
            ] {
                widgets::caption(ui, self.tr.t(key));
                ui.add_space(2.0);
            }
        });
        ui.add_space(theme::S3);
        let all = self.tr.t("gui-credits-licenses");
        if Button::new(&all, Kind::Ghost)
            .icon(icon::SCALES)
            .show(ui)
            .clicked()
        {
            ui.ctx()
                .open_url(egui::OpenUrl::new_tab(format!("{REPO}/tree/main/LICENSES")));
        }
    }

    /// The terms of use, before anything else on first start (and from About later).
    pub(crate) fn terms_screen(&mut self, ui: &mut egui::Ui, full: Rect) {
        let ctx = ui.ctx().clone();
        let lang = self.tr.lang().to_string();
        let md = phoecord_installer::terms::text(&lang);
        let w = 640.0f32.min(full.width() - 48.0);
        let left = full.center().x - w / 2.0;
        let bar_h = 84.0;
        let top = Rect::from_min_max(
            egui::pos2(left, full.top() + 36.0),
            egui::pos2(left + w, full.bottom()),
        );
        // Title block; its height decides where the text starts.
        let mut head = ui.new_child(egui::UiBuilder::new().max_rect(top));
        let (mark, _) = head.allocate_exact_size(vec2(32.0, 32.0), Sense::hover());
        theme::paint_mark(head.painter(), mark);
        head.add_space(theme::S3);
        widgets::h1(
            &mut head,
            self.tr.t(if self.terms_viewing {
                "gui-terms-title-view"
            } else {
                "gui-terms-title"
            }),
        );
        head.add_space(2.0);
        widgets::muted(
            &mut head,
            self.tr.f(
                "gui-terms-sub",
                &[(
                    "version",
                    &phoecord_installer::terms::TERMS_VERSION.to_string(),
                )],
            ),
        );
        let top = head.min_rect();
        let text_rect = Rect::from_min_max(
            egui::pos2(left, top.bottom() + 18.0),
            egui::pos2(left + w, full.bottom() - bar_h),
        );
        ui.painter().line_segment(
            [
                text_rect.left_top() - vec2(0.0, 1.0),
                text_rect.right_top() - vec2(0.0, 1.0),
            ],
            theme::stroke(1.0, color::LINE),
        );
        let mut body = ui.new_child(egui::UiBuilder::new().max_rect(text_rect));
        body.set_clip_rect(text_rect.expand2(vec2(12.0, 0.0)));
        let out = egui::ScrollArea::vertical()
            .id_salt(("terms", &lang))
            .auto_shrink([false, false])
            .max_width(text_rect.width())
            .max_height(text_rect.height())
            .show(&mut body, |ui| {
                ui.set_width(text_rect.width() - 14.0);
                ui.add_space(theme::S3);
                crate::terms::show(ui, md, text_rect.width() - 14.0);
                ui.add_space(theme::S5);
            });
        let at_end = out.state.offset.y + out.inner_rect.height() >= out.content_size.y - 8.0;
        if at_end {
            self.terms_at_end = true;
        }
        // Fade the bottom edge of the text while there is more to read.
        if !at_end {
            let fade = Rect::from_min_max(
                egui::pos2(text_rect.left(), text_rect.bottom() - 36.0),
                text_rect.max,
            );
            let mut mesh = egui::epaint::Mesh::default();
            mesh.colored_vertex(fade.left_top(), egui::Color32::TRANSPARENT);
            mesh.colored_vertex(fade.right_top(), egui::Color32::TRANSPARENT);
            mesh.colored_vertex(fade.right_bottom(), color::BG);
            mesh.colored_vertex(fade.left_bottom(), color::BG);
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            ui.painter().add(mesh);
        }
        // Bottom bar.
        let bar = Rect::from_min_max(
            egui::pos2(left, full.bottom() - bar_h),
            egui::pos2(left + w, full.bottom()),
        );
        ui.painter().line_segment(
            [bar.left_top(), bar.right_top()],
            theme::stroke(1.0, color::LINE),
        );
        let mut b = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(bar.shrink2(vec2(0.0, 18.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        if self.terms_viewing {
            let close = self.tr.t("gui-close");
            if Button::new(&close, Kind::Primary).show(&mut b).clicked() {
                self.terms_viewing = false;
            }
            return;
        }
        let accept = self.tr.t("gui-terms-accept");
        let quit = self.tr.t("gui-terms-quit");
        let lang_btn = if lang == "ru" {
            "English"
        } else {
            "Русский"
        };
        let ready = self.terms_at_end;
        b.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if Button::new(&accept, Kind::Primary)
                .icon(icon::CHECK)
                .enabled(ready)
                .show(ui)
                .clicked()
            {
                match self.backend.mode.env().and_then(|env| {
                    phoecord_installer::terms::accept(
                        &env,
                        phoecord_installer::terms::TERMS_VERSION,
                    )
                }) {
                    Ok(()) => {
                        self.terms_needed = false;
                        motion::forget(&ctx, Id::new("tab-enter"));
                    }
                    Err(e) => {
                        let t = self.tr.f("gui-pkg-err-io", &[("why", &format!("{e:#}"))]);
                        self.toast(&ctx, t, Tone::Danger);
                    }
                }
            }
            if Button::new(&quit, Kind::Ghost).show(ui).clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            if !ready {
                widgets::caption(ui, self.tr.t("gui-terms-scroll"));
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                if Button::new(lang_btn, Kind::Ghost)
                    .icon(icon::GLOBE)
                    .show(ui)
                    .clicked()
                {
                    self.tr = Tr::new(Some(if lang == "ru" { "en" } else { "ru" }));
                    self.terms_at_end = false;
                }
            });
        });
    }
}

/// GitHub logins from the README's `contributors:start` … `contributors:end` block, which
/// `scripts/update_contributors.py` fills from GitHub's contributors list.
fn contributors() -> Vec<&'static str> {
    parse_contributors(include_str!("../../../../README.md"))
}

fn parse_contributors(readme: &str) -> Vec<&str> {
    let Some(block) = readme
        .split("<!-- contributors:start -->")
        .nth(1)
        .and_then(|b| b.split("<!-- contributors:end -->").next())
    else {
        return Vec::new();
    };
    block
        .split("title=\"")
        .skip(1)
        .filter_map(|t| t.split('"').next())
        .filter(|l| !l.is_empty())
        .collect()
}

#[cfg(test)]
mod credits_tests {
    use super::*;

    #[test]
    fn contributors_come_from_the_readme_block() {
        let md = "x title=\"no\"\n<!-- contributors:start -->\n<a title=\"Nanda070\"><img></a>\n<a title=\"b\"></a>\n<!-- contributors:end -->";
        assert_eq!(parse_contributors(md), vec!["Nanda070", "b"]);
        assert!(contributors().contains(&"Nanda070"));
    }
}
