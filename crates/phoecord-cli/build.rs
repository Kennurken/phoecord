//! Embeds the app icon and version info in `phoecord.exe` (the proxy that autostart and the
//! shortcuts run), so Task Manager and Explorer show the PhoeCord icon for it too. The icon file is
//! shared with the installer window. Other targets need nothing.

const ICON: &str = "../phoecord-installer-gui/assets/icon/icon.ico";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={ICON}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon(ICON)
        .set("ProductName", "PhoeCord")
        .set("FileDescription", "PhoeCord proxy")
        .set(
            "LegalCopyright",
            "Copyright (C) 2026 Cheterin Group. GPL-3.0-or-later.",
        );
    res.compile().expect("embedding the Windows icon resource");
}
