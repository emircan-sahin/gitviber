# gpui-pre-macos 0.3.9, patched

The crates.io release of gpui-pre-macos 0.3.9 (Zed's `gpui_macos`), byte for byte, except the
changes below and this file. Wired in through `[patch.crates-io]` in the workspace Cargo.toml
(`vendor/` is excluded from the workspace). The whole patch: `diff -ru
~/.cargo/registry/src/index.crates.io-*/gpui-pre-macos-0.3.9 vendor/gpui-pre-macos`.
It goes with the patched gpui-pre-apple (its PATCH.md has the measurements); on a gpui-pre
bump, re-apply both or drop them together.

## 1. A hidden window gives its GPU memory back (`src/window.rs`)

Why: when AppKit reports a window not visible (minimized, fully covered, on another Space,
display asleep), gpui only stopped the display link and kept every drawable and path texture.

The change, in `window_did_change_occlusion_state`: on hidden, after the display link stops,
`renderer.trim()` (1x1 drawable size, path textures dropped); on visible, `renderer.restore()`
before the display link starts, in the same handler, so no frame is drawn at 1x1. The front
drawable stays on screen until the next frame. 1480x920 pt window: 130 -> 62 MB minimized or
covered, back to the full size on the first frame after it shows.

## 2. Deprecation warnings allowed (`Cargo.toml`)

Why: cargo caps a registry crate's lints but shows a path crate's, and its cocoa calls printed
about 1200 deprecation warnings into every build. The change: `[lints.rust] deprecated =
"allow"`.
