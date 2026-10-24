#!/usr/bin/env bash
# Builds the Arch package dist/rustcut-<version>-<rel>-x86_64.pkg.tar.zst with makepkg (modeled on RustStart's).
# The version comes from [workspace.package] in Cargo.toml. The package is built without the default feature `slint-mcp`
# (no Slint MCP debugging server in a release, Q-D1); this script checks that. Install the result with:
#
#     sudo pacman -U dist/rustcut-<version>-<rel>-x86_64.pkg.tar.zst
#
# Options:  --check   run the test suite inside makepkg as well (slow; scripts/check-all.sh already runs it)
#           --keep    keep dist/build after a successful build (default: removed)
#           --lint    fail when namcap reports an error (scripts/check-all.sh uses it)
# Environment: PKGREL=<n> sets the package release number (default 1).
set -euo pipefail
cd "$(dirname "$0")"

run_check=0
keep=0
lint=0
for arg in "$@"; do
  case "$arg" in
    --check) run_check=1 ;;
    --keep) keep=1 ;;
    --lint) lint=1 ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

version=$(sed -n '/^\[workspace.package\]/,/^\[/{s/^version *= *"\(.*\)"/\1/p}' Cargo.toml)
[ -n "$version" ] || { echo "cannot read the version from Cargo.toml" >&2; exit 1; }
rel=${PKGREL:-1}
pkg="rustcut-${version}-${rel}-x86_64.pkg.tar.zst"

# Required tools: report what is missing with the exact install command instead of failing somewhere in the middle.
if ! scripts/check-deps.sh --quiet; then
  echo "Install the missing required packages first." >&2
  exit 1
fi
for tool in makepkg bsdtar sha256sum git; do
  command -v "$tool" >/dev/null || { echo "missing tool: $tool" >&2; exit 1; }
done

work=dist/build
rm -rf "$work"
mkdir -p "$work" dist

# Source archive: every tracked file plus new files that are not ignored (so uncommitted work is packaged, build output
# and local scratch files are not).
src="$work/rustcut-$version.tar.gz"
git ls-files -z --cached --others --exclude-standard \
  | bsdtar --null -T - -czf "$src" -s ",^,rustcut-$version/," --gid 0 --uid 0 2>/dev/null
sha=$(sha256sum "$src" | cut -d' ' -f1)

# Only the PKGBUILD's own fields: its package() fills @VERSION@ and @DATE@ into the metainfo itself.
sed -e "s/^pkgver=@VERSION@$/pkgver=$version/" -e "s/^pkgrel=@PKGREL@$/pkgrel=$rel/" -e "s/@SHA256@/$sha/" \
  packaging/PKGBUILD.in > "$work/PKGBUILD"

flags=(--force --noconfirm --cleanbuild)
[ "$run_check" = 1 ] || flags+=(--nocheck)
echo "Building $pkg ..."
(cd "$work" && PKGDEST="$PWD/.." makepkg "${flags[@]}")

[ -f "dist/$pkg" ] || { echo "makepkg did not produce dist/$pkg" >&2; exit 1; }

# Content check: every file the desktop needs, and no MCP server in the binary.
problems=()
content=$(bsdtar -tf "dist/$pkg")
for want in usr/bin/rustcut \
  usr/share/applications/io.github.vommie.rustcut.desktop \
  usr/share/metainfo/io.github.vommie.rustcut.metainfo.xml \
  usr/share/icons/hicolor/scalable/apps/rustcut.svg \
  usr/share/icons/hicolor/16x16/apps/rustcut.png \
  usr/share/icons/hicolor/512x512/apps/rustcut.png \
  usr/share/licenses/rustcut/LICENSE; do
  grep -qx "$want" <<<"$content" || problems+=("missing $want")
done
if bsdtar -xOf "dist/$pkg" usr/bin/rustcut | grep -qa SLINT_MCP_PORT; then
  problems+=("usr/bin/rustcut contains Slint's MCP server (built with the feature slint-mcp)")
fi
if bsdtar -xOf "dist/$pkg" usr/share/metainfo/io.github.vommie.rustcut.metainfo.xml | grep -q '@VERSION@\|@DATE@'; then
  problems+=("the metainfo still has a placeholder")
fi

echo
echo "== Package content"
bsdtar -tvf "dist/$pkg" | grep -v '/$' | awk '{ printf "  %s %s %s\n", $1, $5, $9 }'
echo
echo "== Metadata"
pacman -Qip "dist/$pkg" | sed -n '1,/^Install Script/p' | grep -E '^(Name|Version|Architecture|Installed Size|Depends On|Optional Deps|License)' -A0

namcap_failed=0
if command -v namcap >/dev/null; then
  echo
  echo "== namcap (PKGBUILD)"
  pkgbuild_report=$(namcap "$work/PKGBUILD" || true)
  printf '%s\n' "$pkgbuild_report"
  echo "== namcap (package)"
  package_report=$(namcap "dist/$pkg" || true)
  printf '%s\n' "$package_report"
  # namcap prints "E:" for errors and "W:" for warnings; only errors fail a --lint run.
  if printf '%s\n%s\n' "$pkgbuild_report" "$package_report" | grep -q ' E: '; then namcap_failed=1; fi
else
  echo "namcap is not installed, so the package was not linted (sudo pacman -S namcap)."
  [ "$lint" = 1 ] && namcap_failed=1
fi

if [ "${#problems[@]}" -gt 0 ]; then
  echo
  for p in "${problems[@]}"; do echo "PROBLEM: $p" >&2; done
  exit 1
fi
if [ "$lint" = 1 ] && [ "$namcap_failed" = 1 ]; then
  echo "namcap reported errors (see above)." >&2
  exit 1
fi

[ "$keep" = 1 ] || rm -rf "$work"
echo
echo "Done: dist/$pkg"
echo "Install with: sudo pacman -U dist/$pkg"
