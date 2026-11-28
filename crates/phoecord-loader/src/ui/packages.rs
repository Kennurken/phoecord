//! Installing and removing WASM plugin packages from the PhoeCord tab in Phoenix (task 2.3.3,
//! owner's ruling 2026-11-27, phoecord#206: option a1).
//!
//! The loader links `phoecord-catalog::Store`, the same store `phoecord install` and the window
//! use: a local package (a `.zip`, or a folder with `plugin.toml` and `plugin.wasm`) is staged and
//! checked ([`Packages::stage`]), the tab shows what it asks for ([`confirm`], plain data like the
//! rest of the tab), and only on the user's yes is it moved into `plugins/` and switched on
//! ([`Packages::install`]); a no throws it away ([`Packages::discard`]). [`Packages::remove`]
//! deletes an installed package with its data and its `config.toml` entry ([`remove_confirm`] is
//! the question before it).
//!
//! After a change the running plugins follow it ([`Live`]): in an in-process launch the loader's
//! own engine applies the saved config and loads or unloads the package; on the proxy transport
//! the proxy watches `plugins/` and `config.toml` itself, so nothing more is needed.
//!
//! The community catalog from the tab (download and SHA-256 check, a2) comes after a separate
//! review; nothing here downloads.

use phoecord_catalog::store::{self, Candidate, Error, Store};
use phoecord_transport::config::{Config, Paths};
use std::path::Path;

/// A string of PhoeCord's bundle with its arguments; `None` when the bundle lacks the key. Behind
/// a closure so tests need no bundles.
pub type Words<'a> = &'a dyn Fn(&str, &[(&str, &str)]) -> Option<String>;

fn say(tr: Words<'_>, key: &str, args: &[(&str, &str)]) -> String {
    tr(key, args).unwrap_or_else(|| key.to_string())
}

/// What the tab shows before installing a staged package, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Confirm {
    /// "Install NAME?"
    pub title: String,
    /// "1.2.0 · by Alice, Bob", or the version alone.
    pub byline: String,
    pub description: String,
    /// "Replaces the installed version 1.1.0." for an update.
    pub replaces: Option<String>,
    pub sees_title: String,
    /// The events it reads, in words, each with which ones ("all of them" or the names).
    pub sees: Vec<String>,
    pub can_title: String,
    /// Its rights in words, each followed by the right's name as written in `plugin.toml`.
    pub can: Vec<String>,
    /// "It can connect to", only when it lists hosts.
    pub connects_title: Option<String>,
    pub connects: Vec<String>,
    /// "Install plugins only from people you trust …"
    pub trust: String,
    pub yes: String,
    pub no: String,
}

/// The confirm view for a staged package.
pub fn confirm(c: &Candidate, tr: Words<'_>) -> Confirm {
    let m = &c.manifest.manifest;
    let byline = if m.authors.is_empty() {
        m.version.clone()
    } else {
        format!(
            "{} · {} {}",
            m.version,
            say(tr, "phoenix-settings-by", &[]),
            m.authors.join(", ")
        )
    };
    let mut sees: Vec<String> = c
        .asks
        .events
        .iter()
        .map(|e| {
            let (kind, detail) = e.split_once(':').unwrap_or((e.as_str(), ""));
            let what = tr(&format!("gui-event-{}", kind.replace('_', "-")), &[])
                .unwrap_or_else(|| kind.to_string());
            let which = if detail.is_empty() || detail == "*" {
                say(tr, "gui-event-all", &[])
            } else {
                detail.to_string()
            };
            format!("{what}: {which}")
        })
        .collect();
    if sees.is_empty() {
        sees.push(say(tr, "gui-asks-sees-nothing", &[]));
    }
    let mut can: Vec<String> = c
        .asks
        .rights
        .iter()
        .map(|r| {
            let text =
                tr(&format!("gui-right-{}", r.replace('_', "-")), &[]).unwrap_or_else(|| r.clone());
            format!("{text} ({r})")
        })
        .collect();
    if can.is_empty() {
        can.push(say(tr, "gui-asks-can-nothing", &[]));
    }
    Confirm {
        title: say(tr, "gui-confirm-plugin-title", &[("name", &m.name)]),
        byline,
        description: m.description.clone(),
        replaces: c
            .replaces
            .as_ref()
            .map(|v| say(tr, "gui-confirm-plugin-replaces", &[("version", v)])),
        sees_title: say(tr, "gui-asks-sees", &[]),
        sees,
        can_title: say(tr, "gui-asks-can", &[]),
        can,
        connects_title: (!c.asks.fetch.is_empty()).then(|| say(tr, "gui-asks-connects", &[])),
        connects: c.asks.fetch.clone(),
        trust: say(tr, "gui-confirm-plugin-trust", &[]),
        yes: say(tr, "gui-confirm-install", &[]),
        no: say(tr, "gui-cancel", &[]),
    }
}

