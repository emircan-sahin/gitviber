# gpui-pre-apple 0.3.9, patched

The crates.io release of gpui-pre-apple 0.3.9 (Zed's `gpui_apple`), byte for byte, except the
changes below and this file. Wired in through `[patch.crates-io]` in the workspace Cargo.toml
(`vendor/` is excluded from the workspace). The whole patch: `diff -ru
~/.cargo/registry/src/index.crates.io-*/gpui-pre-apple-0.3.9 vendor/gpui-pre-apple`.
gpui-pre-macos is patched with it (its PATCH.md): the window calls `trim` and `restore`, and
drives the display link of change 5.

On a gpui-pre bump, re-apply each change to the new version, or drop it once upstream does
the same. Measured 2026-10-25, M2 Pro, profiling build, 1480x920 pt window at 2x:
History + a file open 130 -> 104 MB, minimized or covered 130 -> 62 MB.

## 1. Path textures sized to the paths (`src/metal_renderer.rs`, `src/shaders.metal`)

Why: paths (History's lane curves, review diagrams, canvas arrows, a rounded corner) are
rasterized into an intermediate texture, then copied to the frame. Upstream made it as big
as the window on every resize, and its pages stayed once a path had been drawn: 21 MB at
1480x920 pt, 88 MB at 3440x1410. Any open file tab draws one small path, so that texture
was committed whenever a file was open, History or not.

The change:
- `prepare_path_intermediate_textures`, called at the start of each frame, sizes the texture
  (and the MSAA one) to the frame's largest path batch on screen (`path_texture_need`,
  `path_batch_bounds`: the batch's clipped bounds, rounded out to whole pixels, cut to the
  viewport), in 64 px steps. It is kept while it fits and is at most 4x too big, and dropped
  on a frame without paths. Command buffers retain what they use, so dropping is safe.
- `draw_paths_to_intermediate` moves the batch's vertices and clip bounds by the batch's top
  left and rasterizes at the texture's size; it returns that top left (None when the batch
  covers no pixel).
- `draw_paths_from_intermediate` passes the texture's frame on screen (origin, size) at
  `SpriteInputIndex::AtlasTextureSize`; `path_sprite_vertex` samples
  `(screen_position - frame.xy) / frame.zw` instead of `screen_position / viewport_size`.
- `update_drawable_size` and the headless `render_scene*` no longer make path textures.

Checked: History lanes in the window, pixel for pixel the same as upstream; off-screen
snapshots of History lanes (identical), an Obsidian canvas (7 of 3.3M pixels differ) and a
review diagram (1 pixel): one MSAA sample at an edge, as the coordinates round differently.

## 2. A hidden window gives its drawables back (`src/metal_renderer.rs`)

Why: a minimized, covered or other-Space window kept its three drawables (63 MB at 1480x920,
233 MB at 3440x1410) and the path textures; gpui only stopped the display link.

The change: `MetalRenderer::trim` sets the layer's drawable size to 1x1, which frees every
drawable but the one on screen, and drops the path textures; `restore` puts the size back.
`update_drawable_size` while trimmed only records the new size. A frame drawn while trimmed
(`draw`, `render_to_image`: a hidden window resized) is drawn at the real size and the window
trimmed again (`untrimmed`). Nothing draws at 1x1.

## 3. The first color atlas page is 512² (`src/metal_atlas.rs`)

Why: the first polychrome page was 1024² BGRA, 4 MB, for file icons and the logo (~0.1 MB).
The change: the first polychrome page (none live yet) is 512²; later pages and tiles bigger
than 512 size as before. The test `test_remove_deallocates_tile_space_for_reuse` uses a 400 px
tile instead of 700, so it still shares the first page. Saves 3 MB.

## 4. Apple's named faces at their CSS weight (`src/text_system.rs`)

Why: `font_id` matches with font-kit, which turns Core Text weights into CSS ones through the
table usWeightClass fills (500 is 0.2). Apple's named faces sit elsewhere: Medium 0.23 read as
530, so a 500 (CSS: try 500, then lighter) got Regular in SF, SF Mono and Helvetica Neue. Light
read 237, Heavy 780 and Black 810, so an 800 got Black.

The change: `css_properties` gives the six off-table Apple weights (UltraLight -0.8, Thin -0.6,
Light -0.4, Medium 0.23, Heavy 0.56, Black 0.62) their CSS weight before `find_best_match`; every
other face keeps font-kit's. Not rounding to 100s: of the 611 faces on this Mac, all off-table
weights are those six, except Cascadia Mono SemiLight (350) and Fira Code Retina (450), which
rounding would fold into Regular and Medium.

Checked: `apple_named_faces_match_their_css_weight` (400/500/600 apart in SF, SF Mono and
Helvetica Neue; SF's 800 and 900 apart). Off-screen, Settings → Editor's preview in SF Mono at
400/500/600 gives three faces (500 drew as 400 before); Geist Mono, JetBrains Mono and Menlo
are byte-identical to before.

## 5. Frames drawn into a `CAMetalDisplayLink`'s drawable (`src/display_link.rs`, `src/metal_renderer.rs`, `Cargo.toml`)

Why: on macOS 26, `-[CAMetalLayer nextDrawable]` called after a `CVDisplayLink` tick blocks the
main thread about a refresh (33 ms at 60 Hz) on some Macs: the compositor deadline moved close to
vsync, so a frame started at the tick slips one. Animations ran at 30 fps and input waited.
Upstream fixed it after 0.3.9 (zed #65472, 0373366); this is our own version of that design.

The change:
- `display_link::MetalFrameSource` (new): a `CAMetalDisplayLink` on the layer, on the main run
  loop in common modes, with a main-thread delegate that calls back with each update's drawable
  (as a metal-rs `MetalDrawable`). `new` is `None` where the class is missing (before macOS 14);
  drop invalidates the link. The link keeps the system's default frame rate range and latency.
- `MetalRenderer::drawable_source`: `Layer` (`nextDrawable`, as before) or `DisplayLink(slot)`.
  While attached, `draw` uses only the drawable `set_frame_drawable` put in the slot and skips a
  frame without one (logged): `nextDrawable` must not be called on a layer a link drives.
  `set_frame_drawable(None)` after the frame returns a drawable the frame did not use.
- The frame's viewport is the drawable's texture size, not the layer's: a link's drawable was
  sized when handed out, possibly before a resize. Path textures (change 1) follow the viewport,
  so they always match.
- objc2-quartz-core gains the `CAMetalDisplayLink` and `objc2-metal` features.

With change 2: the window detaches (invalidates) the link before it trims, and restores before
it attaches one, so a link never hands out 1x1 drawables; a frame drawn while trimmed is drawn
detached, through `nextDrawable`, as before.

Checked: `metal_renderer::tests::gitviber_*` (drawables used once or handed back; a frame
outside an update is skipped; the layer vends drawables again once its link is invalidated; a
trimmed window draws at its real size). The frame-rate effect needs a display (gpui-pre-macos
PATCH.md, change 4).

## Not changed

`maximumDrawableCount` stays 3. With 2 (one drawable less: 21 MB at 1480x920, 75 MB at
3440x1410), measured at 3456x2100 px and 120 Hz, frames held 120 fps but the main thread
waited in `nextDrawable` far longer: a terminal flood 2.3-4.5 s of 10 s (0.02 s in two of
three runs with 3), code scrolling 1.6-8.1 s of 12 s (0.75-1.1 s with 3), p95 up to 8.4 ms,
a whole frame. A 6K display would wait more.
