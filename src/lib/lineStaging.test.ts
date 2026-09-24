import assert from "node:assert/strict";
import { test } from "node:test";
import type { DiffRow } from "./api.ts";
import { changeAt, changes, pick, whole } from "./lineStaging.ts";

const same = (o: number, n: number): DiffRow => ({ k: 0, o, n });
const add = (n: number): DiffRow => ({ k: 1, o: 0, n });
const del = (o: number): DiffRow => ({ k: 2, o, n: 0 });

// 1 kept; 2-3 replaced by 2; 4 kept; 5 added; 5 kept; 6 removed; 6 kept.
const rows = [same(1, 1), del(2), del(3), add(2), same(4, 3), add(4), add(5), same(5, 6), del(6), same(7, 7)];
const list = changes(rows);

test("runs of changed lines, with where they sit", () => {
  assert.deepEqual(list, [
    { original: [2, 4], modified: [2, 3] },
    { original: [5, 5], modified: [4, 6] },
    { original: [6, 7], modified: [7, 7] },
  ]);
  assert.deepEqual(changes([add(1), add(2)]), [{ original: [1, 1], modified: [1, 3] }]);
  assert.deepEqual(changes([del(1), same(2, 1)]), [{ original: [1, 2], modified: [1, 1] }]);
});

test("chosen new lines take what they replace, and only themselves of the rest", () => {
  assert.deepEqual(pick(list, "modified", 2, 2), { removed: [2, 3], added: [2] });
  assert.deepEqual(pick(list, "modified", 5, 5), { removed: [], added: [5] });
  // A removal counts when the line above it is chosen.
  assert.deepEqual(pick(list, "modified", 6, 6), { removed: [6], added: [] });
  assert.deepEqual(pick(list, "modified", 1, 7), { removed: [2, 3, 6], added: [2, 4, 5] });
  assert.deepEqual(pick(list, "modified", 3, 3), { removed: [], added: [] });
});

test("from the old side too", () => {
  assert.deepEqual(pick(list, "original", 3, 3), { removed: [3], added: [2] });
  assert.deepEqual(pick(list, "original", 4, 4), { removed: [], added: [4, 5] });
});

test("the change at a line", () => {
  assert.deepEqual(changeAt(list, "modified", 2), list[0]);
  assert.deepEqual(changeAt(list, "modified", 5), list[1]);
  // Removed lines sit between 6 and 7: either one finds them.
  assert.deepEqual(changeAt(list, "modified", 6), list[2]);
  assert.deepEqual(changeAt(list, "modified", 7), list[2]);
  assert.equal(changeAt(list, "modified", 1), null);
  assert.deepEqual(whole(list[0]), { removed: [2, 3], added: [2] });
});