/// The question before removing an installed package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoveConfirm {
    pub title: String,
    pub body: String,
    pub yes: String,
    pub no: String,
}

pub fn remove_confirm(name: &str, tr: Words<'_>) -> RemoveConfirm {
    RemoveConfirm {
        title: say(tr, "gui-remove-title", &[("name", name)]),
        body: say(tr, "gui-remove-body", &[]),
        yes: say(tr, "gui-remove", &[]),
        no: say(tr, "gui-cancel", &[]),
    }
}

/// Plain words for a package error, with what to do next (the window's and the CLI's strings).
pub fn error_text(e: &Error, tr: Words<'_>) -> String {
    match e {
        Error::NotAPackage => say(tr, "gui-pkg-err-not-package", &[]),
        Error::NoWasm => say(tr, "gui-pkg-err-no-wasm", &[]),
        Error::TooBig => say(
            tr,
            "gui-pkg-err-too-big",
            &[("mb", &(store::MAX_BYTES / 1024 / 1024).to_string())],
        ),
        Error::UnsafePath(p) => say(tr, "gui-pkg-err-unsafe", &[("path", p)]),
        Error::Manifest(why) => say(tr, "gui-pkg-err-manifest", &[("why", why)]),
        Error::Broken(why) => say(tr, "gui-pkg-err-broken", &[("why", why)]),
        Error::NotHttps => say(tr, "gui-pkg-err-https", &[]),
        Error::Download(why) => say(tr, "gui-pkg-err-download", &[("why", why)]),
        Error::Io(why) => say(tr, "gui-pkg-err-io", &[("why", why)]),
        Error::Index(why) => say(tr, "gui-pkg-err-index", &[("why", why)]),
        Error::NotInCatalog(_) => say(tr, "gui-pkg-err-not-in-catalog", &[]),
        Error::HashMismatch => say(tr, "gui-pkg-err-hash", &[]),
        Error::CatalogMismatch(why) => say(tr, "gui-pkg-err-catalog-mismatch", &[("why", why)]),
    }
}

/// The real strings: `l`'s bundle, the language of the tab.
pub fn words(l: &phoecord_i18n::Localizer) -> impl Fn(&str, &[(&str, &str)]) -> Option<String> {
    move |key, args| {
        if args.is_empty() {
            return l.try_t(key);
        }
        l.try_t(key)?;
        let mut a = phoecord_i18n::Args::new();
        for (k, v) in args {
            a.set(*k, v.to_string());
        }
        Some(l.t_args(key, &a))
    }
}

/// What makes the running plugins follow a change on disk.
pub trait Live {
    /// `config.toml` and `plugins/` changed.
    fn reload(&self, config: &Path);
}

/// The proxy transport: the proxy watches the files itself.
pub struct ProxyWatches;

impl Live for ProxyWatches {
    fn reload(&self, _: &Path) {}
}

/// An in-process launch: the engine applies the saved config, then loads new packages and drops
/// removed ones (compiling a package takes a while: call this off the client's UI thread).
impl Live for phoecord_transport::Engine {
    fn reload(&self, config: &Path) {
        if let Ok(cfg) = Config::load(config) {
            self.apply(cfg);
        }
        self.sync_packages();
    }
}

/// Checks that a staged package's code loads; [`Packages::new`] uses the WASM runtime's check.
pub type Verify = Box<store::Verify>;

/// The package store of this installation, as the tab uses it.
pub struct Packages {
    pub store: Store,
    verify: Verify,
}

impl Packages {
    /// The store in `paths` (`data/plugins`, `config.toml`), with the real code check.
    pub fn new(paths: &Paths) -> Packages {
        Packages::with_verify(
            Store::new(&paths.data, &paths.config),
            Box::new(store::verify_package),
        )
    }

    pub fn with_verify(store: Store, verify: Verify) -> Packages {
        Packages { store, verify }
    }

    /// Stages and checks the package at `src` (a `.zip`, a package folder, or its `plugin.toml`
    /// or `plugin.wasm`). Nothing is installed yet.
    pub fn stage(&self, src: &Path) -> Result<Candidate, Error> {
        self.store.stage_path(src, &*self.verify)
    }

    /// The user said no: the staged package is thrown away.
    pub fn discard(&self, c: &Candidate) {
        self.store.discard(c);
    }

