import assert from "node:assert/strict";
import { test } from "node:test";
import { planFit, REWRAP_LINES } from "./fit.ts";

const box = { width: 800, height: 400 };
const at = { cols: 100, rows: 30 };

test("a pane with no room keeps its size rather than shrinking to one row", () => {
  // ⌘J: the panel mounts at the layout library's first size, its pane all padding and no content.
  assert.equal(planFit({ width: 800, height: 0 }, { cols: 100, rows: 1 }, at, 0), null);
  assert.equal(planFit({ width: 0, height: 0 }, { cols: 2, rows: 1 }, at, 0), null, "detached");
  assert.equal(planFit(box, undefined, at, 0), null, "no cell size measured yet");
  assert.equal(planFit(box, { cols: NaN, rows: NaN }, at, 0), null);
});

test("rows follow at once, columns wait past a long history", () => {
  assert.deepEqual(planFit(box, { cols: 80, rows: 20 }, at, 10), { size: { cols: 80, rows: 20 }, colsLater: false });
  assert.deepEqual(planFit(box, { cols: 80, rows: 20 }, at, REWRAP_LINES + 1), { size: { cols: 100, rows: 20 }, colsLater: true });
  assert.deepEqual(planFit(box, { cols: 100, rows: 20 }, at, REWRAP_LINES + 1), { size: { cols: 100, rows: 20 }, colsLater: false }, "only the rows changed");
  // Before the shell starts, and when the held columns come due.
  assert.deepEqual(planFit(box, { cols: 80, rows: 20 }, at, null), { size: { cols: 80, rows: 20 }, colsLater: false });
});
