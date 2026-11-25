//! `phoecord dev watch <plugin source folder>` (task 1.3.10).
//!
//! Builds the plugin for `wasm32-wasip2`, copies `plugin.toml`, `plugin.wasm` and `i18n/*.ftl`
//! into `plugins/<id>/` of the data folder, and does it again whenever a source file changes.
//! The running proxy follows that folder with file-system notifications and reloads the package.

use crate::tf;
use anyhow::{Context, Result, bail};
use clap::Args;
use phoecord_i18n::Localizer;
use phoecord_plugin_runtime::package::PackageManifest;
use phoecord_plugin_runtime::watch::DebouncedWatcher;
use phoecord_proxy::config::Paths;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Quiet time after the last source change before a build starts.
const SOURCE_DEBOUNCE: Duration = Duration::from_millis(200);

#[derive(Args)]
pub struct WatchArgs {
    /// The plugin's source folder (holds `Cargo.toml` and `plugin.toml`).
    source: PathBuf,
    /// Install into this folder instead of the `plugins` folder of the data folder.
    #[arg(long, value_name = "DIR")]
    plugins_dir: Option<PathBuf>,
    /// Build and install once, then exit.
    #[arg(long)]
    once: bool,
}

pub fn run(args: WatchArgs, paths: &Paths, l10n: &Localizer) -> Result<()> {
    let shown = args.source.display().to_string();
    let source = args
        .source
        .canonicalize()
        .with_context(|| tf(l10n, "dev-err-source", &[("path", &shown)]))?;
    let id = plugin_id(&source).with_context(|| l10n.t("dev-err-manifest"))?;
    let dest = args
        .plugins_dir
        .unwrap_or_else(|| paths.data.join("plugins"))
        .join(&id);
    println!(
        "{}",
        tf(
            l10n,
            "dev-watching",
            &[("id", &id), ("path", &dest.display().to_string())]
        )
    );

    let (tx, rx) = mpsc::channel::<()>();
    // The watcher starts before the first build so a save during that build is not lost.
    let _watcher = if args.once {
        None
    } else {
        let base = source.clone();
        let started = DebouncedWatcher::start(
            &source,
            SOURCE_DEBOUNCE,
            move |p| is_source_file(&base, p),
            move |_| {
                let _ = tx.send(());
            },
        );
        Some(
            started
                .map_err(|e| anyhow::anyhow!(e))
                .with_context(|| l10n.t("dev-err-watch"))?,
        )
    };

    let ok = build_and_install(&source, &dest, l10n);
    if args.once {
        return if ok {
            Ok(())
        } else {
            bail!(l10n.t("dev-err-build"))
        };
    }
    println!("{}", l10n.t("dev-waiting"));
    while rx.recv().is_ok() {
        // One build covers every change that arrived while the last one ran.
        while rx.try_recv().is_ok() {}
        build_and_install(&source, &dest, l10n);
        println!("{}", l10n.t("dev-waiting"));
    }
    Ok(())
}

/// One round: build, then install. A failure is reported and the watching goes on.
fn build_and_install(source: &Path, dest: &Path, l10n: &Localizer) -> bool {
    let started = Instant::now();
    println!("{}", l10n.t("dev-building"));
    let wasm = match build(source) {
        Ok(w) => w,
        Err(e) => {
            let why = format!("{e:#}");
            eprintln!("{}", tf(l10n, "dev-build-failed", &[("why", &why)]));
            return false;
        }
    };
    let built = started.elapsed();
    match install(source, dest, &wasm) {
        Ok(()) => {
            let (build, copy) = (
                built.as_millis().to_string(),
                (started.elapsed() - built).as_millis().to_string(),
            );
            println!(
                "{}",
                tf(l10n, "dev-installed", &[("build", &build), ("copy", &copy)])
            );
            true
        }
        Err(e) => {
            let why = format!("{e:#}");
            eprintln!("{}", tf(l10n, "dev-install-failed", &[("why", &why)]));
            false
        }
    }
}

/// The `id` of `source/plugin.toml`, validated like the runtime does.
fn plugin_id(source: &Path) -> Result<String> {
    let file = source.join("plugin.toml");
    let text = std::fs::read_to_string(&file).with_context(|| format!("{}", file.display()))?;
    let m = PackageManifest::parse(&text).map_err(|e| anyhow::anyhow!("plugin.toml: {e}"))?;
    Ok(m.manifest.id)
}

/// Whether a change at `path` can alter the package: not build output, not hidden files.
fn is_source_file(source: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(source) else {
        return false;
    };
    match rel.iter().next().and_then(|c| c.to_str()) {
        None | Some("target") => false,
        Some(first) if first.starts_with('.') => false,
        Some(_) => !path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with('~') || n.ends_with(".tmp") || n.ends_with(".swp")),
    }
}

