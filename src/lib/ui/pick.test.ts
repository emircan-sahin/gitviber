import assert from "node:assert/strict";
import { test } from "node:test";
import { rangeOf, toggled } from "./pick.ts";

const id = (s: string) => s;

test("toggling adds a row or takes it out", () => {
  assert.deepEqual(toggled(["a", "b"], "c", id), ["a", "b", "c"]);
  assert.deepEqual(toggled(["a", "b", "c"], "b", id), ["a", "c"]);
  assert.deepEqual(toggled([], "a", id), ["a"]);
  // By key: a row listed again as a new object is the same row.
  const rows = [{ k: "a" }, { k: "b" }];
  assert.deepEqual(toggled(rows, { k: "a" }, (r) => r.k), [{ k: "b" }]);
});

test("a range runs either way and needs both ends listed", () => {
  const list = ["a", "b", "c", "d"];
  assert.deepEqual(rangeOf(list, "b", "d", id), ["b", "c", "d"]);
  assert.deepEqual(rangeOf(list, "d", "b", id), ["b", "c", "d"]);
  assert.deepEqual(rangeOf(list, "c", "c", id), ["c"]);
  assert.deepEqual(rangeOf(list, "x", "c", id), ["c"]);
});
