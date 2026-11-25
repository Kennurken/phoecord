//! `phoecord dev ...`: tools for plugin authors. Each tool lives in its own file here.

mod watch;

use anyhow::Result;
use clap::Subcommand;
use phoecord_i18n::Localizer;
use phoecord_proxy::config::Paths;

#[derive(Subcommand)]
pub enum DevCmd {
    /// Rebuild a plugin when its sources change and hot-reload it in the running proxy.
    Watch(watch::WatchArgs),
}

pub fn run(cmd: DevCmd, paths: &Paths, l10n: &Localizer) -> Result<()> {
    match cmd {
        DevCmd::Watch(args) => watch::run(args, paths, l10n),
    }
}
