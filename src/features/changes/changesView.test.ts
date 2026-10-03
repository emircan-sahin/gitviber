import assert from "node:assert/strict";
import { test } from "node:test";
import type { FileChange } from "../../lib/api/types.ts";
import { itemsOf, orderFiles } from "./changesView.ts";

const file = (path: string) => ({ path, status: "M" }) as FileChange;
const paths = (list: FileChange[]) => list.map((f) => f.path);
const files = [file("b.ts"), file("src/a.ts"), file("c.ts"), file("gone.ts")];
const times = new Map([
  ["b.ts", 10],
  ["src/a.ts", 30],
  ["c.ts", 20],
]);

test("by name, the list is git's own order", () => {
  assert.deepEqual(paths(orderFiles(files, { tree: false, sort: "name" }, null)), ["b.ts", "src/a.ts", "c.ts", "gone.ts"]);
});

test("recently modified puts the newest first and files with no time (deleted) last", () => {
  assert.deepEqual(paths(orderFiles(files, { tree: false, sort: "recent" }, times)), ["src/a.ts", "c.ts", "b.ts", "gone.ts"]);
  // Until the times are read, as listed.
  assert.deepEqual(paths(orderFiles(files, { tree: false, sort: "recent" }, null)), ["b.ts", "src/a.ts", "c.ts", "gone.ts"]);
});

test("the tree walks folders first, then files, so J/K follow what's on screen", () => {
  assert.deepEqual(paths(orderFiles(files, { tree: true, sort: "name" }, null)), ["src/a.ts", "b.ts", "c.ts", "gone.ts"]);
  assert.deepEqual(paths(orderFiles(files, { tree: true, sort: "recent" }, times)), ["src/a.ts", "c.ts", "b.ts", "gone.ts"]);
});

test("the tree's rows carry a key per list and folder, and a closed folder hides its files", () => {
  const list = [file("src/lib/a.ts"), file("src/b.ts"), file("c.ts")];
  const rows = itemsOf("unstaged", list, { tree: true, sort: "name" }, null, new Set(["unstaged:src/lib"]));
  assert.deepEqual(
    rows.map((r) => (r.type === "folder" ? r.key : r.file.path)),
    ["folder:unstaged:src", "folder:unstaged:src/lib", "src/b.ts", "c.ts"],
  );
  // Another list's closed folder doesn't close this one.
  const other = itemsOf("staged", list, { tree: true, sort: "name" }, null, new Set(["unstaged:src"]));
  assert.equal(other.length, 5);
});
