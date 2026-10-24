# gpui-base 0.8.0, patched

The crates.io release of gpui-base 0.8.0, byte for byte, except `src/text/inline.rs` (the diff
below) and this file. Wired in through `[patch.crates-io]` in the workspace Cargo.toml (and
excluded from the workspace). Drop it once an upstream release sweeps the table itself.

## Why

`RETAINED_LAYOUTS` (upstream `src/text/inline.rs`, lines 277-283 and `retain_layout` at 309-318)
hands each painted paragraph's shaped `StyledText` from one frame to the next, keyed by its
inline state's address. Entries of states dropped without a final paint (a closed or replaced
document) were swept only once the table held 4096 of them, so every closed document's shaped
paragraphs stayed until thousands more had come and gone. Now the table is swept once it has
doubled since the last sweep, and never below 64 entries: dead entries never outnumber live ones
by much, at an amortized constant cost per insert.

## Measured

- crates/markdown/examples/cycles.rs (`MD=prose MallocLargeCache=0`, a different document
  opened, drawn off screen and closed 300 times): unpatched 40 → 57 MB, patched 40 → 42 MB flat.
- crates/markdown `leak_tests::closed_documents_leave_no_shaped_text_behind` (120 documents,
  bytes still allocated): unpatched 12.2 MB (fails), patched 53 KB.

## The change

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
