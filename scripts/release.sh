#!/usr/bin/env bash
# Releases of RustCut always come with a changelog: the notes of a GitHub release are the section of CHANGELOG.md for
# that version (Keep a Changelog format). Assets: the package, the same package as rustcut-x86_64.pkg.tar.zst (so that
# releases/latest/download/rustcut-x86_64.pkg.tar.zst is a permanent link to the newest one) and SHA256SUMS.
# Modeled on RustStart's script.
#
#   scripts/release.sh --check        the changelog documents the version in Cargo.toml (used by scripts/check-all.sh)
#   scripts/release.sh --notes-only   print the release notes of that version (for pasting into the GitHub web form)
#   scripts/release.sh                prepare the release in dist/ (package, checksums, notes) and print the commands;
#                                     creates no tag and publishes nothing
#   scripts/release.sh --publish      also create and push the tag v<version> and create the GitHub release
#                                     (needs `gh auth login`; only when the maintainer says so: "finalisiere" is theirs)
#
# Preparing and publishing happen on master after "finalisiere": clean tree, CHANGELOG section dated, package built.
set -euo pipefail
cd "$(dirname "$0")/.."

mode=prepare
for arg in "$@"; do
  case "$arg" in
    --check) mode=check ;;
    --notes-only) mode=notes ;;
    --publish) mode=publish ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

fail() { echo "release: $*" >&2; exit 1; }

version=$(sed -n '/^\[workspace.package\]/,/^\[/{s/^version *= *"\(.*\)"/\1/p}' Cargo.toml)
[ -n "$version" ] || fail "cannot read the version from Cargo.toml"
tag="v$version"
pkg_name="rustcut-${version}-1-x86_64.pkg.tar.zst"
pkg="dist/$pkg_name"
# The same package under a name without the version: GitHub's .../releases/latest/download/<name> link then always serves
# the newest release (a versioned name changes with every release).
alias_name="rustcut-x86_64.pkg.tar.zst"
alias_file="dist/$alias_name"
sums="dist/SHA256SUMS"
repo_url="https://github.com/vommie/rustcut"

# The body of a changelog section ("## [x.y.z] - date" or "## [Unreleased]") up to the next section.
section() {
  awk -v want="$1" '
    /^## \[/ { if (found) exit; if (index($0, "## [" want "]") == 1) { found = 1; next } }
    found { print }
    /^\[[^]]+\]: / { if (found) exit }
  ' CHANGELOG.md
}
has_text() { [ -n "$(printf '%s' "$1" | tr -d '[:space:]')" ]; }
# Entries are list items ("- ..."); headings alone ("### Added") document nothing. A here-string, not a pipe: grep -q
# stops at the first match, and under pipefail the writer's SIGPIPE would fail the check.
has_entries() { grep -q '^- ' <<<"$1"; }

if [ "$mode" = check ]; then
  [ -f CHANGELOG.md ] || fail "CHANGELOG.md is missing"
  if grep -q "^## \[$version\]" CHANGELOG.md; then
    has_entries "$(section "$version")" || fail "the changelog section of $version has no entries"
  elif has_entries "$(section Unreleased)"; then
    :  # the next version is being collected under [Unreleased]
  else
    fail "CHANGELOG.md documents neither $version nor [Unreleased]: add an entry for what changed"
  fi
  echo "changelog ok ($version)"
  exit 0
fi

header=$(grep -m1 "^## \[$version\]" CHANGELOG.md || true)
[ -n "$header" ] || fail "CHANGELOG.md has no section for $version (rename [Unreleased] when finalizing)"
printf '%s' "$header" | grep -Eq ' - [0-9]{4}-[0-9]{2}-[0-9]{2}$' || fail "the changelog section of $version has no date"
notes=$(section "$version")
has_text "$notes" || fail "the changelog section of $version is empty"

make_notes() {
  printf '%s\n' "$notes" | sed '/./,$!d'
  cat <<EOF

## Install (Arch Linux, Manjaro)

\`\`\`bash
gh release download ${tag} --repo vommie/rustcut --pattern '${pkg_name}' --pattern SHA256SUMS
sha256sum -c SHA256SUMS --ignore-missing
sudo pacman -U ${pkg_name}
\`\`\`

The newest version is always the latest release: \`gh release download --repo vommie/rustcut --pattern 'rustcut-x86_64.pkg.tar.zst' --pattern SHA256SUMS\`
(GitHub CLI, \`gh auth login\` once; this also works while the repository is private). Once the repository is public the permanent link
\`${repo_url}/releases/latest/download/${alias_name}\` works with a plain \`curl -LO\`.
Tag & Rate needs RustView (\`gh release download --repo vommie/rustview --pattern 'rustview-x86_64.pkg.tar.zst'\`).

Full changelog: ${repo_url}/blob/${tag}/CHANGELOG.md
EOF
}

if [ "$mode" = notes ]; then
  make_notes
  exit 0
fi

# --- prepare / publish --------------------------------------------------------------------------------------------------
git diff --quiet && git diff --cached --quiet || fail "the working tree has uncommitted changes"
branch=$(git rev-parse --abbrev-ref HEAD)
[ "$branch" = master ] || fail "release from master (now on $branch): finalize first"
if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
  [ "$(git rev-parse "$tag^{commit}")" = "$(git rev-parse HEAD)" ] || fail "the tag $tag exists but does not point at HEAD"
fi

[ -f "$pkg" ] || ./build-package.sh
[ -f "$pkg" ] || fail "no package at $pkg"
command cp -f "$pkg" "$alias_file"
(cd dist && sha256sum "$pkg_name" "$alias_name" > SHA256SUMS)
notes_file="dist/release-notes-${version}.md"
make_notes > "$notes_file"
echo "Prepared: $pkg, $alias_file, $sums, $notes_file"

if [ "$mode" = prepare ]; then
  cat <<EOF

Nothing was tagged or published. To publish (the maintainer decides):
  git tag -a $tag -m "RustCut $version"     (unless it exists)
  git push origin master $tag
  gh release create $tag $pkg $alias_file $sums --title "RustCut $version" --notes-file $notes_file --verify-tag --latest
or use the GitHub web form (Releases > Draft a new release, tag $tag, paste $notes_file, attach the three files).
EOF
  exit 0
fi

command -v gh >/dev/null || fail "gh is not installed (sudo pacman -S github-cli)"
gh auth status >/dev/null 2>&1 || fail "gh is not logged in: run \`gh auth login\` first"
git rev-parse -q --verify "refs/tags/$tag" >/dev/null || git tag -a "$tag" -m "RustCut $version"
git push origin master "$tag"
gh release create "$tag" "$pkg" "$alias_file" "$sums" --title "RustCut $version" --notes-file "$notes_file" --verify-tag --latest
echo "Published: $(gh release view "$tag" --json url -q .url)"
echo "Newest package, permanent link: ${repo_url}/releases/latest/download/${alias_name}"
