//! `phoecord dev …`: tools for plugin authors. Each subcommand lives in its own file.

pub mod console;
mod inspect_pe;
mod watch;

use anyhow::Result;
use clap::Subcommand;
use phoecord_i18n::Localizer;
use phoecord_proxy::config::Paths;

#[derive(Subcommand)]
pub enum DevCmd {
    /// Attach to the running proxy: live plugin logs, events, verdicts and timings, and commands
    Console(console::Args),
    /// Rebuild a plugin when its sources change and hot-reload it in the running proxy.
    Watch(watch::WatchArgs),
    /// Read-only inspection of a PE executable: imports, DLL search restrictions, sections.
    InspectPe(inspect_pe::InspectPeArgs),
}

pub fn run(cmd: DevCmd, paths: &Paths, l10n: &Localizer) -> Result<()> {
    match cmd {
        DevCmd::Console(args) => console::run(args, paths, l10n),
        DevCmd::Watch(args) => watch::run(args, paths, l10n),
        DevCmd::InspectPe(args) => inspect_pe::run(args, l10n),
    }
}
