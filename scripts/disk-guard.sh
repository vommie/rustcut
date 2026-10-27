#!/usr/bin/env bash
# Keeps the disk of the build from running full (night questionnaire Q-P9n "disk" A; in the night of 2026-10-26 /home ran
# full when target/debug/incremental had grown to 145 GB). Below RUSTCUT_DISK_MIN_GB (default 40) GB free on the disk
# that holds target/, it deletes target/debug/incremental (the compiler's incremental caches: the next build takes a few
# minutes longer, nothing else is lost) and says so loudly. With enough space it does nothing and prints nothing.
#
#   scripts/disk-guard.sh            check and clean up (check-all.sh and rc-testenv run it before they build)
#   scripts/disk-guard.sh --dry-run  only say what would happen
set -uo pipefail
repo="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")/.." && pwd)"
target="${CARGO_TARGET_DIR:-$repo/target}"
incremental="$target/debug/incremental"
min_gb="${RUSTCUT_DISK_MIN_GB:-40}"
dry=0
[ "${1:-}" = "--dry-run" ] && dry=1

if [ -t 2 ]; then y=$'\e[1;33m'; n=$'\e[0m'; else y=""; n=""; fi
loud() { printf '%s[disk-guard] %s%s\n' "$y" "$*" "$n" >&2; }

# Free space in whole GB on the disk of DIR (its nearest existing parent).
free_gb() {
  local dir="$1"
  while [ ! -e "$dir" ]; do dir="$(dirname "$dir")"; done
  df -P -B1G "$dir" | awk 'NR == 2 { print $4 }'
}

free=$(free_gb "$target")
if ! [[ "$free" =~ ^[0-9]+$ ]]; then
  loud "could not read the free space of $target (df said '$free'); nothing cleaned up"
  exit 0
fi
[ "$free" -ge "$min_gb" ] && exit 0

if [ ! -d "$incremental" ]; then
  loud "only $free GB free on the disk of $target (limit $min_gb GB), and there is no $incremental to delete:"
  loud "free space yourself (du -sh $target/*; cargo clean)"
  exit 0
fi
size=$(du -sh "$incremental" 2>/dev/null | cut -f1)
if [ "$dry" -eq 1 ]; then
  loud "only $free GB free (limit $min_gb GB): would delete $incremental ($size)"
  exit 0
fi
loud "only $free GB free on the disk of $target (limit $min_gb GB): deleting $incremental ($size)"
rm -rf -- "$incremental"
loud "now $(free_gb "$target") GB free; the next build rebuilds the incremental caches (a few minutes longer)"
exit 0
