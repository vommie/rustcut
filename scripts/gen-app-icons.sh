#!/usr/bin/env bash
# Renders the app icon in every size the desktop and the package need, and the window and tray icons the app embeds,
# from packaging/rustcut.svg, plus the colorless symbol of the empty player (P9.9). Needs `rsvg-convert` (package
# librsvg) and `magick` (imagemagick). The PNGs are committed, so a normal build does not need them; run this after
# changing the SVG.
set -euo pipefail
cd "$(dirname "$0")/.."
SVG=packaging/rustcut.svg
OUT=crates/rustcut/assets/app
command -v rsvg-convert >/dev/null || { echo "rsvg-convert is missing: sudo pacman -S librsvg" >&2; exit 1; }
command -v magick >/dev/null || { echo "magick is missing: sudo pacman -S imagemagick" >&2; exit 1; }
mkdir -p "$OUT" packaging/icons
for s in 16 22 24 32 48 64 128 256 512; do
  rsvg-convert -w "$s" -h "$s" "$SVG" -o "packaging/icons/rustcut-$s.png"
done
command cp -f packaging/icons/rustcut-256.png "$OUT/window-icon.png"
command cp -f packaging/icons/rustcut-64.png "$OUT/tray-icon.png"
# The empty player's symbol: the icon without color, at twice the 96 px it is shown at (HiDPI).
rsvg-convert -w 192 -h 192 "$SVG" | magick - -colorspace Gray "$OUT/empty-player.png"
echo "icons written to packaging/icons and $OUT"