    /// The user said yes: the package goes into `plugins/<id>` (a first install is switched on, an
    /// update keeps its data and its switch) and the running plugins follow. Returns its id.
    pub fn install(&self, c: &Candidate, live: &dyn Live) -> Result<String, Error> {
        let id = self.store.commit(c)?;
        live.reload(&self.store.config);
        Ok(id)
    }

    /// Deletes installed package `id` with its data and its `config.toml` entry; the running
    /// plugins drop it.
    pub fn remove(&self, id: &str, live: &dyn Live) -> Result<String, Error> {
        let id = self.store.remove(id)?;
        live.reload(&self.store.config);
        Ok(id)
    }
}

/// The keys this module reads from PhoeCord's bundle, so a test can check both languages have
/// them.
pub const KEYS: &[&str] = &[
    "phoenix-settings-by",
    "gui-confirm-plugin-title",
    "gui-confirm-plugin-replaces",
    "gui-confirm-plugin-trust",
    "gui-confirm-install",
    "gui-cancel",
    "gui-asks-sees",
    "gui-asks-sees-nothing",
    "gui-asks-can",
    "gui-asks-can-nothing",
    "gui-asks-connects",
    "gui-event-all",
    "gui-remove-title",
    "gui-remove-body",
    "gui-remove",
    "gui-pkg-err-not-package",
    "gui-pkg-err-no-wasm",
    "gui-pkg-err-too-big",
    "gui-pkg-err-unsafe",
    "gui-pkg-err-manifest",
    "gui-pkg-err-broken",
    "gui-pkg-err-https",
    "gui-pkg-err-download",
    "gui-pkg-err-io",
    "gui-pkg-err-index",
    "gui-pkg-err-not-in-catalog",
    "gui-pkg-err-hash",
    "gui-pkg-err-catalog-mismatch",
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Keys come back as `key(arg=value,…)`, so a test sees which string and which arguments.
    fn echo(key: &str, args: &[(&str, &str)]) -> Option<String> {
        if key.starts_with("gui-event-") && key != "gui-event-all" && key != "gui-event-dispatch" {
            return None;
        }
        if args.is_empty() {
            return Some(key.to_string());
        }
        let a: Vec<String> = args.iter().map(|(k, v)| format!("{k}={v}")).collect();
        Some(format!("{key}({})", a.join(",")))
    }

    const MANIFEST: &str = r#"
id = "hello"
name = "Hello"
version = "1.2.0"
description = "Says hello."
authors = ["Alice", "Bob"]
api = 1
events = ["dispatch:MESSAGE_CREATE", "send"]
rights = ["kv", "fetch"]
fetch = ["api.example.com"]
"#;

    fn package(dir: &Path, manifest: &str) -> std::path::PathBuf {
        let p = dir.join("src-hello");
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("plugin.toml"), manifest).unwrap();
        std::fs::write(p.join("plugin.wasm"), b"\0asm").unwrap();
        p
    }

    fn packages(dir: &Path) -> Packages {
        Packages::with_verify(
            Store::new(&dir.join("data"), &dir.join("config.toml")),
            Box::new(|_: &Path| Ok(())),
        )
    }

    #[derive(Default)]
    struct Count(RefCell<u32>);
    impl Live for Count {
        fn reload(&self, _: &Path) {
            *self.0.borrow_mut() += 1;
        }
    }

    #[test]
    fn the_confirm_view_says_what_the_package_asks_for() {
        let dir = tempfile::tempdir().unwrap();
        let p = packages(dir.path());
        let c = p.stage(&package(dir.path(), MANIFEST)).unwrap();
        let v = confirm(&c, &echo);
        assert_eq!(v.title, "gui-confirm-plugin-title(name=Hello)");
        assert_eq!(v.byline, "1.2.0 · phoenix-settings-by Alice, Bob");
        assert_eq!(v.replaces, None);
        assert_eq!(
            v.sees,
            ["gui-event-dispatch: MESSAGE_CREATE", "send: gui-event-all"]
        );
        assert_eq!(v.can, ["gui-right-kv (kv)", "gui-right-fetch (fetch)"]);
        assert_eq!(v.connects_title.as_deref(), Some("gui-asks-connects"));
        assert_eq!(v.connects, ["api.example.com"]);
        assert_eq!(
            (v.yes.as_str(), v.no.as_str()),
            ("gui-confirm-install", "gui-cancel")
        );
        p.discard(&c);
        assert!(!dir.path().join("data/plugins-staging").exists());
        assert!(!dir.path().join("data/plugins/hello").exists());
    }

    #[test]
    fn a_package_without_events_or_rights_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let p = packages(dir.path());
        let bare =
            "id = \"hello\"\nname = \"Hello\"\nversion = \"1.0.0\"\ndescription = \"x\"\napi = 1\n";
        let c = p.stage(&package(dir.path(), bare)).unwrap();
        let v = confirm(&c, &echo);
        assert_eq!(v.byline, "1.0.0");
        assert_eq!(v.sees, ["gui-asks-sees-nothing"]);
        assert_eq!(v.can, ["gui-asks-can-nothing"]);
        assert_eq!(v.connects_title, None);
        p.discard(&c);
    }

    #[test]
    fn install_switches_it_on_and_the_engine_follows_then_remove_undoes_it() {
        let dir = tempfile::tempdir().unwrap();
        let p = packages(dir.path());
        let live = Count::default();
        let c = p.stage(&package(dir.path(), MANIFEST)).unwrap();
        assert_eq!(p.install(&c, &live).unwrap(), "hello");
        assert!(dir.path().join("data/plugins/hello/plugin.wasm").is_file());
        let cfg = Config::load(&dir.path().join("config.toml")).unwrap();
        assert!(cfg.is_enabled("hello"));
        assert_eq!(*live.0.borrow(), 1);

        // An update says which version it replaces.
        let again = p.stage(&package(dir.path(), MANIFEST)).unwrap();
        assert_eq!(
            confirm(&again, &echo).replaces.as_deref(),
            Some("gui-confirm-plugin-replaces(version=1.2.0)")
        );
        p.discard(&again);

        assert_eq!(p.remove("hello", &live).unwrap(), "hello");
        assert!(!dir.path().join("data/plugins/hello").exists());
        let cfg = Config::load(&dir.path().join("config.toml")).unwrap();
        assert!(!cfg.plugins.contains_key("hello"));
        assert_eq!(*live.0.borrow(), 2);
    }

    #[test]
    fn a_refused_package_installs_nothing_and_the_error_says_what_to_do() {
        let dir = tempfile::tempdir().unwrap();
        let p = packages(dir.path());
        let src = package(dir.path(), MANIFEST);
        std::fs::remove_file(src.join("plugin.wasm")).unwrap();
        let e = p.stage(&src).unwrap_err();
        assert_eq!(e, Error::NoWasm);
        assert_eq!(error_text(&e, &echo), "gui-pkg-err-no-wasm");
        assert!(!dir.path().join("data/plugins").exists());
        assert_eq!(
            error_text(&Error::TooBig, &echo),
            format!("gui-pkg-err-too-big(mb={})", store::MAX_BYTES / 1024 / 1024)
        );
        assert_eq!(
            error_text(&Error::Broken("trap".into()), &echo),
            "gui-pkg-err-broken(why=trap)"
        );
    }

    #[test]
    fn the_engine_applies_the_saved_config_on_reload() {
        use phoecord_plugin_runtime::Chain;
        use phoecord_transport::{Engine, host::Shared};
        let dir = tempfile::tempdir().unwrap();
        let chain = Chain::new(vec![Box::new(phoecord_plugins::no_track::NoTrack::new())]);
        let engine = Engine::new(
            chain,
            Shared::new(dir.path().join("plugins")),
            Config::default(),
        );
        let path = dir.path().join("config.toml");
        let mut cfg = Config::default();
        cfg.set_enabled("no_track", false).unwrap();
        cfg.save(&path).unwrap();
        Live::reload(&engine, &path);
        assert!(!engine.chain.enabled_ids().contains(&"no_track".to_string()));
        cfg.set_enabled("no_track", true).unwrap();
        cfg.save(&path).unwrap();
        Live::reload(&engine, &path);
        assert!(engine.chain.enabled_ids().contains(&"no_track".to_string()));
    }

    #[test]
    fn the_remove_question() {
        let q = remove_confirm("Hello", &echo);
        assert_eq!(q.title, "gui-remove-title(name=Hello)");
        assert_eq!(
            (q.yes.as_str(), q.no.as_str()),
            ("gui-remove", "gui-cancel")
        );
    }

    #[test]
    fn both_languages_have_every_key() {
        for lang in ["en", "ru"] {
            let l = phoecord_i18n::Localizer::new(lang);
            for key in KEYS {
                assert!(l.try_t(key).is_some(), "{lang} lacks {key}");
            }
        }
    }

    #[test]
    fn the_real_words_fill_in_arguments() {
        let l = phoecord_i18n::Localizer::new("en");
        let w = words(&l);
        let title = w("gui-confirm-plugin-title", &[("name", "Hello")]).unwrap();
        assert!(title.contains("Hello"), "{title}");
        assert_eq!(w("no-such-key", &[]), None);
        assert_eq!(w("no-such-key", &[("a", "b")]), None);
    }
}
