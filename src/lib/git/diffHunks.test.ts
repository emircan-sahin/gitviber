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

const same = (o: number, n = o): DiffRow => ({ k: 0, o, n });

test("a stacked diff keeps the changes and their context, and folds the rest", () => {
  // A line added after the fourth of eight.
  const rows: DiffRow[] = [same(1), same(2), same(3), same(4), { k: 1, o: 0, n: 5 }, same(5, 6), same(6, 7), same(7, 8), same(8, 9)];
  assert.deepEqual(shownRows(rows, 1), [{ gap: 3, o: 1, n: 1 }, rows[3], rows[4], rows[5], { gap: 3, o: 6, n: 7 }]);
  assert.deepEqual(shownRows([same(1)], 3), [{ gap: 1, o: 1, n: 1 }]);
  assert.deepEqual(shownRows([], 3), []);
});

test("runs shorter than the minimum stay shown, as the code view leaves them", () => {
  const rows: DiffRow[] = [same(1), same(2), { k: 2, o: 3, n: 0 }, same(4, 3), same(5, 4), same(6, 5), same(7, 6)];
  assert.deepEqual(shownRows(rows, 0, 3), [same(1), same(2), rows[2], { gap: 4, o: 4, n: 3 }]);
  assert.deepEqual(shownRows(rows, 0, 5), rows);
});

test("changes on the first and last lines, overlapping context, and no context", () => {
  const rows: DiffRow[] = [{ k: 1, o: 0, n: 1 }, same(1, 2), same(2, 3), { k: 2, o: 3, n: 0 }, same(4, 4), same(5, 5), same(6, 6), { k: 1, o: 0, n: 7 }];
  // Context 1 around each change: the run between the deletion and the last line folds to one.
  assert.deepEqual(shownRows(rows, 1), [rows[0], rows[1], rows[2], rows[3], rows[4], { gap: 1, o: 5, n: 5 }, rows[6], rows[7]]);
  // Context 2 overlaps: nothing folds.
  assert.deepEqual(shownRows(rows, 2), rows);
  assert.deepEqual(shownRows(rows, 0), [rows[0], { gap: 2, o: 1, n: 2 }, rows[3], { gap: 3, o: 4, n: 4 }, rows[7]]);
});

test("a deletion only keeps the old lines' context; revealed lines open a fold", () => {
  const rows: DiffRow[] = [same(1), same(2), same(3), { k: 2, o: 4, n: 0 }, { k: 2, o: 5, n: 0 }, same(6, 4), same(7, 5), same(8, 6)];
  assert.deepEqual(shownRows(rows, 1), [{ gap: 2, o: 1, n: 1 }, rows[2], rows[3], rows[4], rows[5], { gap: 2, o: 7, n: 5 }]);
  assert.deepEqual(shownRows(rows, 1, 1, new Set([1])), [rows[0], { gap: 1, o: 2, n: 2 }, rows[2], rows[3], rows[4], rows[5], { gap: 2, o: 7, n: 5 }]);
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
  // Ranges on a token's edges, empty ones, and past the end of the line.
  assert.deepEqual(emphasized("abcd", [["ab", "#1", 0], ["cd", "#2", 2]], [[2, 4]]), [
    ["ab", "#1", 0, false],
    ["cd", "#2", 2, true],
  ]);
  assert.deepEqual(emphasized("abcd", undefined, [[1, 1], [3, 9]]), [
    ["a", "", 0, false],
    ["bc", "", 0, false],
    ["d", "", 0, true],
  ]);
});
