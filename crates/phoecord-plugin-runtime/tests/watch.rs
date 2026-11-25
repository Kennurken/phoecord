//! File-system notifications: a changed, added or removed package is picked up at once, and a
//! burst of writes is one callback.

mod common;

use common::{TempDir, write_package};
use phoecord_core::EventKind;
use phoecord_plugin_runtime::Chain;
use phoecord_plugin_runtime::package::PackageSet;
use phoecord_plugin_runtime::watch::{DebouncedWatcher, is_package_file};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

const MESSAGE_CREATE: EventKind = EventKind::Dispatch {
    name: "MESSAGE_CREATE",
};
const TYPING_START: EventKind = EventKind::Dispatch {
    name: "TYPING_START",
};

fn watch(
    root: &std::path::Path,
    debounce: Duration,
) -> (DebouncedWatcher, mpsc::Receiver<Vec<PathBuf>>) {
    let (tx, rx) = mpsc::channel();
    let base = root.to_path_buf();
    let w = DebouncedWatcher::start(
        root,
        debounce,
        move |p| is_package_file(&base, p),
        move |paths| {
            let _ = tx.send(paths);
        },
    )
    .expect("watcher starts");
    (w, rx)
}

/// Wait up to `ms` for a callback.
fn next(rx: &mpsc::Receiver<Vec<PathBuf>>, ms: u64) -> Option<Vec<PathBuf>> {
    rx.recv_timeout(Duration::from_millis(ms)).ok()
}

#[test]
fn change_add_and_remove_are_reported() {
    let root = TempDir::new();
    let dir = write_package(&root.0, "a", 1, "");
    let (_w, rx) = watch(&root.0, Duration::from_millis(30));

    std::fs::write(dir.join("plugin.toml"), "id = \"a\"\n").unwrap();
    let got = next(&rx, 3000).expect("change reported");
    assert!(got.iter().any(|p| p.ends_with("plugin.toml")), "{got:?}");

    write_package(&root.0, "b", 1, "");
    let got = next(&rx, 3000).expect("add reported");
    assert!(
        got.iter().any(|p| p.starts_with(root.0.join("b"))),
        "{got:?}"
    );

    std::fs::remove_dir_all(&dir).unwrap();
    let got = next(&rx, 3000).expect("removal reported");
    assert!(got.iter().any(|p| p.starts_with(&dir)), "{got:?}");
}

#[test]
fn a_burst_of_writes_is_one_callback() {
    let root = TempDir::new();
    let dir = write_package(&root.0, "a", 1, "");
    let (_w, rx) = watch(&root.0, Duration::from_millis(150));

    // Ten writes 10 ms apart are one burst.
    for i in 0..10 {
        std::fs::write(dir.join("plugin.wasm"), vec![i as u8; 16]).unwrap();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(next(&rx, 3000).is_some(), "burst reported");
    assert!(next(&rx, 600).is_none(), "burst is one callback");
}

#[test]
fn a_notified_sync_reloads_loads_and_unloads_without_waiting_for_an_event() {
    let root = TempDir::new();
    let dir = write_package(&root.0, "hot", 1, "events = [\"dispatch:MESSAGE_*\"]");
    let chain = Chain::new(vec![]);
    let mut set = PackageSet::new(common::runtime(), root.0.clone());
    let (_w, rx) = watch(&root.0, Duration::from_millis(30));
    set.sync(&chain);
    chain.set_enabled(chain.ids());
    assert!(chain.wants(&MESSAGE_CREATE));
    assert!(!chain.wants(&TYPING_START));

    // Change: the new subscription is live right after the notification, no event needed.
    std::fs::write(
        dir.join("plugin.toml"),
        "id = \"hot\"\nname = \"hot\"\nversion = \"0.2.0\"\napi = 1\nevents = [\"dispatch:TYPING_*\"]\n",
    )
    .unwrap();
    next(&rx, 3000).expect("notified");
    let logs = set.sync(&chain);
    assert!(logs.iter().any(|l| l.contains("reloaded")), "{logs:?}");
    chain.set_enabled(chain.ids());
    assert!(chain.wants(&TYPING_START) && !chain.wants(&MESSAGE_CREATE));

    // Add.
    write_package(&root.0, "fresh", 1, "events = [\"dispatch\"]");
    next(&rx, 3000).expect("notified");
    let logs = set.sync(&chain);
    assert!(logs.iter().any(|l| l.contains("fresh loaded")), "{logs:?}");
    assert!(chain.ids().contains(&"fresh".to_string()));

    // Remove.
    std::fs::remove_dir_all(root.0.join("fresh")).unwrap();
    next(&rx, 3000).expect("notified");
    let logs = set.sync(&chain);
    assert!(logs.iter().any(|l| l.contains("unloaded")), "{logs:?}");
    assert!(!chain.ids().contains(&"fresh".to_string()));
}

/// A root reached through a symlink: macOS reports the real path (`/private/var/...`), and the
/// callback must still see paths under the root as given.
#[cfg(unix)]
#[test]
fn a_root_behind_a_symlink_still_reports_its_changes() {
    let real = TempDir::new();
    let dir = write_package(&real.0, "a", 1, "");
    let link_parent = TempDir::new();
    let link = link_parent.0.join("plugins");
    std::os::unix::fs::symlink(&real.0, &link).unwrap();
    let (_w, rx) = watch(&link, Duration::from_millis(30));
    std::fs::write(dir.join("plugin.toml"), "id = \"a\"\n").unwrap();
    let got = next(&rx, 3000).expect("change reported");
    assert!(got.iter().all(|p| p.starts_with(&link)), "{got:?}");
}
