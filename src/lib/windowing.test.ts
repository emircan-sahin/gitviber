import assert from "node:assert/strict";
import { test } from "node:test";
import { renderedRows, visibleRows } from "./windowing.ts";

test("the rows on screen, with overscan, clamped to the list", () => {
  // The list starts 60px below the viewport's top: its first rows are on screen.
  assert.deepEqual(visibleRows(-60, 520, 26, 3001, 12), [0, 30]);
  // Scrolled 2600px into it: rows 100 to 120 are on screen.
  assert.deepEqual(visibleRows(2600, 520, 26, 3001, 12), [88, 132]);
  assert.deepEqual(visibleRows(78000, 520, 26, 3001, 12), [2988, 3001]);
  // Scrolled past it entirely.
  assert.deepEqual(visibleRows(-5000, 520, 26, 3001, 12), [0, 0]);
});

test("kept rows render wherever they are, once, in order", () => {
  assert.deepEqual(renderedRows([3, 6], [184, 0, -1, 4, 9999], 201), [0, 3, 4, 5, 184]);
});
