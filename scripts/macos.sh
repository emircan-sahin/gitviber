#!/bin/bash
# The macOS release, step by step, so release.yml can compile with no secret in reach and sign
# after. Everything lands in target/dist/macos/.
#
#   scripts/macos.sh build [--host] [--profile P] [--features F]
#                              the app for both architectures, joined with lipo (--host: this
#                              Mac's only, for a quick local bundle); profile release by default
#   scripts/macos.sh app       GitViber.app around it: Info.plist, icons, the `gitviber` command
#   scripts/macos.sh sign      codesign with the hardened runtime as $APPLE_SIGNING_IDENTITY,
#                              ad hoc ("-") when unset
#   scripts/macos.sh notarize <file>
#                              notarytool, then staples the ticket: an App Store Connect API key
#                              ($APPLE_API_KEY, $APPLE_API_ISSUER, $APPLE_API_KEY_PATH) or an Apple
#                              ID ($APPLE_ID, $APPLE_PASSWORD, $APPLE_TEAM_ID)
#   scripts/macos.sh dmg       GitViber_<version>_universal.dmg, signed like the app
#   scripts/macos.sh updater   GitViber_<version>_universal.app.tar.gz, what the updater installs
#                              (signed after with examples/sign.rs)
#
# Locally: scripts/macos.sh build && scripts/macos.sh app && scripts/macos.sh sign gives an ad hoc
# signed app; never sign with your own identity here, the release does that.
set -euo pipefail
cd "$(dirname "$0")/.."

OUT=target/dist/macos
APP="$OUT/GitViber.app"
IDENTIFIER=app.gitviber.desktop
version() { sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml; }
die() {
  echo "macos.sh: $*" >&2
  exit 1
}

build() {
  local profile=release features=() targets=(aarch64-apple-darwin x86_64-apple-darwin)
  local host
  host=$(rustc -vV | sed -n 's/^host: //p')
  while [ $# -gt 0 ]; do
    case $1 in
      --host) targets=("$host") ;;
      --profile) profile=$2; shift ;;
      --features) features=(--features "$2"); shift ;;
      *) die "build: unknown option $1" ;;
    esac
    shift
  done
  local bins=() dir=$profile
  [ "$profile" = dev ] && dir=debug
  # The oldest macOS the bundle runs on (LSMinimumSystemVersion), as the Tauri build set it.
  export MACOSX_DEPLOYMENT_TARGET=13.0
  for target in "${targets[@]}"; do
    # This Mac's own architecture builds where a plain `cargo build` does, so one is reused.
    if [ "$target" = "$host" ]; then
      cargo build -p gitviber --locked --profile "$profile" ${features[@]+"${features[@]}"}
      bins+=("target/$dir/gitviber")
    else
      cargo build -p gitviber --locked --profile "$profile" --target "$target" ${features[@]+"${features[@]}"}
      bins+=("target/$target/$dir/gitviber")
    fi
  done
  mkdir -p "$OUT"
  lipo -create -output "$OUT/gitviber" "${bins[@]}"
  lipo -info "$OUT/gitviber"
}

app() {
  [ -f "$OUT/gitviber" ] || die "no binary; run scripts/macos.sh build first"
  rm -rf "$APP"
  mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources/bin"
  sed "s/@VERSION@/$(version)/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"
  plutil -lint -s "$APP/Contents/Info.plist"
  cp "$OUT/gitviber" "$APP/Contents/MacOS/gitviber"
  cp packaging/icons/icon.icns packaging/icons/Assets.car "$APP/Contents/Resources/"
  # The `gitviber` command: crates/core/src/cli.rs links it onto PATH, Homebrew's cask too.
  cp packaging/macos/gitviber "$APP/Contents/Resources/bin/gitviber"
  chmod 755 "$APP/Contents/MacOS/gitviber" "$APP/Contents/Resources/bin/gitviber"
  echo "$APP"
}

sign() {
  local identity=${APPLE_SIGNING_IDENTITY:--} args=(--force --options runtime --identifier "$IDENTIFIER")
  # An ad hoc signature can't be timestamped.
  if [ "$identity" = - ]; then args+=(--timestamp=none); else args+=(--timestamp); fi
  # The old app had no entitlements, and needs none: no JIT, no third-party libraries.
  codesign "${args[@]}" --sign "$identity" "$APP"
  codesign --verify --strict --verbose=2 "$APP"
}

notarize() {
  local file=$1 auth submit
  [ -e "$file" ] || die "notarize: no $file"
  if [ -n "${APPLE_API_KEY_PATH:-}" ]; then
    auth=(--key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY" --issuer "$APPLE_API_ISSUER")
  elif [ -n "${APPLE_ID:-}" ]; then
    auth=(--apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID")
  else
    die "notarize: set APPLE_API_KEY_PATH, APPLE_API_KEY and APPLE_API_ISSUER, or APPLE_ID, APPLE_PASSWORD and APPLE_TEAM_ID"
  fi
  submit=$file
  # notarytool takes a zip, a .dmg or a .pkg; the app goes as a zip, the ticket onto the app.
  if [ -d "$file" ]; then
    submit="$OUT/notarize.zip"
    ditto -c -k --keepParent "$file" "$submit"
  fi
  local result
  result=$(xcrun notarytool submit "$submit" "${auth[@]}" --wait --output-format json)
  [ "$submit" = "$file" ] || rm -f "$submit"
  echo "$result"
  if [ "$(jq -r .status <<< "$result")" != Accepted ]; then
    xcrun notarytool log "$(jq -r .id <<< "$result")" "${auth[@]}" || true
    die "Apple didn't accept $file"
  fi
  xcrun stapler staple "$file"
}

dmg() {
  [ -d "$APP" ] || die "no app; run scripts/macos.sh app first"
  local dmg="$OUT/GitViber_$(version)_universal.dmg" stage="$OUT/dmg"
  rm -rf "$stage" "$dmg"
  mkdir -p "$stage"
  # ditto keeps the signature's extended attributes and the stapled ticket.
  ditto "$APP" "$stage/GitViber.app"
  ln -s /Applications "$stage/Applications"
  hdiutil create -volname GitViber -srcfolder "$stage" -fs HFS+ -format UDZO -imagekey zlib-level=9 -ov "$dmg"
  rm -rf "$stage"
  local identity=${APPLE_SIGNING_IDENTITY:--}
  if [ "$identity" != - ]; then
    codesign --force --timestamp --sign "$identity" "$dmg"
  fi
  echo "$dmg"
}

updater() {
  [ -d "$APP" ] || die "no app; run scripts/macos.sh app first"
  local archive="$OUT/GitViber_$(version)_universal.app.tar.gz"
  # One folder, GitViber.app, at the root: Tauri's updater drops the first path component, ours
  # expects one bundle. No AppleDouble files or extended attributes, which neither unpacks.
  COPYFILE_DISABLE=1 tar --no-xattrs --no-mac-metadata -czf "$archive" -C "$OUT" GitViber.app
  echo "$archive"
}

command=${1:-}
[ $# -gt 0 ] && shift
case $command in
  build) build "$@" ;;
  app) app ;;
  sign) sign ;;
  notarize) notarize "$@" ;;
  dmg) dmg ;;
  updater) updater ;;
  *) die "usage: scripts/macos.sh build|app|sign|notarize <file>|dmg|updater" ;;
esac
