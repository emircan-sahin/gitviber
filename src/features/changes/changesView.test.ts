import assert from "node:assert/strict";
import { test } from "node:test";
import type { FileChange } from "../../lib/api/types.ts";
import { orderFiles } from "./changesView.ts";

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
