//! File-system notifications for plugin packages (task 1.3.10, fast hot reload).
//!
//! [`DebouncedWatcher`] watches a folder with the operating system's notifications (the `notify`
//! crate) and calls back once per burst of changes: an editor or `cargo` writes a file in several
//! steps, and one reload per burst is enough. [`is_package_file`] says which paths of a `plugins/`
//! folder matter (`plugin.toml`, `plugin.wasm`, `i18n/*.ftl` and the package folders themselves).
//!
//! Notifications can fail (a network drive, a full inotify limit, a removed folder). Callers keep
//! a slow scan as a fallback and check [`DebouncedWatcher::is_healthy`].

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// A burst is cut after this long even if changes keep coming, so a noisy folder still reloads.
const MAX_BURST: Duration = Duration::from_secs(2);

/// Calls `on_change` with the changed paths after the folder has been quiet for `debounce`.
/// Dropping it stops the watching.
pub struct DebouncedWatcher {
    _watcher: RecommendedWatcher,
    healthy: Arc<AtomicBool>,
}

impl DebouncedWatcher {
    /// Watch `root` recursively. Only paths for which `relevant` is true count. `on_change` runs
    /// on a background thread. Fails when the operating system refuses to watch `root`.
    pub fn start(
        root: &Path,
        debounce: Duration,
        relevant: impl Fn(&Path) -> bool + Send + 'static,
        on_change: impl Fn(Vec<PathBuf>) + Send + 'static,
    ) -> Result<DebouncedWatcher, String> {
        let (tx, rx) = mpsc::channel::<Result<notify::Event, notify::Error>>();
        let healthy = Arc::new(AtomicBool::new(true));
        let mut watcher = notify::recommended_watcher(move |res| {
            let _ = tx.send(res);
        })
        .map_err(|e| e.to_string())?;
        watcher
            .watch(root, RecursiveMode::Recursive)
            .map_err(|e| format!("{}: {e}", root.display()))?;
        let flag = healthy.clone();
        std::thread::Builder::new()
            .name("phoecord-fs-watch".into())
            .spawn(move || {
                // Ends when the watcher (and with it the sender) is dropped.
                while let Ok(first) = rx.recv() {
                    let mut paths = Vec::new();
                    let mut take = |res: Result<notify::Event, notify::Error>| match res {
                        Ok(ev) => {
                            if !matches!(ev.kind, EventKind::Access(_)) {
                                paths.extend(ev.paths.into_iter().filter(|p| relevant(p)));
                            }
                        }
                        Err(_) => flag.store(false, Ordering::SeqCst),
                    };
                    take(first);
                    let started = Instant::now();
                    while started.elapsed() < MAX_BURST {
                        match rx.recv_timeout(debounce) {
                            Ok(res) => take(res),
                            Err(_) => break,
                        }
                    }
                    if !paths.is_empty() {
                        paths.sort();
                        paths.dedup();
                        on_change(paths);
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(DebouncedWatcher {
            _watcher: watcher,
            healthy,
        })
    }

    /// False once the operating system reported a watching error: fall back to scanning.
    pub fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::SeqCst)
    }
}

/// Whether `path` (under the packages folder `root`) is something a package loads or the
/// package folder itself: `<id>`, `<id>/plugin.toml`, `<id>/plugin.wasm`, `<id>/i18n` and
/// `<id>/i18n/*.ftl`.
pub fn is_package_file(root: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else {
        return false;
    };
    let parts: Vec<&str> = rel.iter().filter_map(|c| c.to_str()).collect();
    match parts.as_slice() {
        [_] => true,
        [_, "plugin.toml" | "plugin.wasm" | "i18n"] => true,
        [_, "i18n", f] => f.ends_with(".ftl"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_files_are_recognised() {
        let root = Path::new("/data/plugins");
        let yes = |p: &str| is_package_file(root, &root.join(p));
        assert!(yes("x"));
        assert!(yes("x/plugin.wasm"));
        assert!(yes("x/plugin.toml"));
        assert!(yes("x/i18n"));
        assert!(yes("x/i18n/ru.ftl"));
        assert!(!yes("x/i18n/ru.txt"));
        assert!(!yes("x/plugin.wasm.tmp"));
        assert!(!yes("x/data/y"));
        assert!(!is_package_file(root, Path::new("/elsewhere/x")));
    }
}
