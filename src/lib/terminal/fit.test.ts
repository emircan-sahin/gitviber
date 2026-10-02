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
  // Before the shell starts, and when the held columns come due: no history to hold for.
  assert.deepEqual(planFit(box, { cols: 80, rows: 20 }, at, 0), { size: { cols: 80, rows: 20 }, colsLater: false });
});

/** What fitPane does with a plan: the size xterm ends at, the sizes it passed through on the way. */
function replay(frames: { box: { width: number; height: number }; proposed?: { cols: number; rows: number }; history: number }[], start = at) {
  let size = start;
  const seen: string[] = [];
  for (const f of frames) {
    const plan = planFit(f.box, f.proposed, size, f.history);
    if (!plan) continue;
    if (plan.size.cols !== size.cols || plan.size.rows !== size.rows) seen.push(`${plan.size.cols}x${plan.size.rows}`);
    size = plan.size;
  }
  return { size, seen };
}

test("⌘J spam with the panel's first frame in between never takes the pane through one row", () => {
  // Each show: the pane all padding (0 high, FitAddon's 1 row), then laid out at its size again.
  const show = [
    { box: { width: 1188, height: 0 }, proposed: { cols: 146, rows: 1 }, history: 400 },
    { box: { width: 1188, height: 267 }, proposed: { cols: 100, rows: 30 }, history: 400 },
  ];
  const { size, seen } = replay(Array.from({ length: 10 }, () => show).flat());
  assert.deepEqual(size, at);
  assert.deepEqual(seen, [], "no resize at all, so the program is never told");
});

test("a pane with real but tiny room still gets its one row", () => {
  // A split pane dragged to its minimum, or a short window: the guard is for no room, not little.
  assert.deepEqual(planFit({ width: 563, height: 14 }, { cols: 68, rows: 1 }, at, 10), { size: { cols: 68, rows: 1 }, colsLater: false });
  assert.equal(planFit({ width: 0, height: 267 }, { cols: 2, rows: 15 }, at, 10), null, "a split pane squeezed to no width");
});

test("columns wait only past REWRAP_LINES, however long the history", () => {
  const narrower = { cols: 80, rows: 30 };
  assert.equal(planFit(box, narrower, at, REWRAP_LINES)!.colsLater, false, "at the threshold");
  assert.equal(planFit(box, narrower, at, REWRAP_LINES + 1)!.colsLater, true);
  // A full 10k-line scrollback (Claude Code's long sessions).
  assert.deepEqual(planFit(box, narrower, at, 10_000), { size: at, colsLater: true }, "rows unchanged: nothing now, the columns later");
});

test("a font zoom over a long history: rows at once, then the held columns with the rows kept", () => {
  const zoomed = { cols: 89, rows: 19 };
  const first = planFit(box, zoomed, at, 5000)!;
  assert.deepEqual(first, { size: { cols: 100, rows: 19 }, colsLater: true });
  // 100 ms later fitPane runs with `now`: no history to hold for.
  assert.deepEqual(planFit(box, zoomed, first.size, 0), { size: zoomed, colsLater: false });
  // Hidden by ⌘J before the 100 ms: the held columns wait for the pane to be back.
  assert.equal(planFit({ width: 0, height: 0 }, { cols: 2, rows: 1 }, first.size, 0), null);
});

test("a restored tab with a long history opens at its full size before its shell starts", () => {
  // Not started: fitPane passes no history to hold for, so the shell is spawned at the right columns.
  assert.deepEqual(planFit(box, { cols: 146, rows: 17 }, { cols: 80, rows: 24 }, 0), { size: { cols: 146, rows: 17 }, colsLater: false });
});
