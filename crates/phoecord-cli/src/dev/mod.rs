//! `phoecord dev …`: tools for plugin authors. Each subcommand lives in its own file.

pub mod console;

use anyhow::Result;
use clap::Subcommand;
use phoecord_i18n::Localizer;
use phoecord_proxy::config::Paths;

#[derive(Subcommand)]
pub enum DevCmd {
    /// Attach to the running proxy: live plugin logs, events, verdicts and timings, and commands
    Console(console::Args),
}

pub fn run(cmd: DevCmd, paths: &Paths, l10n: &Localizer) -> Result<()> {
    match cmd {
        DevCmd::Console(args) => console::run(args, paths, l10n),
    }
}
