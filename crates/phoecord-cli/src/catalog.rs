//! `phoecord install | update | remove | catalog`: community plugins from the catalog index
//! (`catalog/plugins.json`), through the same staging and rights check as the installer window.
//! A download whose SHA-256 is not the listed one is refused before anything is unpacked.

use crate::tf;
use anyhow::{Result, bail};
use phoecord_catalog::catalog::{self, fetch_index, index_url};
use phoecord_catalog::index::{Index, Mark};
use phoecord_catalog::store::{self, Candidate, Error, Store};
use phoecord_i18n::Localizer;
use phoecord_proxy::config::Paths;
use std::io::{BufRead, IsTerminal, Write};
use std::path::Path;

fn store(paths: &Paths) -> Store {
    Store::new(&paths.data, &paths.config)
}

/// The message for a package or catalog error, in the user's language.
fn explain(l: &Localizer, e: &Error) -> String {
    match e {
        Error::HashMismatch => l.t("cli-cat-err-hash"),
        Error::CatalogMismatch(why) => tf(l, "cli-cat-err-mismatch", &[("why", why)]),
        Error::Index(why) => tf(l, "cli-cat-err-index", &[("why", why)]),
        Error::NotInCatalog(id) => tf(l, "cli-cat-not-found", &[("id", id)]),
        Error::NotAPackage => l.t("gui-pkg-err-not-package"),
        Error::NoWasm => l.t("gui-pkg-err-no-wasm"),
        Error::TooBig => tf(
            l,
            "gui-pkg-err-too-big",
            &[("mb", &(store::MAX_BYTES / 1024 / 1024).to_string())],
        ),
        Error::UnsafePath(p) => tf(l, "gui-pkg-err-unsafe", &[("path", p)]),
        Error::Manifest(why) => tf(l, "gui-pkg-err-manifest", &[("why", why)]),
        Error::Broken(why) => tf(l, "gui-pkg-err-broken", &[("why", why)]),
        Error::NotHttps => l.t("gui-pkg-err-https"),
        Error::Download(why) => tf(l, "gui-pkg-err-download", &[("why", why)]),
        Error::Io(why) => tf(l, "gui-pkg-err-io", &[("why", why)]),
    }
}

fn fail<T>(l: &Localizer, e: &Error) -> Result<T> {
    bail!("{}", explain(l, e))
}

fn load_index(l: &Localizer) -> Result<Index> {
    eprintln!("{}", l.t("cli-cat-reading"));
    match fetch_index(&index_url()) {
        Ok(i) => Ok(i),
        // The reason is already in words; "cannot read" frames it.
        Err(Error::Index(why)) => bail!("{}", tf(l, "cli-cat-err-index", &[("why", &why)])),
        Err(e) => bail!("{}", tf(l, "cli-cat-failed", &[("why", &explain(l, &e))])),
    }
}

/// What the package asks for, one line each, as the window's confirm dialog says it.
pub fn describe(l: &Localizer, c: &Candidate, author: &str) -> Vec<String> {
    let m = &c.manifest.manifest;
    let mut out = vec![tf(
        l,
        "cli-install-title",
        &[
            ("name", &m.name),
            ("version", &m.version),
            ("author", author),
        ],
    )];
    if let Some(old) = &c.replaces {
        out.push(tf(l, "cli-install-replaces", &[("version", old)]));
    }
    out.push(format!("  {}", l.t("cli-install-sees")));
    if c.asks.events.is_empty() {
        out.push(format!("    - {}", l.t("gui-asks-sees-nothing")));
    }
    for e in &c.asks.events {
        let (kind, detail) = e.split_once(':').unwrap_or((e.as_str(), ""));
        let what = l
            .try_t(&format!("gui-event-{}", kind.replace('_', "-")))
            .unwrap_or_else(|| kind.to_string());
        let which = if detail.is_empty() || detail == "*" {
            l.t("gui-event-all")
        } else {
            detail.to_string()
        };
        out.push(format!("    - {what}: {which}"));
    }
    out.push(format!("  {}", l.t("cli-install-can")));
    if c.asks.rights.is_empty() {
        out.push(format!("    - {}", l.t("gui-asks-can-nothing")));
    }
    for r in &c.asks.rights {
        let text = l
            .try_t(&format!("gui-right-{}", r.replace('_', "-")))
            .unwrap_or_else(|| r.clone());
        out.push(format!("    - {text} ({r})"));
    }
    if !c.asks.fetch.is_empty() {
        out.push(format!("  {}", l.t("cli-install-connects")));
        for h in &c.asks.fetch {
            out.push(format!("    - {h}"));
        }
    }
    out
}

