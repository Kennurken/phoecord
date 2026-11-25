//! The Catalog list of the Plugins tab: community plugins from `catalog/plugins.json`, install and
//! update through the same staging and rights dialog as a file or a link. The download is checked
//! against the hash in the index (`phoecord-catalog`) before it is even unpacked.

use crate::app::App;
use crate::packages::{self, Installed};
use crate::theme::{self, color};
use crate::widgets::{self, Button, Kind, Tone};
use eframe::egui::{self, Align, Layout};
use egui_phosphor::regular as icon;
use phoecord_catalog::catalog;
use phoecord_catalog::index::{Entry, Index, Mark, is_newer};
use std::collections::HashMap;

#[derive(Default)]
pub enum Load {
    /// Nothing fetched yet: the catalog is only read when the user opens it, or when a package
    /// installed from it exists (to offer its updates).
    #[default]
    Idle,
    Loading,
    /// The reason, already in words.
    Failed(String),
    Ready(Index),
}

#[derive(Default)]
pub struct State {
    /// The list is shown.
    pub open: bool,
    pub load: Load,
    /// Catalog id -> installed version, for packages installed from the catalog.
    marks: HashMap<String, String>,
    auto_checked: bool,
}

impl State {
    pub fn loaded(&mut self, r: Result<Index, String>) {
        self.load = match r {
            Ok(i) => Load::Ready(i),
            Err(why) => Load::Failed(why),
        };
    }

    /// Re-read which installed packages came from the catalog (after the package list changed).
    pub fn refresh_marks(&mut self, packages: &[Installed]) {
        self.marks = packages
            .iter()
            .filter(|p| Mark::read(&p.dir).is_some())
            .filter_map(|p| Some((p.id.clone(), p.manifest.as_ref()?.manifest.version.clone())))
            .collect();
    }

    /// The installed version of `e`, when the catalog has a newer one.
    fn update_from(&self, e: &Entry) -> Option<&str> {
        let have = self.marks.get(&e.id)?;
        is_newer(&e.version, have).then_some(have.as_str())
    }

    fn update_count(&self) -> usize {
        match &self.load {
            Load::Ready(i) => i
                .plugins
                .iter()
                .filter(|e| self.update_from(e).is_some())
                .count(),
            _ => 0,
        }
    }
}

impl App {
    pub(crate) fn toggle_catalog(&mut self) {
        self.catalog.open = !self.catalog.open;
        if self.catalog.open && matches!(self.catalog.load, Load::Idle | Load::Failed(_)) {
            self.load_catalog();
        }
    }

    fn load_catalog(&mut self) {
        self.catalog.load = Load::Loading;
        self.backend.plugin_job("catalog", || {
            packages::Msg::Index(catalog::fetch_index(&catalog::index_url()).map(Box::new))
        });
    }

    /// Download `entry` (hash-checked), stage it; the answer opens the rights dialog.
    fn install_from_catalog(&mut self, entry: Entry) {
        self.plugin_busy = true;
        let store = self.store.clone();
        self.backend.plugin_job("catalog-install", move || {
            packages::Msg::Staged(catalog::stage_entry(&store, &entry, &App::verify).map(Box::new))
        });
    }

