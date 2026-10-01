import assert from "node:assert/strict";
import { test } from "node:test";
import { rowInPlace } from "./selection.ts";

test("a row that left hands its place to the next one still there", () => {
  assert.equal(rowInPlace(["a", "b", "c"], new Set(["a", "c"]), "b"), "c");
  // Several left at once (a multi-selection): the first after the open one that stayed.
  assert.equal(rowInPlace(["a", "b", "c", "d"], new Set(["a", "d"]), "b"), "d");
  // Rows that came into the list meanwhile don't count; it's the order the list had.
  assert.equal(rowInPlace(["a", "b", "c"], new Set(["x", "a", "c"]), "b"), "c");
});

test("the last row hands its place to the one before it, and an emptied list to none", () => {
  assert.equal(rowInPlace(["a", "b", "c"], new Set(["a", "b"]), "c"), "b");
  assert.equal(rowInPlace(["a", "b", "c"], new Set(["a"]), "c"), "a");
  assert.equal(rowInPlace(["a"], new Set(), "a"), undefined);
  assert.equal(rowInPlace(["a", "b"], new Set(["b"]), "x"), undefined);
});