/// Show the rights, then ask. `--yes` skips the question; without a terminal and without `--yes`
/// the answer is no (a script must say yes on purpose).
fn confirm(l: &Localizer, c: &Candidate, author: &str, yes: bool) -> Result<bool> {
    for line in describe(l, c, author) {
        println!("{line}");
    }
    if yes {
        return Ok(true);
    }
    if !std::io::stdin().is_terminal() {
        bail!("{}", l.t("cli-install-needs-yes"));
    }
    print!(
        "{} ",
        tf(
            l,
            "cli-install-confirm",
            &[("name", &c.manifest.manifest.name)]
        )
    );
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(matches!(
        line.trim().to_lowercase().as_str(),
        "y" | "yes" | "д" | "да"
    ))
}

/// Commit a staged package after the user's yes.
fn finish(l: &Localizer, s: &Store, c: Candidate, author: &str, yes: bool) -> Result<()> {
    match confirm(l, &c, author, yes) {
        Ok(true) => {}
        Ok(false) => {
            s.discard(&c);
            println!("{}", l.t("cli-install-declined"));
            return Ok(());
        }
        Err(e) => {
            s.discard(&c);
            return Err(e);
        }
    }
    let m = c.manifest.manifest.clone();
    if let Err(e) = s.commit(&c) {
        s.discard(&c);
        return fail(l, &e);
    }
    println!(
        "{}",
        tf(
            l,
            "cli-install-done",
            &[("name", &m.name), ("version", &m.version)]
        )
    );
    Ok(())
}

/// `phoecord install <id|url|path>`.
pub fn install(paths: &Paths, l: &Localizer, target: &str, yes: bool) -> Result<()> {
    let s = store(paths);
    if store::is_https(target) {
        println!("{}", l.t("cli-install-link-warning"));
        let staged = store::download(target).and_then(|b| s.stage_zip(&b, &store::verify_package));
        return match staged {
            Ok(c) => finish(l, &s, c, "?", yes),
            Err(e) => fail(l, &e),
        };
    }
    if Path::new(target).exists() {
        println!("{}", l.t("cli-install-link-warning"));
        return match s.stage_path(Path::new(target), &store::verify_package) {
            Ok(c) => finish(l, &s, c, "?", yes),
            Err(e) => fail(l, &e),
        };
    }
    let index = load_index(l)?;
    let Some(entry) = index.find(target) else {
        return fail(l, &Error::NotInCatalog(target.to_string()));
    };
    match catalog::stage_entry(&s, entry, &store::verify_package) {
        Ok(c) => finish(l, &s, c, &entry.author, yes),
        Err(e) => fail(l, &e),
    }
}

/// `phoecord update [id]`: install the newer catalog version of what is installed from it.
pub fn update(paths: &Paths, l: &Localizer, only: Option<&str>, yes: bool) -> Result<()> {
    let s = store(paths);
    let index = load_index(l)?;
    let mut ups = index.updates(&s.list());
    if let Some(id) = only {
        ups.retain(|u| u.id == id);
    }
    if ups.is_empty() {
        println!("{}", l.t("cli-update-none"));
        return Ok(());
    }
    for u in ups {
        println!(
            "{}",
            tf(
                l,
                "cli-update-available",
                &[("id", &u.id), ("from", &u.from), ("to", &u.to)]
            )
        );
        let Some(entry) = index.find(&u.id) else {
            continue;
        };
        match catalog::stage_entry(&s, entry, &store::verify_package) {
            Ok(c) => finish(l, &s, c, &entry.author, yes)?,
            // One tampered or broken download must not hide the other updates' answer.
            Err(e) => eprintln!("{}", explain(l, &e)),
        }
    }
    Ok(())
}

