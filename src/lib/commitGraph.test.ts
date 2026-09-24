import assert from "node:assert/strict";
import { test } from "node:test";
import { graphRows } from "./commitGraph.ts";

const c = (sha: string, ...parents: string[]) => ({ sha, parents });

test("a straight history stays in one lane", () => {
  assert.deepEqual(graphRows([c("c", "b"), c("b", "a"), c("a")]), [
    { col: 0, through: [], into: [], out: [0], width: 1 },
    { col: 0, through: [], into: [0], out: [0], width: 1 },
    { col: 0, through: [], into: [0], out: [], width: 1 },
  ]);
});

test("a merge opens a lane for the merged branch, which closes where it forked", () => {
  //  m        merge of f into b
  //  |\
  //  | f
  //  b |
  //  |/
  //  a
  assert.deepEqual(graphRows([c("m", "b", "f"), c("f", "a"), c("b", "a"), c("a")]), [
    { col: 0, through: [], into: [], out: [0, 1], width: 2 },
    { col: 1, through: [0], into: [1], out: [1], width: 2 },
    { col: 0, through: [1], into: [0], out: [0], width: 2 },
    { col: 0, through: [], into: [0, 1], out: [], width: 2 },
  ]);
});

test("a freed lane is reused rather than widening the graph", () => {
  const rows = graphRows([c("m2", "m1", "g"), c("g", "m1"), c("m1", "b", "f"), c("f", "b"), c("b")]);
  assert.equal(Math.max(...rows.map((r) => r.width)), 2);
});

test("a parent listed above its child gets no line", () => {
  // Clock skew: git's date order put the parent first.
  assert.deepEqual(graphRows([c("a"), c("b", "a")])[1], { col: 0, through: [], into: [], out: [], width: 1 });
});

test("a row is only as wide as the lanes it draws", () => {
  // f's branch closed at b: a is back to one lane.
  const rows = graphRows([c("m", "b", "f"), c("f", "b"), c("b", "a"), c("a")]);
  assert.deepEqual(rows.map((r) => r.width), [2, 2, 2, 1]);
});

test("a parent past the loaded page keeps its lane open to the bottom", () => {
  assert.deepEqual(graphRows([c("b", "a")])[0].out, [0]);
});
