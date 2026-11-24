# Native installers

The installer window ships as a real installer on each OS, built by
[`cargo-packager`](https://github.com/crabnebula-dev/cargo-packager) from the
`[package.metadata.packager]` table in `crates/phoecord-installer-gui/Cargo.toml`:

| OS | Files | Notes |
|:---|:---|:---|
| Windows | `PhoeCord_<version>_x64-setup.exe` (NSIS), `PhoeCord_<version>_x64_en-US.msi` / `_ru-RU.msi` (WiX) | Per-user install, no administrator prompt; Start menu entry and uninstaller |
| macOS | `PhoeCord_<version>_<arch>.dmg` with `PhoeCord.app` | Drag into Applications; the DMG shows the GPL first |

Both binaries go in: the window (`phoecord-installer`) and the proxy (`phoecord`), side by side,
because the window looks for `phoecord` next to itself.

## Build locally

```bash
cargo install cargo-packager --locked --version 0.11.8
cd crates/phoecord-installer-gui
cargo packager --release -f app -f dmg      # macOS
cargo packager --release -f nsis -f wix     # Windows (downloads NSIS and WiX on first run)
```

Results land in `target/packages/`. CI builds the same on every PR that touches the installer and
on `v*` tags (`.github/workflows/package.yml`) and keeps them as workflow artifacts.

## Artwork

`python scripts/package/make_art.py` renders the app icons (`assets/icon/`) and the installer
pictures (`assets/installer/`: NSIS header and sidebar, WiX banner and dialog, DMG background) from
`assets/icon/icon.svg`. It needs `rsvg-convert` and Pillow (`iconutil` on macOS for the `.icns`).
Commit what it writes; packaging only reads those files.

## Not signed yet

Releases are not code-signed (stage 1.4.5). Windows SmartScreen and macOS Gatekeeper warn on first
start: on macOS, right-click PhoeCord.app → Open. Check downloads against `SHA256SUMS`.
