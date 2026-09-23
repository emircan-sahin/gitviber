#!/bin/sh
# `pnpm install:mac`: a release build copied into /Applications, replacing the one there.
set -e
cd "$(dirname "$0")/.."

# The .app alone: the DMG isn't needed to install locally.
pnpm tauri build --bundles app

osascript -e 'if application "GitViber" is running then quit application "GitViber"'
rm -rf /Applications/GitViber.app
cp -R src-tauri/target/release/bundle/macos/GitViber.app /Applications/
open /Applications/GitViber.app