/// `cargo build --target wasm32-wasip2 --release` in `source`; returns the `.wasm` it produced.
fn build(source: &Path) -> Result<PathBuf> {
    let mut child = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args([
            "build",
            "--target",
            "wasm32-wasip2",
            "--release",
            "--message-format=json-render-diagnostics",
        ])
        .current_dir(source)
        .stdout(Stdio::piped())
        .spawn()
        .context("cannot run cargo")?;
    let out = std::io::read_to_string(child.stdout.take().context("no cargo output")?)?;
    let status = child.wait()?;
    if !status.success() {
        bail!("cargo build failed ({status})");
    }
    wasm_from_cargo_output(&out).context("cargo built no .wasm file (is the crate a cdylib?)")
}

/// The last `.wasm` named by a `compiler-artifact` line of cargo's JSON output.
fn wasm_from_cargo_output(out: &str) -> Option<PathBuf> {
    out.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["reason"] == "compiler-artifact")
        .filter_map(|v| {
            v["filenames"]
                .as_array()?
                .iter()
                .filter_map(|f| f.as_str())
                .find(|f| f.ends_with(".wasm"))
                .map(PathBuf::from)
        })
        .next_back()
}

/// Put the package in `dest`. Every file is written next to its final name and renamed into
/// place, so the proxy never reads a half-written file.
fn install(source: &Path, dest: &Path, wasm: &Path) -> Result<()> {
    std::fs::create_dir_all(dest.join("i18n"))?;
    place(&source.join("plugin.toml"), &dest.join("plugin.toml"))?;
    let mut kept = Vec::new();
    for entry in std::fs::read_dir(source.join("i18n"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let p = entry.path();
        if p.extension().is_some_and(|e| e == "ftl")
            && let Some(name) = p.file_name()
        {
            place(&p, &dest.join("i18n").join(name))?;
            kept.push(name.to_owned());
        }
    }
    // A string file deleted from the source is deleted from the install.
    for entry in std::fs::read_dir(dest.join("i18n"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let p = entry.path();
        if p.extension().is_some_and(|e| e == "ftl")
            && p.file_name().is_some_and(|n| !kept.iter().any(|k| k == n))
        {
            let _ = std::fs::remove_file(p);
        }
    }
    // The wasm goes last: it is the file whose arrival people wait for.
    place(wasm, &dest.join("plugin.wasm"))
}

fn place(from: &Path, to: &Path) -> Result<()> {
    let tmp = to.with_file_name(format!(
        "{}.tmp",
        to.file_name().and_then(|n| n.to_str()).unwrap_or("file")
    ));
    std::fs::copy(from, &tmp).with_context(|| format!("copy {}", from.display()))?;
    // Windows refuses to replace a file another process has open for a moment: try again.
    let mut last = None;
    for _ in 0..10 {
        match std::fs::rename(&tmp, to) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(Duration::from_millis(30));
            }
        }
    }
    let _ = std::fs::remove_file(&tmp);
    Err(last.expect("the loop ran")).with_context(|| format!("replace {}", to.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_sources_trigger_a_build() {
        let s = Path::new("/p");
        assert!(is_source_file(s, &s.join("src/lib.rs")));
        assert!(is_source_file(s, &s.join("plugin.toml")));
        assert!(is_source_file(s, &s.join("i18n/ru.ftl")));
        assert!(!is_source_file(
            s,
            &s.join("target/wasm32-wasip2/release/x.wasm")
        ));
        assert!(!is_source_file(s, &s.join(".git/index")));
        assert!(!is_source_file(s, &s.join("src/lib.rs~")));
        assert!(!is_source_file(s, &s.join("src/.lib.rs.swp")));
        assert!(!is_source_file(s, s));
    }

    #[test]
    fn the_wasm_is_found_in_cargo_output() {
        let out = r#"{"reason":"compiler-artifact","filenames":["/t/a.rlib"]}
{"reason":"compiler-artifact","filenames":["/t/x_wasm.wasm"]}
{"reason":"build-finished","success":true}
not json"#;
        assert_eq!(
            wasm_from_cargo_output(out),
            Some(PathBuf::from("/t/x_wasm.wasm"))
        );
        assert_eq!(wasm_from_cargo_output("{}"), None);
    }

    #[test]
    fn install_copies_the_package_and_drops_stale_strings() {
        let tmp = std::env::temp_dir().join(format!("phoecord-dev-watch-{}", std::process::id()));
        let (src, dest) = (tmp.join("src"), tmp.join("out/p"));
        std::fs::create_dir_all(src.join("i18n")).unwrap();
        std::fs::write(src.join("plugin.toml"), "t").unwrap();
        std::fs::write(src.join("i18n/en.ftl"), "a = 1").unwrap();
        std::fs::write(tmp.join("m.wasm"), "w").unwrap();
        std::fs::create_dir_all(dest.join("i18n")).unwrap();
        std::fs::write(dest.join("i18n/old.ftl"), "x").unwrap();
        install(&src, &dest, &tmp.join("m.wasm")).unwrap();
        assert_eq!(
            std::fs::read_to_string(dest.join("plugin.wasm")).unwrap(),
            "w"
        );
        assert!(dest.join("i18n/en.ftl").is_file());
        assert!(!dest.join("i18n/old.ftl").exists());
        assert!(!dest.join("plugin.wasm.tmp").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
