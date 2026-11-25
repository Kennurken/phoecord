//! Measures hot-reload latency on the real example `examples/plugins/clear_urls`: from the moment
//! `plugin.wasm` is replaced (what `phoecord dev watch` does after a build) to the moment the
//! running chain has the new version. Run it in release mode for the number that matters:
//!
//! ```text
//! cargo test --release -p phoecord-plugin-runtime --test watch_latency -- --ignored --nocapture
//! ```

mod common;

use common::TempDir;
use phoecord_plugin_runtime::Chain;
use phoecord_plugin_runtime::package::PackageSet;
use phoecord_plugin_runtime::watch::{DebouncedWatcher, is_package_file};
use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[test]
#[ignore = "timing measurement; builds the example plugin"]
fn reload_after_the_wasm_is_replaced() {
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugins/clear_urls");
    let out = Command::new(env!("CARGO"))
        .args(["build", "--release", "--target", "wasm32-wasip2"])
        .current_dir(&example)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    assert!(out.status.success());
    let built = example.join("target/wasm32-wasip2/release/clear_urls_wasm.wasm");

    let root = TempDir::new();
    let dest = root.0.join("clear_urls_wasm");
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::copy(example.join("plugin.toml"), dest.join("plugin.toml")).unwrap();
    std::fs::copy(&built, dest.join("plugin.wasm")).unwrap();

    let chain = Chain::new(vec![]);
    let mut set = PackageSet::new(common::runtime(), root.0.clone());
    set.sync(&chain);
    assert!(chain.ids().contains(&"clear_urls_wasm".to_string()));

    let (tx, rx) = mpsc::channel();
    let base = root.0.clone();
    // The same debounce the proxy uses (50 ms).
    let _w = DebouncedWatcher::start(
        &root.0,
        Duration::from_millis(50),
        move |p| is_package_file(&base, p),
        move |_| {
            let _ = tx.send(());
        },
    )
    .unwrap();

    let bytes = std::fs::read(&built).unwrap();
    let mut ms = Vec::new();
    for _ in 0..15 {
        std::fs::write(dest.join("plugin.wasm.tmp"), &bytes).unwrap();
        let t = Instant::now();
        std::fs::rename(dest.join("plugin.wasm.tmp"), dest.join("plugin.wasm")).unwrap();
        rx.recv_timeout(Duration::from_secs(5)).expect("notified");
        let notified = t.elapsed();
        let logs = set.sync(&chain);
        assert!(logs.iter().any(|l| l.contains("reloaded")), "{logs:?}");
        let total = t.elapsed();
        println!("notified after {notified:?}, reloaded after {total:?}");
        ms.push(total.as_millis());
        while rx.try_recv().is_ok() {}
        std::thread::sleep(Duration::from_millis(200));
        while rx.try_recv().is_ok() {}
    }
    ms.sort();
    println!(
        "reload latency ms: min {} median {} max {}",
        ms[0],
        ms[ms.len() / 2],
        ms[ms.len() - 1]
    );
}
