import assert from "node:assert/strict";
import { test } from "node:test";
import { wheelRows } from "./wheel.ts";

test("a swipe reports a row for each row's height of pixels, and keeps the rest", () => {
  assert.deepEqual(wheelRows(0, 50, 20), { rows: 2, pending: 10 });
  assert.deepEqual(wheelRows(10, 12, 20), { rows: 1, pending: 2 });
  assert.deepEqual(wheelRows(0, 7, 20), { rows: 0, pending: 7 });
  assert.deepEqual(wheelRows(0, -45, 20), { rows: -2, pending: -5 });
  // Turning back takes up what the other way left first.
  assert.deepEqual(wheelRows(15, -20, 20), { rows: 0, pending: -5 });
  // A trackpad's small steps add up to the rows it moved, none lost.
  let pending = 0;
  let rows = 0;
  for (let i = 0; i < 100; i++) {
    const step = wheelRows(pending, 3, 17);
    [rows, pending] = [rows + step.rows, step.pending];
  }
  assert.equal(rows, Math.floor(300 / 17));
});
