#!/bin/bash
# The app's version is the workspace's ([workspace.package] in Cargo.toml); Cargo.lock carries a
# copy for every crate of ours, which has to match.
#   scripts/version.sh check [tag]   prints it; fails on a mismatch, or when the tag isn't vX.Y.Z
#                                    of it or a test tag like vX.Y.Z-rc.1
#   scripts/version.sh set X.Y.Z     writes it to Cargo.toml and Cargo.lock
set -euo pipefail
cd "$(dirname "$0")/.."
SEMVER='^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$'
die() {
  echo "version: $*" >&2
  exit 1
}
version() { sed -n '/^\[workspace\.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml; }

check() {
  local version tag=${1:-} off
  version=$(version)
  [ -n "$version" ] || die "no version in Cargo.toml's [workspace.package]"
  [[ $version =~ $SEMVER ]] || die "Cargo.toml's version $version isn't semver (X.Y.Z)"
  # Our crates: gitviber and gitviber-*, from no registry (a vendored patch is a path too).
  off=$(awk -v want="$version" '
    function done() { if (name ~ /^gitviber(-|$)/ && !source && ver != want) print name " " ver }
    /^\[\[package\]\]/ { done(); name = ""; ver = ""; source = 0 }
    /^name = / { name = $3; gsub(/"/, "", name) }
    /^version = / { ver = $3; gsub(/"/, "", ver) }
    /^source = / { source = 1 }
    END { done() }' Cargo.lock)
  [ -z "$off" ] || die "Cargo.lock has $(echo "$off" | paste -sd, -), Cargo.toml has $version. Run scripts/version.sh set $version"
  if [ -n "$tag" ] && [ "$tag" != "v$version" ] && [[ $tag != "v$version-"* ]]; then
    die "tag $tag doesn't match the version, v$version"
  fi
  echo "$version"
}

set_version() {
  local version=${1:-}
  [[ $version =~ $SEMVER ]] || die "usage: scripts/version.sh set X.Y.Z"
  sed -i.bak "/^\[workspace\.package\]/,/^\[/s/^version = \".*\"/version = \"$version\"/" Cargo.toml
  rm -f Cargo.toml.bak
  # Only our own crates' entries change; every dependency stays at its locked version.
  cargo update --workspace --offline --quiet
  check
}

case ${1:-} in
  check) check "${2:-}" ;;
  set) set_version "${2:-}" ;;
  *) die "usage: scripts/version.sh check [tag] | set X.Y.Z" ;;
esac
