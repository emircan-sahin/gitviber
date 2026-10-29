# gpui-component 0.8.0, patched

The crates.io release of gpui-component 0.8.0, byte for byte, except the changes below and this
file. Wired in through `[patch.crates-io]` in the workspace Cargo.toml (and excluded from the
workspace). The whole patch: `diff -ru ~/.cargo/registry/src/index.crates.io-*/gpui-component-0.8.0
vendor/gpui-component`. On a gpui-kit bump, re-apply it to the new version. Most of it is
GitViber's density and cursors; the two screen-reader changes are fixes upstream could take.

## Why

The app's text is 12px (SF, weight 500). Upstream draws a `.small()` control at 14px in a 24px
box and menus, tooltips and toasts at 14px, so every control and overlay was both larger and more
cramped than the labels around it. The old app's controls were 12px in 28px boxes. Changing the
size table once beats setting a size at ~360 call sites.

## The changes

- `src/sizing.rs`: `Size::control_text_size()` is the one text table, used by
  `input_text_size`, `button_text_size` and `table_cell_size`: XSmall 11.5px, Small 12px,
  Medium 12.5px, Large 14px (rems, so the interface scale still applies). `input_h`:
  24/28/32/36px (upstream 20/24/32/44). `input_px`: 8/10/10/12px (upstream 4/8/10/12).
- `src/button/button.rs`: buttons take the same heights (icon-only squares 24/28/32/36px) and
  a bit more side padding (8/10/12/14px).
- `src/menu/popup_menu.rs`: menu rows at 12px. `src/tooltip.rs`: tooltips at 11.5px.
  `src/notification.rs`: a toast's title at 12px over an 11.5px muted message; a message
  without a title stays at 12px in the foreground. `src/dialog/title.rs`: a dialog's title at
  13.5px (still semibold); `src/dialog/description.rs`: its description at 12px, muted.
- `src/theme/theme_color.rs`, `src/theme/schema.rs`, `src/switch.rs`: a `switch_border` color
  (`switch.border` in a theme file, transparent by default) for the 1px ring of an unchecked
  switch's track. A light gray track and white thumb on a white surface was about 1.5:1.

- Cursors (the old app gave anything clickable the pointing hand, and the arrow while
  disabled). Pointer when it can be pressed, default when disabled: `button/button.rs` (every
  variant; upstream only link and text), `menu/menu_item.rs` (popup, context and completion
  menu rows, submenus too), `switch.rs` (upstream: not-allowed when disabled), `checkbox.rs`,
  `radio.rs`, `tab/tab.rs`, `button/toggle.rs`, `list/list_item.rs` (a row with its own click),
  `list/list.rs` (the row a click confirms), `tree.rs`, `command/state.rs` (palette rows;
  upstream: the arrow), `notification.rs` (a toast with an `on_click`),
  `searchable_list/item.rs` (disabled: default, upstream not-allowed). Inputs keep the I-beam;
  resize handles already had theirs (gpui-base).

- Screen readers (gpui's AccessKit tree; upstream already names and states Switch, Checkbox,
  Radio, Tab, Toggle, List and Tree rows): `button/button.rs` `accessibility_name()`, an
  icon-only button is named by its tooltip's text (upstream: no name, VoiceOver said "button");
  `menu/menu_item.rs` + `menu/popup_menu.rs`, an item with a check mark is a
  `MenuItemCheckBox`, toggled on (upstream: a plain item, the check mark unsaid).

- The old app's overlay look. `menu/menu_item.rs` + `menu/popup_menu.rs`: a highlighted menu
  row is solid primary with primary-foreground ink, its icons, check mark and shortcut too;
  at rest icons are muted and check marks primary; a submenu whose own rows hold the
  highlight shows the hover color (upstream: the accent, icons in the label's color).
  Completion rows keep the accent. `styled.rs`: menus, popovers and toasts at `shadow-lg`
  black/50 (`lg_shadow`; upstream `shadow-md`/`shadow-lg` at 0.1). `dialog/dialog.rs`: the
  popover surface, `input` border (our border_strong), 4px corners, `shadow-lg` black/60, and
  22% of the viewport from the top by default (upstream: background, border, radius_lg,
  `shadow-xl` at 0.1, a tenth). `notification.rs`, `tooltip.rs`: the `input` border.
  `styled.rs` `popover_ring`: menus and popovers edged in solid `input` (border_strong);
  upstream a 10% foreground hairline. `popover.rs`: no `p_3` of its own, as the old app's
  popover had none; each content pads itself.

- Scrolling the code editor (`highlighter/input_adapter.rs`): the editor asks for the
  visible lines' styles every frame and upstream ran the tree-sitter query each time. Styles are
  now cached per 32 lines (at most 128 chunks; a chunk over 32 KB isn't cached), keyed by the
  resolver, and cleared whenever the tree changes (an edit, or a background parse landing).
  `highlighter/highlighter.rs`: the main query's capture names are made into `SharedString`s
  once instead of a `String` per capture.

Untouched on purpose: icon sizes, tabs, toggles, lists' row padding, the switch's own sizes
(28×16 at Small is the old app's).