    pub(crate) fn catalog_section(&mut self, ui: &mut egui::Ui) {
        // Updates are offered without the user opening the catalog, but only when something from
        // it is installed: nobody who never used the catalog is contacted.
        if !self.catalog.auto_checked {
            self.catalog.auto_checked = true;
            if !self.catalog.marks.is_empty() && matches!(self.catalog.load, Load::Idle) {
                self.load_catalog();
            }
        }
        let updates = self.catalog.update_count();
        if !self.catalog.open {
            if updates > 0 {
                let text = self
                    .tr
                    .f("gui-catalog-updates", &[("count", &updates.to_string())]);
                widgets::pill(ui, egui::Id::new("catalog-updates"), &text, Tone::Warn);
                ui.add_space(theme::S4);
            }
            return;
        }
        ui.horizontal(|ui| {
            widgets::title(ui, self.tr.t("gui-catalog-title"));
            if updates > 0 {
                let text = self
                    .tr
                    .f("gui-catalog-updates", &[("count", &updates.to_string())]);
                widgets::pill(ui, egui::Id::new("catalog-updates"), &text, Tone::Warn);
            }
        });
        ui.add_space(theme::S1);
        widgets::muted(ui, self.tr.t("gui-catalog-sub"));
        ui.add_space(theme::S3);

        let mut retry = false;
        let mut install: Option<Entry> = None;
        match &self.catalog.load {
            Load::Idle | Load::Loading => {
                widgets::caption(ui, self.tr.t("gui-catalog-loading"));
            }
            Load::Failed(why) => {
                widgets::text(
                    ui,
                    self.tr.f("gui-catalog-failed", &[("why", why)]),
                    theme::regular(theme::T_CAPTION),
                    color::DANGER,
                );
                ui.add_space(theme::S2);
                let again = self.tr.t("gui-catalog-retry");
                retry = Button::new(&again, Kind::Secondary)
                    .icon(icon::ARROW_CLOCKWISE)
                    .show(ui)
                    .clicked();
            }
            Load::Ready(index) if index.plugins.is_empty() => {
                widgets::muted(ui, self.tr.t("gui-catalog-empty"));
            }
            Load::Ready(index) => {
                for e in &index.plugins {
                    if self.catalog_row(ui, e) {
                        install = Some(e.clone());
                    }
                }
            }
        }
        if retry {
            self.load_catalog();
        }
        if let Some(e) = install {
            self.install_from_catalog(e);
        }
        ui.add_space(theme::S2);
        let hide = self.tr.t("gui-catalog-hide");
        if Button::new(&hide, Kind::Ghost).show(ui).clicked() {
            self.catalog.open = false;
        }
        ui.add_space(theme::S5);
    }

    /// One entry; true when its button was clicked.
    fn catalog_row(&self, ui: &mut egui::Ui, e: &Entry) -> bool {
        let update = self.catalog.update_from(e);
        let installed = self.catalog.marks.contains_key(&e.id);
        let mut clicked = false;
        egui::Frame::new()
            .stroke(theme::stroke(1.0, color::LINE))
            .fill(color::SURFACE)
            .corner_radius(theme::radius(theme::R_TILE))
            .inner_margin(egui::Margin::symmetric(16, 12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    let text_w = (ui.available_width() - 150.0).max(120.0);
                    ui.vertical(|ui| {
                        ui.set_width(text_w);
                        widgets::text(
                            ui,
                            e.name.clone(),
                            theme::semibold(theme::T_BODY),
                            color::TEXT,
                        );
                        widgets::caption(
                            ui,
                            self.by_line(&e.version, std::slice::from_ref(&e.author)),
                        );
                        widgets::muted(ui, e.description.clone());
                        let rights = if e.rights.is_empty() {
                            self.tr.t("gui-catalog-rights-none")
                        } else {
                            self.tr
                                .f("gui-catalog-rights", &[("list", &e.rights.join(", "))])
                        };
                        widgets::caption(ui, rights);
                    });
                    ui.with_layout(Layout::top_down(Align::Max), |ui| {
                        let (label, kind, enabled) = match (update, installed) {
                            (Some(_), _) => (
                                self.tr.f("gui-catalog-update", &[("version", &e.version)]),
                                Kind::Primary,
                                true,
                            ),
                            (None, true) => {
                                (self.tr.t("gui-catalog-installed"), Kind::Ghost, false)
                            }
                            (None, false) => {
                                (self.tr.t("gui-catalog-install"), Kind::Primary, true)
                            }
                        };
                        let b = Button::new(&label, kind)
                            .icon(icon::DOWNLOAD_SIMPLE)
                            .enabled(enabled && !self.plugin_busy)
                            .busy(self.plugin_busy && enabled);
                        clicked = b.show(ui).clicked();
                    });
                });
            });
        ui.add_space(theme::S2);
        clicked
    }
}