/// `phoecord remove <id>`.
pub fn remove(paths: &Paths, l: &Localizer, id: &str) -> Result<()> {
    let s = store(paths);
    if !s.list().iter().any(|p| p.id == id) {
        bail!("{}", tf(l, "cli-remove-missing", &[("id", id)]));
    }
    match s.remove(id) {
        Ok(id) => {
            println!("{}", tf(l, "cli-remove-done", &[("id", &id)]));
            Ok(())
        }
        Err(e) => fail(l, &e),
    }
}

/// `phoecord catalog`: every entry, and whether it is installed or has an update.
pub fn list(paths: &Paths, l: &Localizer) -> Result<()> {
    let s = store(paths);
    let index = load_index(l)?;
    if index.plugins.is_empty() {
        println!("{}", l.t("cli-cat-empty"));
        return Ok(());
    }
    let installed = s.list();
    let ups = index.updates(&installed);
    for e in &index.plugins {
        let state = if let Some(u) = ups.iter().find(|u| u.id == e.id) {
            tf(
                l,
                "cli-cat-state-update",
                &[("from", &u.from), ("to", &u.to)],
            )
        } else if installed
            .iter()
            .any(|p| p.id == e.id && Mark::read(&p.dir).is_some())
        {
            l.t("cli-cat-state-installed")
        } else {
            l.t("cli-cat-state-new")
        };
        println!(
            "{}",
            tf(
                l,
                "cli-cat-line",
                &[
                    ("id", &e.id),
                    ("version", &e.version),
                    ("name", &e.name),
                    ("author", &e.author),
                    ("state", &state),
                ]
            )
        );
        println!("    {}", e.description);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoecord_catalog::store::Store;

    const TOML: &str = "id = \"hello_wasm\"\nname = \"Hello\"\nversion = \"1.0.0\"\napi = 1\nevents = [\"dispatch:MESSAGE_CREATE\"]\nrights = [\"notify\", \"rest\"]\nfetch = [\"api.example.com\"]\n";

    fn staged(t: &Path) -> (Store, Candidate) {
        let dir = t.join("pkg");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("plugin.toml"), TOML).unwrap();
        std::fs::write(dir.join("plugin.wasm"), b"x").unwrap();
        let s = Store::new(&t.join("data"), &t.join("config.toml"));
        let c = s.stage_path(&dir, &|_: &Path| Ok(())).unwrap();
        (s, c)
    }

    #[test]
    fn the_rights_are_shown_in_both_languages() {
        let t = tempfile::tempdir().unwrap();
        let (_s, c) = staged(t.path());
        let en = describe(&Localizer::new("en"), &c, "Aigerim").join("\n");
        assert!(en.contains("Hello 1.0.0 by Aigerim asks for"), "{en}");
        assert!(en.contains("(notify)") && en.contains("(rest)"), "{en}");
        assert!(en.contains("api.example.com") && en.contains("MESSAGE_CREATE"));
        let ru = describe(&Localizer::new("ru"), &c, "Aigerim").join("\n");
        assert!(ru.contains("автор Aigerim"), "{ru}");
        assert_ne!(en, ru);
    }

    #[test]
    fn without_yes_and_without_a_terminal_nothing_is_installed() {
        let t = tempfile::tempdir().unwrap();
        let (s, c) = staged(t.path());
        let l = Localizer::new("en");
        // Test runs have no terminal on stdin.
        assert!(finish(&l, &s, c, "A", false).is_err());
        assert!(s.list().is_empty());
        assert!(!s.staging.exists());
    }

    #[test]
    fn yes_installs_and_remove_takes_it_away() {
        let t = tempfile::tempdir().unwrap();
        let (s, c) = staged(t.path());
        let l = Localizer::new("en");
        finish(&l, &s, c, "A", true).unwrap();
        assert_eq!(s.list().len(), 1);
        let paths = Paths {
            config: t.path().join("config.toml"),
            data: t.path().join("data"),
        };
        remove(&paths, &l, "hello_wasm").unwrap();
        assert!(s.list().is_empty());
        assert!(remove(&paths, &l, "hello_wasm").is_err());
    }
}
