//! PhoeCord installer window (stage 1.4.3): terms of use, install / repair / uninstall per
//! Discord client, and plugin packages, without a terminal. The work itself is done by
//! `phoecord-installer`, the same code `phoecord setup` and `phoecord uninstall` run.
//!
//! `--dry-run` keeps every change inside a sandbox folder (`PHOECORD_GUI_SANDBOX`, default
//! `<temp>/phoecord-gui-dry-run`) with a simulated trust store; `--demo` adds two made-up Discord
//! installs to it (for screenshots).

#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod backend;
mod motion;
mod packages;
mod tabs;
mod terms;
mod text;
mod theme;
mod widgets;

fn icon() -> eframe::egui::IconData {
    // The mark, rendered small: a rounded square going from rose to garnet.
    let n = 64u32;
    let mut rgba = Vec::with_capacity((n * n * 4) as usize);
    let r = 18.0f32;
    for y in 0..n {
        for x in 0..n {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let cx = fx.clamp(r, n as f32 - r);
            let cy = fy.clamp(r, n as f32 - r);
            let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
            let a = (r - d + 0.5).clamp(0.0, 1.0);
            let c = theme::lerp_color(
                theme::color::ROSE,
                eframe::egui::Color32::from_rgb(0x7A, 0x10, 0x22),
                fy / n as f32,
            );
            rgba.extend_from_slice(&[c.r(), c.g(), c.b(), (a * 255.0) as u8]);
        }
    }
    eframe::egui::IconData {
        rgba,
        width: n,
        height: n,
    }
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mode = backend::Mode::from_args(&args);
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("PhoeCord")
            .with_app_id("dev.phoecord.installer")
            .with_inner_size([960.0, 640.0])
            .with_min_inner_size([520.0, 520.0])
            .with_icon(icon()),
        ..Default::default()
    };
    eframe::run_native(
        "PhoeCord",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, mode)))),
    )
}
