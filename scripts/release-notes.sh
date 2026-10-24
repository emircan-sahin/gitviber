#!/bin/bash
# `scripts/release-notes.sh vX.Y.Z`: prints X.Y.Z's CHANGELOG.md section, which becomes the GitHub
# release's notes and the updater's. Fails when the section is missing, empty or undated, so a
# tag can't ship with notes that weren't written. A test tag (vX.Y.Z-rc.1) uses X.Y.Z's section,
# dated or not.
set -euo pipefail
cd "$(dirname "$0")/.."
die() {
  echo "release-notes: $*" >&2
  exit 1
}
tag=${1:-}
[[ $tag =~ ^v([0-9]+\.[0-9]+\.[0-9]+)(-.+)?$ ]] || die "usage: scripts/release-notes.sh vX.Y.Z"
version=${BASH_REMATCH[1]}
prerelease=${BASH_REMATCH[2]}
heading=$(grep -m1 -F "## [$version]" CHANGELOG.md || true)
[[ $heading == "## [$version]"* ]] || die "CHANGELOG.md has no \"## [$version]\" section"
if [ -z "$prerelease" ] && ! [[ $heading =~ \ -\ [0-9]{4}-[0-9]{2}-[0-9]{2}$ ]]; then
  die "date the heading: \"## [$version] - YYYY-MM-DD\", not \"$heading\""
fi
# The section runs to the next heading of its level or the link references at the bottom.
notes=$(awk -v start="$heading" '
  found && (/^## / || /^\[[^]]+\]: /) { exit }
  found { print }
  $0 == start { found = 1 }' CHANGELOG.md)
# Trimmed of blank lines at both ends, as the release shows it.
notes=$(printf '%s\n' "$notes" | sed -e '/./,$!d' | sed -e ':a' -e '/^\n*$/{$d;N;ba' -e '}')
[ -n "$notes" ] || die "the $version section of CHANGELOG.md is empty"
printf '%s\n' "$notes"
