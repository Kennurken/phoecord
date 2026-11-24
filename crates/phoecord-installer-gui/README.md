# phoecord-installer-gui

The PhoeCord installer window (`phoecord-installer`, stage 1.4.3): terms of use, install / repair /
uninstall per Discord client, and plugin packages, without a terminal. All changes to the machine go
through `phoecord-installer`, the same code `phoecord setup` and `phoecord uninstall` run.

The window starts the `phoecord` binary for autostart, shortcuts and "Open Discord", so ship both
files in one folder (or point `PHOECORD_BIN` at it).

```bash
cargo build -p phoecord-cli -p phoecord-installer-gui
./target/debug/phoecord-installer                    # the real thing
./target/debug/phoecord-installer --dry-run --demo   # sandbox + two made-up Discord installs
./target/debug/phoecord-installer --dry-run --open path/to/plugin.zip
```

`--dry-run` keeps every folder under `PHOECORD_GUI_SANDBOX` (default `<temp>/phoecord-gui-dry-run`)
and simulates the trust store, registry, launchd and systemd; nothing outside the sandbox changes.

Look: one palette in `src/theme.rs`, motion in `src/motion.rs` (interruptible tweens, custom
easing, "Reduce motion" in Settings). Font: Onest (SIL OFL 1.1, `LICENSES/OFL-onest.txt`); icons:
Phosphor (MIT, through `egui-phosphor`).
