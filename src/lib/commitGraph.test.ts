import assert from "node:assert/strict";
import { test } from "node:test";
import { type GraphRow, graphRows } from "./commitGraph.ts";

const c = (sha: string, ...parents: string[]) => ({ sha, parents });

// Lanes as [column, branch] pairs, to keep the expected rows short.
const flat = (r: GraphRow) => ({ ...r, through: r.through.map((l) => [l.col, l.id]), into: r.into.map((l) => [l.col, l.id]), out: r.out.map((l) => [l.col, l.id]) });

test("a straight history stays in one lane", () => {
  assert.deepEqual(graphRows([c("c", "b"), c("b", "a"), c("a")]).map(flat), [
    { col: 0, id: 0, through: [], into: [], out: [[0, 0]], width: 1 },
    { col: 0, id: 0, through: [], into: [[0, 0]], out: [[0, 0]], width: 1 },
    { col: 0, id: 0, through: [], into: [[0, 0]], out: [], width: 1 },
  ]);
});

test("a merge opens a lane for the merged branch, which closes where it forked", () => {
  //  m        merge of f into b
  //  |\
  //  | f
  //  b |
  //  |/
  //  a
  assert.deepEqual(graphRows([c("m", "b", "f"), c("f", "a"), c("b", "a"), c("a")]).map(flat), [
    { col: 0, id: 0, through: [], into: [], out: [[0, 0], [1, 1]], width: 2 },
    { col: 1, id: 1, through: [[0, 0]], into: [[1, 1]], out: [[1, 1]], width: 2 },
    { col: 0, id: 0, through: [[1, 1]], into: [[0, 0]], out: [[0, 0]], width: 2 },
    { col: 0, id: 0, through: [], into: [[0, 0], [1, 1]], out: [], width: 2 },
  ]);
});

test("a branch keeps its own id in a reused column", () => {
  // f's lane closes at m1; g's opens in the same column, but it's another branch.
  const rows = graphRows([c("m2", "m1", "g"), c("g", "m1"), c("m1", "b", "f"), c("f", "b"), c("b")]);
  assert.equal(rows[1].col, rows[3].col);
  assert.notEqual(rows[1].id, rows[3].id);
});

test("a freed lane is reused rather than widening the graph", () => {
  const rows = graphRows([c("m2", "m1", "g"), c("g", "m1"), c("m1", "b", "f"), c("f", "b"), c("b")]);
  assert.equal(Math.max(...rows.map((r) => r.width)), 2);
});

test("a parent listed above its child gets no line", () => {
  // Clock skew: git's date order put the parent first.
  assert.deepEqual(graphRows([c("a"), c("b", "a")])[1], { col: 0, id: 1, through: [], into: [], out: [], width: 1 });
});

test("a row is only as wide as the lanes it draws", () => {
  // f's branch closed at b: a is back to one lane.
  const rows = graphRows([c("m", "b", "f"), c("f", "b"), c("b", "a"), c("a")]);
  assert.deepEqual(rows.map((r) => r.width), [2, 2, 2, 1]);
});

test("a parent past the loaded page keeps its lane open to the bottom", () => {
  assert.deepEqual(graphRows([c("b", "a")])[0].out, [{ col: 0, id: 0 }]);
});

test("HEAD keeps the first lane below a newer branch's tip, with nothing drawn above it", () => {
  //    f      another branch, newer than HEAD
  //  h |      HEAD
  //  |/
  //  a
  const rows = graphRows([c("f", "a"), c("h", "a"), c("a")], "h").map(flat);
  assert.deepEqual(rows[0], { col: 1, id: 1, through: [], into: [], out: [[1, 1]], width: 2 });
  assert.deepEqual(rows[1], { col: 0, id: 0, through: [[1, 1]], into: [], out: [[0, 0]], width: 2 });
  assert.deepEqual(rows[2].into, [[0, 0], [1, 1]]);
});

test("a branch built on HEAD leads into HEAD's lane", () => {
  // f's first parent is h: f's lane runs down and joins HEAD's.
  const rows = graphRows([c("f", "h"), c("h", "a"), c("a")], "h").map(flat);
  assert.deepEqual(rows[1], { col: 0, id: 0, through: [], into: [[1, 1]], out: [[0, 0]], width: 2 });
});
