import assert from "node:assert/strict";
import test from "node:test";
import { moveAlong } from "./stacked.ts";

test("moveAlong names the branches", () => {
  assert.equal(moveAlong(["a"]), "a");
  assert.equal(moveAlong(["a", "b"]), "a and b");
  assert.equal(moveAlong(["a", "b", "c"]), "a, b and c");
  assert.equal(moveAlong(["a", "b", "c", "d", "e"]), "a, b, c and 2 more");
});
