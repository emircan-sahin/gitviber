import assert from "node:assert/strict";
import { test } from "node:test";
import type { DiffRow } from "../api/types.ts";
import { emphasized, hunks, shownRows, usefulEmphasis } from "./diffHunks.ts";

test("a replaced line pairs its word changes", () => {
  const rows: DiffRow[] = [
    { k: 0, o: 1, n: 1 },
    { k: 2, o: 2, n: 0, e: [[6, 9]] },
    { k: 1, o: 0, n: 2, e: [[6, 9]] },
    { k: 0, o: 3, n: 3 },
  ];
  assert.deepEqual(hunks(rows, ["a", "const foo = 1;", "b"], ["a", "const bar = 1;", "b"]), [
    { original: [2, 3], modified: [2, 3], inner: [[[2, 6], [2, 9], [2, 6], [2, 9]]] },
  ]);
});

test("pure additions and deletions sit between the lines around them", () => {
  const rows: DiffRow[] = [
    { k: 0, o: 1, n: 1 },
    { k: 1, o: 0, n: 2 },
    { k: 0, o: 2, n: 3 },
    { k: 2, o: 3, n: 0 },
  ];
  assert.deepEqual(hunks(rows, ["a", "b", "c"], ["a", "x", "b"]), [
    { original: [2, 2], modified: [2, 3], inner: [] },
    { original: [3, 4], modified: [4, 4], inner: [] },
  ]);
});

test("a side with fewer word changes gets empty ones in order", () => {
  const rows: DiffRow[] = [
    { k: 2, o: 1, n: 0, e: [[0, 1], [4, 5]] },
    { k: 1, o: 0, n: 1, e: [[2, 3]] },
  ];
  assert.deepEqual(hunks(rows, ["a b c d e f g h"], ["a b c d e f g h"])[0].inner, [
    [[1, 0], [1, 1], [1, 2], [1, 3]],
    [[1, 4], [1, 5], [1, 3], [1, 3]],
  ]);
});

test("emphasis stays off indentation and off mostly rewritten lines", () => {
  assert.deepEqual(usefulEmphasis("    foo(bar);", [[0, 7]]), [[4, 7]]);
  assert.deepEqual(usefulEmphasis("foo(bar);", [[0, 8]]), []);
  assert.deepEqual(usefulEmphasis("foo(bar);", undefined), []);
});

test("a stacked diff keeps the changes and their context, and folds the rest", () => {
  // A line added after the fourth of eight.
  const same = (o: number): DiffRow => ({ k: 0, o, n: o > 4 ? o + 1 : o });
  const rows: DiffRow[] = [same(1), same(2), same(3), same(4), { k: 1, o: 0, n: 5 }, same(5), same(6), same(7), same(8)];
  assert.deepEqual(shownRows(rows, 1), [{ gap: 3, o: 1, n: 1 }, rows[3], rows[4], rows[5], { gap: 3, o: 6, n: 7 }]);
  assert.deepEqual(shownRows([same(1)], 3), [{ gap: 1, o: 1, n: 1 }]);
  assert.deepEqual(shownRows([], 3), []);
});

test("word changes cut the tokens they cross", () => {
  const tokens: [string, string, number][] = [
    ["const ", "#c", 0],
    ["foo", "#v", 0],
    [" = 1;", "#p", 0],
  ];
  assert.deepEqual(emphasized("const foo = 1;", tokens, [[4, 9]]), [
    ["cons", "#c", 0, false],
    ["t ", "#c", 0, true],
    ["foo", "#v", 0, true],
    [" = 1;", "#p", 0, false],
  ]);
  assert.deepEqual(emphasized("ab", undefined, []), [["ab", "", 0, false]]);
});
