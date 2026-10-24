#!/usr/bin/env bash
# Renders the app icon in every size the desktop and the package need, and the window and tray icons the app embeds,
# from packaging/rustcut.svg. Needs `rsvg-convert` (package librsvg). The PNGs are committed, so a normal build does
# not need it; run this after changing the SVG.
set -euo pipefail
cd "$(dirname "$0")/.."
SVG=packaging/rustcut.svg
OUT=crates/rustcut/assets/app
command -v rsvg-convert >/dev/null || { echo "rsvg-convert is missing: sudo pacman -S librsvg" >&2; exit 1; }
mkdir -p "$OUT" packaging/icons
for s in 16 22 24 32 48 64 128 256 512; do
  rsvg-convert -w "$s" -h "$s" "$SVG" -o "packaging/icons/rustcut-$s.png"
done
command cp -f packaging/icons/rustcut-256.png "$OUT/window-icon.png"
command cp -f packaging/icons/rustcut-64.png "$OUT/tray-icon.png"
echo "icons written to packaging/icons and $OUT"
