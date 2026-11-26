//! Task 2.3.7: the three degradation scenarios of `docs/research/phoenix-recon.md` ("Degradation"),
//! automated with fake locator results and a fake `.text`. What these do not cover is the run
//! inside a real Phoenix process; that stays a manual check on Windows.

use phoecord_loader::plan::{EntryPoint, Plan, Reason, Route, Target, UiFeature};
use phoecord_loader::signatures::{ResolveError, SignatureSet, resolve_all};
use phoecord_loader::version::{ClientKind, ClientVersion};

const TEXT_RVA: u32 = 0x1000;

fn version(file_version: &str) -> ClientVersion {
    ClientVersion {
        file_version: Some(file_version.into()),
        text_hash: "00".into(),
        text_rva: TEXT_RVA,
        text_len: 0x100,
    }
}

/// A fake `.text` with a few unique byte runs to match.
fn text() -> Vec<u8> {
    let mut t = vec![0xCCu8; 0x100];
    t[0x10..0x14].copy_from_slice(&[0x48, 0x89, 0x5C, 0x24]);
    t[0x40..0x44].copy_from_slice(&[0x55, 0x41, 0x57, 0x41]);
    t[0x80..0x84].copy_from_slice(&[0x56, 0x57, 0x48, 0x83]);
    t
}

const TABLE: &str = r#"
[build."0.1.0.2208"]
http_send = { aob = "48 89 5C 24" }
gateway_registry = { aob = "55 41 57 41" }
theme_palette = { aob = "56 57 48 83" }
"#;

/// Scenario 1: no table for this build. Every entry point stays with the proxy (network plugins
/// keep working there) and the UI layer is off; nothing runs in-process.
#[test]
fn scenario_1_unknown_build_is_proxy_only() {
    let set = SignatureSet::parse(TABLE).unwrap();
    assert!(set.select(&version("0.1.0.9999")).is_none());

    let plan = Plan::unknown_build();
    assert!(plan.proxy_only());
    assert!(!plan.all_in_process());
    for p in EntryPoint::ALL {
        assert_eq!(plan.route(p), &Route::Proxy(Reason::UnknownBuild));
    }
    for f in UiFeature::ALL {
        assert_eq!(plan.ui_off(f), Some(&Reason::UnknownBuild));
    }
}

/// Scenario 2, signature level: a table for this build with one pattern that matches nothing
/// gives exactly one error, and every other signature still resolves.
#[test]
fn scenario_2_one_broken_signature_fails_alone() {
    let broken = TABLE.replace(
        "[build.\"0.1.0.2208\"]",
        "[build.\"0.1.0.2208\"]\nmissing_on_purpose = { aob = \"DE AD BE EF DE AD BE EF DE AD BE EF\" }",
    );
    let set = SignatureSet::parse(&broken).unwrap();
    let (key, sigs) = set.select(&version("0.1.0.2208")).unwrap();
    assert_eq!(key, "0.1.0.2208");
    let results = resolve_all(sigs, &text(), TEXT_RVA);
    assert_eq!(results.len(), 4);
    let failed: Vec<_> = results.iter().filter(|(_, r)| r.is_err()).collect();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].0, "missing_on_purpose");
    assert_eq!(failed[0].1, Err(ResolveError::NotFound));
    for (name, r) in &results {
        match *name {
            "http_send" => assert_eq!(*r, Ok(TEXT_RVA + 0x10)),
            "gateway_registry" => assert_eq!(*r, Ok(TEXT_RVA + 0x40)),
            "theme_palette" => assert_eq!(*r, Ok(TEXT_RVA + 0x80)),
            _ => {}
        }
    }
}

/// Scenario 2, plan level: one missing locator turns off its own feature or entry point (and
/// what cannot work without it), nothing else. Because the transport is all or nothing per
/// launch, any missing network locator sends the whole launch to the proxy.
#[test]
fn scenario_2_one_missing_locator_costs_only_its_feature() {
    let only = |missing: Target| {
        move |t: Target| -> Result<(), String> {
            if t == missing {
                Err("signature missing_on_purpose: no match".into())
            } else {
                Ok(())
            }
        }
    };

    // A UI feature: only that feature is off, the network stays in-process.
    let plan = Plan::resolve(only(Target::Ui(UiFeature::ContextMenu)));
    assert!(plan.all_in_process());
    for f in UiFeature::ALL {
        assert_eq!(
            plan.ui_off(f).is_some(),
            f == UiFeature::ContextMenu,
            "{}",
            f.name()
        );
    }

    // The gateway registry: dispatch and send (which needs it) leave, REST and the UI stay.
    let plan = Plan::resolve(only(Target::Net(EntryPoint::Dispatch)));
    assert_eq!(plan.route(EntryPoint::Request), &Route::InProcess);
    assert_eq!(plan.route(EntryPoint::Response), &Route::InProcess);
    assert!(matches!(
        plan.route(EntryPoint::Dispatch),
        Route::Proxy(Reason::Locator(_))
    ));
    assert_eq!(
        plan.route(EntryPoint::Send),
        &Route::Proxy(Reason::Needs(EntryPoint::Dispatch))
    );
    assert!(UiFeature::ALL.iter().all(|&f| plan.ui_off(f).is_none()));
    assert!(!plan.all_in_process());
    assert!(!plan.proxy_only());
}

/// Scenario 3: the loader beside an Electron client recognises it is not Phoenix, so no Phoenix
/// hook is attempted and the client runs as without the loader.
#[test]
fn scenario_3_electron_client_is_not_phoenix() {
    for exe in [
        r"C:\Users\a\AppData\Local\Discord\app-1.0.9200\Discord.exe",
        r"C:\Users\a\AppData\Local\DiscordCanary\app-1.0.1364\DiscordCanary.exe",
        r"C:\Users\a\AppData\Local\DiscordPTB\app-1.0.1200\DiscordPTB.exe",
    ] {
        assert_eq!(
            ClientKind::of(exe, Some("1.0.9200.0")),
            ClientKind::Electron
        );
    }
    assert_eq!(
        ClientKind::of(
            r"C:\Users\a\AppData\Local\DiscordPhoenix\app\Discord.exe",
            Some("0.1.0.2208")
        ),
        ClientKind::Phoenix
    );
}
