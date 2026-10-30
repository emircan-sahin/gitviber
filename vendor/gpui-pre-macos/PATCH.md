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

## 3. Punctuation keys also count as their US names (`src/events.rs`)

Why: GPUI names a key by what the layout types, so a ⌘ or ⌃ chord bound on a US punctuation key
matched nothing where the layout types something else there: on Turkish Q the key left of 1
types `"`, and ⌃` (Toggle Terminal) arrived as ctrl-" (the old app matched both names).

The change, in `parse_keystroke`: for a ⌘ or ⌃ chord without ⌥ on a punctuation key whose US
name (`us_name`, by key code, shifted as the keymap names it) differs from the key, `key_char` is
that name. GPUI's keymap already matches `key_char` under ⌘ and ⌃ alone (`should_match`), so the
chord as typed and the US one both match; gitviber-ui's keymap picks the typed one when both are
bound (Turkish ⌘- on the US = key zooms out). Not with ⌥, which that match drops: Turkish ⌥⌘ on the
US = key would have run ⌘=. On an ISO keyboard (`KBGetLayoutType`) macOS reports the key left of
1 as kVK_ISO_Section, so that one is the US ` key there. Letters and digits stay as typed (German
⌘Y, the US Z key, isn't ⌘Z). These chords had no `key_char` before. Test:
`events::gitviber_tests` (`cargo nextest run -p gpui-pre-macos gitviber`).

## 4. Frames paced by `CAMetalDisplayLink` on macOS 14+ (`src/window.rs`, `src/metal_frames.rs`)

Why: on macOS 26 the main thread waited about 33 ms per frame in `nextDrawable` after each
`CVDisplayLink` tick (gpui-pre-apple PATCH.md, change 5). Upstream fixed it after 0.3.9 (zed
#65472, 0373366); this is our own version of that design, without its `Arc` to `Rc` rework.

The change:
- `FrameSource`, chosen once per window: `Metal(Rc<MetalFrames>)` when
  `MetalFrameSource::is_available()` (macOS 14+), else `DisplayLink`, the `CVDisplayLink`
  registry as before. Older macOS keeps the old path unchanged.
- `start_display_link` (Metal): `renderer.restore()`, then a new link on the layer whose callback
  holds the window state weakly and runs `metal_frame`: hand the drawable to the renderer, run
  GPUI's frame callback outside the lock, take back an unused drawable.
- `stop_display_link` (Metal) invalidates the link instead of pausing it, so the synchronous
  draws (activation, live resize's `displayLayer:`) and a trimmed window's draw can use
  `nextDrawable`. Live resize makes a new link per resize step.
- Idle pause (`MetalFrames`): after an update in which GPUI neither drew, presented, nor asked for
  another frame, the link pauses, so an idle window takes no drawables and does no work.
  `frame_waker` and `schedule_frame` (no-ops on this platform before) resume it; the waker lives
  in `MacWindow` outside the state lock. A link replaced during an update counts as a wake, so
  that update doesn't pause the new one.
- Window drop invalidates the link.

With change 1: hidden → `stop_display_link` (link invalidated), then `trim`; visible →
`restore`, then a new link. `start_display_link` also restores, so a link never starts on a
trimmed layer.

Checked: `metal_frames::gitviber_tests` (pause only when GPUI neither drew nor asked again; a
wake or a link replaced during the update keeps it running). Frame time, idle CPU, resize, displays and
fullscreen need the running app.
