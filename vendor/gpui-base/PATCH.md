# gpui-base 0.8.0, patched

The crates.io release of gpui-base 0.8.0, byte for byte, except three changes and this file:
1. `src/text/inline.rs`: closed markdown documents' shaped text is swept (below). Drop it once
   an upstream release sweeps the table itself.
2. Line blocks in the code editor (the second half of this file). Drop it once upstream has
   blocks between lines.
3. Two read-only accessors the code view's editing commands need (the end of this file). Drop
   them once upstream exposes the selections and the syntax context.

Wired in through `[patch.crates-io]` in the workspace Cargo.toml (and excluded from the
workspace). The whole patch: `diff -ru ~/.cargo/registry/src/index.crates.io-*/gpui-base-0.8.0
vendor/gpui-base`.

## 1. Retained layouts: why

`RETAINED_LAYOUTS` (upstream `src/text/inline.rs`, lines 277-283 and `retain_layout` at 309-318)
hands each painted paragraph's shaped `StyledText` from one frame to the next, keyed by its
inline state's address. Entries of states dropped without a final paint (a closed or replaced
document) were swept only once the table held 4096 of them, so every closed document's shaped
paragraphs stayed until thousands more had come and gone. Now the table is swept once it has
doubled since the last sweep, and never below 64 entries: dead entries never outnumber live ones
by much, at an amortized constant cost per insert.

## 1. Measured

- crates/markdown/examples/cycles.rs (`MD=prose MallocLargeCache=0`, a different document
  opened, drawn off screen and closed 300 times): unpatched 40 → 57 MB, patched 40 → 42 MB flat.
- crates/markdown `leak_tests::closed_documents_leave_no_shaped_text_behind` (120 documents,
  bytes still allocated): unpatched 12.2 MB (fails), patched 53 KB.

## 1. The change

```diff
--- a/src/text/inline.rs
+++ b/src/text/inline.rs
@@ -279,9 +279,17 @@
 }
 
 /// Dead entries (states that were dropped without a final paint, e.g. a
-/// replaced document) are swept once the table grows past this many.
-const RETAINED_SWEEP_AT: usize = 4096;
+/// replaced or closed document) are swept once the table has doubled since the
+/// last sweep, and no sooner than this many: dead ones never outnumber the live
+/// ones by much, at an amortized constant cost per insert. A fixed 4096 let a
+/// closed document's shaped paragraphs stay until thousands more had come and
+/// gone (GitViber: +65 KB a document opened and closed).
+const RETAINED_SWEEP_MIN: usize = 64;
 
+thread_local! {
+    static NEXT_SWEEP: std::cell::Cell<usize> = const { std::cell::Cell::new(RETAINED_SWEEP_MIN) };
+}
+
 fn state_key(state: &Arc<Mutex<InlineState>>) -> usize {
     Arc::as_ptr(state) as usize
 }
@@ -309,8 +317,9 @@
 fn retain_layout(state: &Arc<Mutex<InlineState>>, retained: RetainedLayout) {
     RETAINED_LAYOUTS.with(|layouts| {
         let mut layouts = layouts.borrow_mut();
-        if layouts.len() >= RETAINED_SWEEP_AT {
+        if layouts.len() >= NEXT_SWEEP.get() {
             layouts.retain(|_, retained| retained.state.strong_count() > 0);
+            NEXT_SWEEP.set((layouts.len() * 2).max(RETAINED_SWEEP_MIN));
         }
         layouts.insert(state_key(state), retained);
     });
```

## 2. Line blocks: why

Review notes on a file's own view sit between its lines (the old app used Monaco's view zones).
The editor had no way to leave room between lines, and everything in it (visible rows, scroll
height, scrollbar, caret moves, hit tests, line numbers, folds) assumes rows of one height.

## 2. The change

A block takes whole rows: `gap` empty wrap rows under a buffer line, so every row computation
keeps working and only the places that walk a line's rows learn about the gap.

- `EditorState::set_line_blocks(Vec<LineBlock>)` (`src/input/editor/mod.rs`): `LineBlock { id,
  line, rows, render }`, exported as `gpui_base::input::LineBlock`. A block keeps the rows it
  measured while its id stays. `DisplayMap::gaps()` / `gap_rows(line)` read them back.
- `text_wrapper.rs`: `LineItem.gap`, counted in the summary's wrap rows and a line's row range;
  `TextWrapper::set_gaps`; `_update` moves gaps with their lines (past an edit by its line delta,
  inside it to its end; a whole-text replacement or re-wrap keeps them). `LineLayout.gap_rows`
  is in its `size()`.
- `display_map.rs` / `wrap_map.rs`: `set_gaps`, `gaps`, `gap_rows`, `is_gap_display_row`.
- `element.rs`: lines get their gap rows; line numbers, the active line, fold icons and range
  frames skip the gap; `prepaint_blocks` lays each visible block out at the text's width (its
  height measured, rounded up to rows), centred in its rows, fixed sideways, clipped to the
  input, `block_mouse_except_scroll` so clicks stay in it and the wheel still scrolls. A block
  measuring other rows is resized after paint (the next frame has it).
- `movement.rs`: ↑/↓, page and add-cursor moves step over gap rows. `indent.rs`: guides skip it.
- `kind.rs`: `EditorExtras.blocks`, `InputExtras::line_blocks`. `input/mod.rs`: the export.

Tested from crates/files (`tests::blocks_between_lines_take_rows_the_caret_steps_over`: rows,
placement, caret, clicks, edits, reload, leaks) and seen in a snapshot of the files demo.

## 3. Selections and syntax context: why and the change

The file view's ⌘/, ⇧⌘K, ⌥↑/⌥↓, ⌘D and ⌘L act on every selection, and its bracket matching
skips brackets in strings and comments; the editor kept both to itself.

- `src/input/base/state.rs`: `InputBaseState::selected_ranges()`, every selection as a byte
  range, the active one first (after `selected_range`).
- `src/input/editor/mod.rs`: `EditorState::syntax_context_at` is `pub` (was `pub(crate)`).

Tested from crates/files (`edit::tests`, `tests::oldbugs::the_old_editors_keys_edit_the_code`).
