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

test("fractional pixels over many events lose no rows", () => {
  let pending = 0;
  let rows = 0;
  for (let i = 0; i < 1000; i++) {
    const step = wheelRows(pending, -0.7, 17.3);
    [rows, pending] = [rows + step.rows, step.pending];
  }
  assert.equal(rows, -Math.floor(700 / 17.3));
  assert.ok(Math.abs(pending) < 17.3);
});

test("a pane with no row height reports no rows, and what's pending stays a number", () => {
  assert.deepEqual(wheelRows(5, 40, 0), { rows: 0, pending: 5 });
  assert.deepEqual(wheelRows(5, 40, NaN), { rows: 0, pending: 5 });
  assert.deepEqual(wheelRows(5, 40, -20), { rows: 0, pending: 5 });
  assert.deepEqual(wheelRows(NaN, 40, 20), { rows: 0, pending: 0 });
});

test("one event reports a screenful at most, and drops the rows past it", () => {
  assert.deepEqual(wheelRows(0, 100_000, 20, 40), { rows: 40, pending: 0 });
  assert.deepEqual(wheelRows(0, -100_010, 20, 40), { rows: -40, pending: -10 });
  assert.deepEqual(wheelRows(0, 50, 20, 40), { rows: 2, pending: 10 });
});
