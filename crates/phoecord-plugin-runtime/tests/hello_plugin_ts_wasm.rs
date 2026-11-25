//! The TypeScript template `examples/hello-plugin-ts` builds with `componentize-js`, loads as a
//! package and behaves like the Rust example: it logs every `MESSAGE_CREATE`.
//!
//! It needs Node.js and npm (`npm ci` downloads the build tools), so it is ignored by default:
//! `cargo test -p phoecord-plugin-runtime --test hello_plugin_ts_wasm -- --ignored`. The CI job
//! `typescript-template` runs it.

use phoecord_core::Val;
use phoecord_plugin_runtime::package::PackageSet;
use phoecord_plugin_runtime::wasm::WasmRuntime;
use phoecord_plugin_runtime::{Chain, NullHost, Outcome};
use std::path::Path;
use std::process::Command;

mod common;
use common::TempDir;

fn map(fields: Vec<(&str, Val)>) -> Val {
    Val::Map(
        fields
            .into_iter()
            .map(|(k, v)| (Val::Str(k.into()), v))
            .collect(),
    )
}

fn message(id: &str) -> Val {
    let text = |s: &str| Val::Str(s.into());
    let d = map(vec![
        ("id", text(id)),
        ("channel_id", text("42")),
        ("content", text("secret text")),
    ]);
    map(vec![
        ("op", Val::Int(0)),
        ("t", text("MESSAGE_CREATE")),
        ("d", d),
    ])
}

/// `npm <args>` in `dir` (`npm.cmd` on Windows).
fn npm(dir: &Path, args: &[&str]) {
    let program = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let out = Command::new(program)
        .args(args)
        .current_dir(dir)
        .output()
        .expect("cannot run npm: install Node.js to build the TypeScript template");
    assert!(
        out.status.success(),
        "npm {args:?} failed\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
#[ignore = "needs Node.js and npm; run with --ignored"]
fn the_typescript_template_loads_and_logs_messages() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello-plugin-ts");
    npm(&dir, &["ci"]);
    npm(&dir, &["run", "build"]);

    let root = TempDir::new();
    let dest = root.0.join("hello_plugin_ts");
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::copy(dir.join("plugin.toml"), dest.join("plugin.toml")).unwrap();
    std::fs::copy(dir.join("plugin.wasm"), dest.join("plugin.wasm")).unwrap();

    let chain = Chain::default();
    let mut set = PackageSet::new(WasmRuntime::new().unwrap(), root.0.clone());
    let started = std::time::Instant::now();
    let logs = set.sync(&chain);
    // The engine is compiled to native code on load; for a JavaScript component that is the
    // slow part. Shown with `--nocapture`.
    eprintln!("loaded the package in {:?}", started.elapsed());
    assert_eq!(chain.ids(), vec!["hello_plugin_ts"], "{logs:?}");

    let mut host = NullHost::default();
    let mut extra = Vec::new();
    for id in ["7", "8"] {
        let mut payload = message(id);
        let out = chain.dispatch(&mut host, "MESSAGE_CREATE", &mut payload, &mut extra);
        assert_eq!(out, Outcome::Untouched);
        assert_eq!(payload, message(id));
    }
    let mine: Vec<&String> = host
        .logs
        .iter()
        .filter(|l| l.contains("MESSAGE_CREATE"))
        .collect();
    assert_eq!(mine.len(), 2, "{:?}", host.logs);
    assert!(mine[0].contains("#1: message 7 in channel 42"), "{mine:?}");
    assert!(mine[1].contains("#2: message 8"), "{mine:?}");
    assert!(!host.logs.iter().any(|l| l.contains("secret")));
}
