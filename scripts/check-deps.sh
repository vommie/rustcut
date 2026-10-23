#!/usr/bin/env bash
# Verifies everything needed to build, test, run and package RustCut on Arch/Manjaro.
# Nothing is skipped silently: every missing item is listed with the package that provides it, followed by the exact
# `sudo pacman -S` command. Exit status: 0 = all required items present, 1 = a required item is missing.
#
#   scripts/check-deps.sh            check everything
#   scripts/check-deps.sh --quiet    print only what is missing
set -uo pipefail
cd "$(dirname "$0")/.."

quiet=0
[ "${1:-}" = "--quiet" ] && quiet=1

if [ -t 1 ]; then ok=$'\e[32mok     \e[0m'; req=$'\e[31mMISSING\e[0m'; opt=$'\e[33mmissing\e[0m'; else ok="ok     "; req="MISSING"; opt="missing"; fi

missing_required=()
missing_optional=()

# check KIND TEST PACKAGE DESCRIPTION
#   KIND: required | optional
#   TEST: "cmd:<command>" (found in PATH), "pkg:<package>" (installed pacman package) or "pc:<module>" (pkg-config module)
check() {
  local kind="$1" test="$2" package="$3" what="$4" found=0
  case "$test" in
    cmd:*) command -v "${test#cmd:}" >/dev/null 2>&1 && found=1 ;;
    pkg:*) pacman -Qq "${test#pkg:}" >/dev/null 2>&1 && found=1 ;;
    pc:*) pkg-config --exists "${test#pc:}" 2>/dev/null && found=1 ;;
  esac
  if [ "$found" = 1 ]; then
    [ "$quiet" = 1 ] || printf '  %s %-24s %s\n' "$ok" "${test#*:}" "$what"
  elif [ "$kind" = required ]; then
    printf '  %s %-24s %s  [package: %s]\n' "$req" "${test#*:}" "$what" "$package"
    missing_required+=("$package")
  else
    printf '  %s %-24s %s  [package: %s]\n' "$opt" "${test#*:}" "$what" "$package"
    missing_optional+=("$package")
  fi
}

section() { [ "$quiet" = 1 ] || printf '\n%s\n' "$1"; }

