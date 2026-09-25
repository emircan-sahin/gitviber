import assert from "node:assert/strict";
import { test } from "node:test";
import type { DiffRow } from "../api/types.ts";
import { hunks, usefulEmphasis } from "./diffHunks.ts";

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
