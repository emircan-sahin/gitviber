import assert from "node:assert/strict";
import { test } from "node:test";
import { type Leaving, rowInPlace, settleLeaving } from "./leaving.ts";

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

test("rows still on their way out are passed over, as S moves on before git is done", () => {
  const out = new Set(["b", "c"]);
  const still = { has: (k: string) => !out.has(k) };
  assert.equal(rowInPlace(["a", "b", "c", "d"], still, "c"), "d");
  assert.equal(rowInPlace(["a", "b", "c"], still, "c"), "a");
});

const record = (keys: string[], follow = false, seen: number | null = null): Leaving<number> => ({ keys: new Set(keys), follow, seen });

test("a record ends once its rows have left, and moves the open row on only when it had nowhere to go", () => {
  const moved = record(["a"]);
  const stuck = record(["z"], true);
  const records = new Set([moved, stuck]);
  assert.equal(settleLeaving(records, new Set(["a", "z"]), 1, "z"), false, "nothing left yet");
  assert.equal(records.size, 2);
  assert.equal(settleLeaving(records, new Set(["b"]), 2, "z"), true);
  assert.equal(records.size, 0);
  // The user opened another row meanwhile: theirs stays.
  assert.equal(settleLeaving(new Set([record(["z"], true)]), new Set(), 3, "q"), false);
});

test("a record that settled with its rows still there ends with the next status, not before", () => {
  const r = record(["a"], true);
  const records = new Set([r]);
  // Still running: a status read meanwhile doesn't end it.
  settleLeaving(records, new Set(["a"]), 1, "a");
  assert.ok(records.has(r));
  r.seen = 1;
  settleLeaving(records, new Set(["a"]), 1, "a");
  assert.ok(records.has(r), "the status it settled on");
  assert.equal(settleLeaving(records, new Set(["a"]), 2, "a"), false);
  assert.equal(records.size, 0);
});
