//! Embeds the app icon and version info in `phoecord-installer.exe`. Windows shows the icon in the
//! taskbar, Explorer and Settings > Apps (the uninstall entry's `DisplayIcon` points at this exe);
//! the window's own `with_icon` covers only the title bar. Other targets need nothing.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/icon/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/icon/icon.ico")
        .set("ProductName", "PhoeCord")
        .set("FileDescription", "PhoeCord")
        .set(
            "LegalCopyright",
            "Copyright (C) 2026 Cheterin Group. GPL-3.0-or-later.",
        );
    res.compile().expect("embedding the Windows icon resource");
}