section "Build tools"
check required cmd:rustc      rustup  "Rust compiler (see rust-version in Cargo.toml)"
check required cmd:cargo      rustup  "Cargo"
check required cmd:cc         gcc     "C compiler/linker for native dependencies (bundled SQLite)"
check required cmd:clang      clang   "bindgen generates the libmpv bindings with it"
check required cmd:pkg-config pkgconf "finds the system libraries"
check required cmd:git        git     "source control, used to collect the package sources"
if command -v rustc >/dev/null 2>&1; then
  have=$(rustc --version | sed -n 's/^rustc \([0-9]*\.[0-9]*\).*/\1/p')
  want=$(sed -n 's/^rust-version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)
  if [ -n "$have" ] && [ -n "$want" ] && [ "$(printf '%s\n%s\n' "$want" "$have" | sort -V | head -1)" != "$want" ]; then
    printf '  %s %-24s rustc %s is older than the required %s\n' "$req" "rustc version" "$have" "$want"
    missing_required+=("rustup")
  fi
fi

section "Libraries (build and run time)"
check required pc:mpv             mpv              "libmpv, the player"
check required pkg:fontconfig     fontconfig       "font lookup"
check required pkg:freetype2      freetype2        "font rendering"
check required pkg:libxkbcommon   libxkbcommon     "keyboard handling"
check required pkg:libx11         libx11           "X11 session"
check required pkg:libxcb         libxcb           "X11 session"
check required pkg:libxcursor     libxcursor       "X11 mouse cursors"
check required pkg:libxi          libxi            "X11 input"
check required pkg:libxrender     libxrender       "X11 rendering"
check required pkg:libxkbcommon-x11 libxkbcommon-x11 "keyboard handling on X11"
check required pkg:wayland        wayland          "Wayland session"
check required pkg:dbus           dbus             "notifications, taskbar progress, sleep inhibitor, tray"
check required pkg:libglvnd       libglvnd         "OpenGL loader (the UI and the player render with OpenGL)"
check optional pkg:mesa           mesa             "OpenGL driver (open source drivers; the proprietary NVIDIA driver works too)"

section "Media tools (run time)"
check required cmd:ffmpeg  ffmpeg   "renders the jobs"
check required cmd:ffprobe ffmpeg   "reads streams and durations, verifies the rendered files"
check optional pkg:vid.stab vid.stab "deshake filter (vidstabdetect/vidstabtransform in ffmpeg)"
# RustView is not in the Arch repositories: it comes as a package from its GitHub releases.
rustview_missing=0
if command -v rustview >/dev/null 2>&1; then
  [ "$quiet" = 1 ] || printf '  %s %-24s %s\n' "$ok" rustview "Tag & Rate: the RustView library (rustview api)"
else
  printf '  %s %-24s %s  [package: rustview from github.com/vommie/rustview releases]\n' "$opt" rustview \
    "Tag & Rate: the RustView library (rustview api)"
  rustview_missing=1
fi

section "Packaging"
check required cmd:makepkg               pacman             "builds the .pkg.tar.zst"
check required cmd:bsdtar                libarchive         "creates the source archive and lists package contents"
check optional cmd:namcap                namcap             "lints the PKGBUILD and the package"
check optional cmd:desktop-file-validate desktop-file-utils "validates the desktop file"
check optional cmd:appstreamcli          appstream          "validates the AppStream metainfo"

section "Quality checks"
check optional cmd:cargo-audit cargo-audit "checks dependencies for known vulnerabilities"
check optional cmd:cargo-deny  cargo-deny  "checks licenses, bans and advisories (deny.toml)"

section "Visual tests (only needed to take screenshots of the running app)"
check optional cmd:xdotool xdotool     "clicks, typing and window lookup"
check optional cmd:import  imagemagick "window screenshots"
check optional cmd:xprop   xorg-xprop  "window properties"
check optional cmd:wmctrl  wmctrl      "closes the test window like the user does (scripts/session.sh)"
check optional cmd:python3 python      "talks to Slint's MCP server of a running test instance (scripts/mcp.py)"

section "Debugging and profiling (optional)"
check optional cmd:samply         samply         "sampling profiler (cargo build --profile profiling, samply record)"
check optional cmd:heaptrack      heaptrack      "heap allocation profiler"
check optional cmd:hyperfine      hyperfine      "command-line benchmarks"
check optional cmd:cargo-nextest  cargo-nextest  "faster test runner (cargo nextest run)"
check optional cmd:cargo-llvm-cov cargo-llvm-cov "test coverage (cargo llvm-cov)"
check optional cmd:cargo-machete  cargo-machete  "finds unused dependencies"
check optional cmd:bacon          bacon          "background check/clippy/test runner"
check optional cmd:kooha          kooha          "screen recording of visual checks"

section "Run time services (optional features)"
check optional pkg:xdg-desktop-portal     xdg-desktop-portal     "file dialogs"
check optional pkg:xdg-desktop-portal-kde xdg-desktop-portal-kde "native Plasma file dialogs"

print_command() {
  local -n list=$1
  [ "${#list[@]}" -gt 0 ] || return 0
  mapfile -t uniq < <(printf '%s\n' "${list[@]}" | sort -u)
  echo
  echo "$2"
  echo
  echo "    sudo pacman -S --needed ${uniq[*]}"
}

if [ "$rustview_missing" = 1 ]; then
  echo
  echo "RustView is not installed (Tag & Rate stays disabled until it is, or until its path is set in the settings). Install it with:"
  echo
  echo "    gh release download --repo vommie/rustview --pattern rustview-x86_64.pkg.tar.zst --dir /tmp && sudo pacman -U /tmp/rustview-x86_64.pkg.tar.zst"
fi
if [ "${#missing_required[@]}" -eq 0 ] && [ "${#missing_optional[@]}" -eq 0 ]; then
  [ "$quiet" = 1 ] || [ "$rustview_missing" = 1 ] || { echo; echo "Everything is installed."; }
  exit 0
fi
print_command missing_required "Required packages are missing. Install them with:"
print_command missing_optional "Optional packages are missing (the matching checks or features are unavailable until they are installed):"
[ "${#missing_required[@]}" -eq 0 ]
