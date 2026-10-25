#!/bin/bash
# `scripts/updater-json.sh <dir> <tag> <version> <notes-file>`: writes <dir>/latest.json, the file
# the updater reads from the latest release, from the signed bundles in <dir>. The keys follow
# tauri-plugin-updater, which installed Tauri-built copies still run, and crates/updater reads
# the same: `{os}-{arch}-{bundle}` first, then `{os}-{arch}`.
set -euo pipefail
dir=${1:-} tag=${2:-} version=${3:-} notes_file=${4:-}
[ -n "$notes_file" ] || {
  echo "usage: scripts/updater-json.sh <dir> <tag> <version> <notes-file>" >&2
  exit 1
}
repo=${GITHUB_REPOSITORY:-emircan-sahin/gitviber}
platforms='{}'
add() { # add <file> <key>...
  local file=$1 entry
  shift
  entry=$(jq -n --arg sig "$(tr -d '\n' < "$dir/$file.sig")" \
    --arg url "https://github.com/$repo/releases/download/$tag/$(jq -rn --arg f "$file" '$f|@uri')" \
    '{signature: $sig, url: $url}')
  for key in "$@"; do
    platforms=$(jq --arg k "$key" --argjson e "$entry" '.[$k] = $e' <<< "$platforms")
  done
}
for sig in "$dir"/*.sig; do
  [ -e "$sig" ] || continue
  file=$(basename "${sig%.sig}")
  [ -e "$dir/$file" ] || continue
  case $file in *amd64* | *x86_64* | *_x64.*) arch=x86_64 ;; *arm64* | *aarch64*) arch=aarch64 ;; *) continue ;; esac
  case $file in
    # Each Mac takes its own architecture's app; the old universal one served both.
    *.app.tar.gz) add "$file" "darwin-$arch" "darwin-$arch-app" ;;
    # Like tauri-action, a Linux install that doesn't find its own bundle type gets the AppImage.
    *.AppImage) add "$file" "linux-$arch" "linux-$arch-appimage" ;;
    *.deb) add "$file" "linux-$arch-deb" ;;
    *.rpm) add "$file" "linux-$arch-rpm" ;;
  esac
done
# A release that updates only some installs is worse than one that fails here. These are the keys
# 0.1.17 published; installed copies look up the one of their own kind.
missing=$(jq -r '["darwin-aarch64", "darwin-aarch64-app", "darwin-x86_64", "darwin-x86_64-app",
  "linux-x86_64", "linux-x86_64-appimage", "linux-x86_64-deb", "linux-x86_64-rpm"] - keys | join(", ")' <<< "$platforms")
if [ -n "$missing" ]; then
  echo "updater-json: no signed bundle for $missing" >&2
  exit 1
fi
jq -n --arg version "$version" --rawfile notes "$notes_file" --arg date "$(date -u +%Y-%m-%dT%H:%M:%S.000Z)" \
  --argjson platforms "$platforms" \
  '{version: $version, notes: ($notes | sub("^\\s+"; "") | sub("\\s+$"; "")), pub_date: $date, platforms: $platforms}' > "$dir/latest.json"
jq -r '.platforms | keys[]' "$dir/latest.json"
