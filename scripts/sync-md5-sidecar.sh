#!/usr/bin/env bash
# Keeps crates/md5-sidecar byte-identical with its canonical copy in RustKnownClean (crates/md5-sidecar/SPEC.md, rule 9).
# The same script lives in every app that vendors the crate.
#
#   scripts/sync-md5-sidecar.sh          replace this repo's crates/md5-sidecar with the canonical copy
#   scripts/sync-md5-sidecar.sh --check  only compare (scripts/check-all.sh); exit 1 on drift
#
# The canonical copy is $MD5_SIDECAR_CANONICAL or `crates/md5-sidecar` of a RustKnownClean checkout next to this repository (`../rustknown`). Without it (another machine)
# the script says so and exits 0: the check is skipped. In the repo that holds the canonical copy there is nothing to do.
set -euo pipefail
cd "$(dirname "$0")/.."

here="$PWD/crates/md5-sidecar"
canonical="${MD5_SIDECAR_CANONICAL:-$PWD/../rustknown/crates/md5-sidecar}"
check=0
case "${1:-}" in
  --check) check=1 ;;
  "") ;;
  *) echo "usage: $0 [--check]" >&2; exit 64 ;;
esac

if [ ! -f "$canonical/SPEC.md" ] || [ ! -f "$canonical/Cargo.toml" ]; then
  echo "md5-sidecar: SKIPPED, no canonical copy at $canonical (set MD5_SIDECAR_CANONICAL to compare)"
  exit 0
fi
if [ "$(realpath "$canonical")" = "$(realpath -m "$here")" ]; then
  echo "md5-sidecar: this repo holds the canonical copy ($here)"
  exit 0
fi

if [ "$check" = 1 ]; then
  if diff -r "$canonical" "$here" >/dev/null 2>&1; then
    echo "md5-sidecar: identical with $canonical"
    exit 0
  fi
  echo "md5-sidecar: crates/md5-sidecar differs from the canonical copy $canonical:" >&2
  diff -rq "$canonical" "$here" >&2 || true
  echo "Change the crate only in the canonical copy, then run scripts/sync-md5-sidecar.sh here." >&2
  exit 1
fi

mkdir -p "$here"
rsync -a --delete --checksum "$canonical/" "$here/"
echo "md5-sidecar: crates/md5-sidecar now matches $canonical"
