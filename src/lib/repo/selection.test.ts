import assert from "node:assert/strict";
import { test } from "node:test";
import { filesSelection, onDisk, selectionKey, selectionPath } from "./selection.ts";

test("two working-tree files compared make one tab per pair, the right one's path its own", () => {
  const ab = filesSelection("src/a.ts", "src/b.ts");
  assert.equal(selectionPath(ab), "src/b.ts");
  assert.equal(selectionKey(ab), selectionKey(filesSelection("src/a.ts", "src/b.ts")));
  // Another left side, or the sides swapped, is another comparison.
  assert.notEqual(selectionKey(ab), selectionKey(filesSelection("src/c.ts", "src/b.ts")));
  assert.notEqual(selectionKey(ab), selectionKey(filesSelection("src/b.ts", "src/a.ts")));
  // Not the file's own tab.
  assert.notEqual(selectionKey(ab), selectionKey({ kind: "file", path: "src/b.ts" }));
  assert.equal(onDisk(ab), false);
});

test("names with spaces, unicode and a leading dash keep their sides apart", () => {
  const odd = filesSelection("-dash $x.txt", "ünï code.txt");
  assert.equal(selectionPath(odd), "ünï code.txt");
  assert.ok(selectionKey(odd).includes("-dash $x.txt"));
});
