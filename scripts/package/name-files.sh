#!/usr/bin/env bash
# Give the installers their release names. cargo-packager names the NSIS .exe and the WiX .msi after
# the main binary (phoecord-installer_<version>_x64-setup.exe); users get PhoeCord_<version>_... like
# the .dmg, which already uses the product name. Run after `cargo packager`; idempotent.
# Usage: bash scripts/package/name-files.sh [dir]   (default: target/packages)
set -euo pipefail
cd "${1:-target/packages}"
shopt -s nullglob
for f in phoecord-installer_*.exe phoecord-installer_*.msi phoecord-installer_*.dmg; do
  mv -f -- "$f" "PhoeCord_${f#phoecord-installer_}"
  echo "PhoeCord_${f#phoecord-installer_}"
done
