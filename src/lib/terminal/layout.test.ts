import assert from "node:assert/strict";
import { test } from "node:test";
import { type Direction, type Layout, leaves, mapPanes, neighbor, type Rect, removePane, resize, row, savedLayout, splitPane, type Split } from "./layout.ts";

/** Where each pane is drawn in a 1200×800 tab with 1px dividers, as the panel lays them out. */
function drawn(l: Layout, box: Rect = { x: 0, y: 0, width: 1200, height: 800 }, out = new Map<number, Rect>()) {
  if (typeof l === "number") return out.set(l, box);
  const across = l.dir === "row";
  const free = (across ? box.width : box.height) - (l.children.length - 1);
  let at = across ? box.x : box.y;
  l.children.forEach((c, i) => {
    const size = (free * l.sizes[i]) / 100;
    drawn(c, across ? { ...box, x: at, width: size } : { ...box, y: at, height: size }, out);
    at += size + 1;
  });
  return out;
}

/** Sizes to a millionth: a save's rescale may differ in the last bits. */
const round = (l: unknown) => JSON.parse(JSON.stringify(l, (_, v) => (typeof v === "number" ? Math.round(v * 1e6) / 1e6 : v)));

test("splits go right or down, and a split already going that way takes one more pane", () => {
  let l: Layout = 1;
  l = splitPane(l, 1, 2, "row");
  assert.deepEqual(l, { dir: "row", children: [1, 2], sizes: [50, 50] });
  l = splitPane(l, 1, 3, "row");
  assert.deepEqual(l, { dir: "row", children: [1, 3, 2], sizes: [25, 25, 50] });
  l = splitPane(l, 2, 4, "col");
  assert.deepEqual(l, { dir: "row", children: [1, 3, { dir: "col", children: [2, 4], sizes: [50, 50] }], sizes: [25, 25, 50] });
  assert.deepEqual(leaves(l), [1, 3, 2, 4]);
});

test("closing a pane gives its space to the rest in proportion, and a split left with one pane gives way to it", () => {
  const grid: Layout = { dir: "row", children: [{ dir: "col", children: [1, 2], sizes: [50, 50] }, { dir: "col", children: [3, 4], sizes: [50, 50] }], sizes: [50, 50] };
  const l = removePane(grid, 2)!;
  assert.deepEqual(l, { dir: "row", children: [1, { dir: "col", children: [3, 4], sizes: [50, 50] }], sizes: [50, 50] });
  assert.deepEqual(removePane({ dir: "row", children: [1, 2, 3], sizes: [20, 30, 50] }, 1), { dir: "row", children: [2, 3], sizes: [37.5, 62.5] });
  assert.equal(removePane(removePane(l, 1)!, 3), 4);
  assert.equal(removePane(1, 1), null);
  // A row inside a row (the column around it collapsed) joins its parent, each pane keeping its share.
  const nested: Layout = { dir: "row", children: [{ dir: "col", children: [{ dir: "row", children: [2, 3], sizes: [25, 75] }, 4], sizes: [50, 50] }, 1], sizes: [60, 40] };
  assert.deepEqual(removePane(nested, 4), { dir: "row", children: [2, 3, 1], sizes: [15, 45, 40] });
});

test("two dividers dragged at once both keep their sizes, and only the path to them is copied", () => {
  const inner: Split = { dir: "col", children: [2, 3], sizes: [50, 50] };
  const left: Split = { dir: "col", children: [4, 5], sizes: [50, 50] };
  const l: Layout = { dir: "row", children: [left, 1, inner], sizes: [30, 30, 40] };
  const both = resize(resize(l, [], [20, 40, 40]), [2], [30, 70]) as Split;
  assert.deepEqual(both, { dir: "row", children: [left, 1, { dir: "col", children: [2, 3], sizes: [30, 70] }], sizes: [20, 40, 40] });
  assert.equal(both.children[0], left, "the other split untouched");
  assert.deepEqual(resize(l, [1], [50, 50]), l, "a path to a pane");
  assert.deepEqual(resize(l, [2], [10, 20, 70]), l, "sizes for another split");
});

test("any run of splits and closes keeps the tree sound", () => {
  // mulberry32: the same run every time.
  let seed = 7;
  const random = () => {
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  const sound = (l: Layout, parent?: Split["dir"]) => {
    if (typeof l === "number") return;
    assert.ok(l.children.length >= 2 && l.sizes.length === l.children.length);
    assert.ok(Math.abs(l.sizes.reduce((a, b) => a + b, 0) - 100) < 1e-6);
    assert.ok(l.sizes.every((s) => s > 0));
    assert.notEqual(l.dir, parent, "a split inside one going the same way");
    l.children.forEach((c) => sound(c, l.dir));
  };
  let l: Layout = 1;
  let live = [1];
  let next = 2;
  for (let step = 0; step < 500; step++) {
    const pick = live[Math.floor(random() * live.length)];
    if (live.length > 1 && random() < 0.4) {
      l = removePane(l, pick)!;
      live = live.filter((id) => id !== pick);
    } else {
      l = splitPane(l, pick, next, random() < 0.5 ? "row" : "col");
      live = [...live, next++];
    }
    sound(l);
    assert.deepEqual([...leaves(l)].sort((a, b) => a - b), [...live].sort((a, b) => a - b));
    const order = leaves(l);
    assert.deepEqual(round(mapPanes(savedLayout(JSON.parse(JSON.stringify(mapPanes(l, (id) => order.indexOf(id)))), order.length), (i) => order[i])), round(l));
  }
});

test("a save restores the same tree under the new panes' ids", () => {
  const l: Layout = { dir: "col", children: [{ dir: "row", children: [7, { dir: "col", children: [9, 8], sizes: [40, 60] }], sizes: [30, 70] }, 12], sizes: [65, 35] };
  const order = leaves(l);
  const saved = JSON.parse(JSON.stringify(mapPanes(l, (id) => order.indexOf(id))));
  const ids = [101, 102, 103, 104];
  assert.deepEqual(round(mapPanes(savedLayout(saved, order.length), (i) => ids[i])), round(mapPanes(l, (id) => ids[order.indexOf(id)])));
});

test("a saved layout that doesn't hold up is a row; sizes are rescaled to 100", () => {
  const flat = row([0, 1]);
  const split = (children: unknown, sizes: unknown, dir = "row") => ({ dir, children, sizes });
  assert.deepEqual(savedLayout(undefined, 2), flat, "saved before splits went both ways");
  assert.equal(savedLayout(undefined, 1), 0);
  assert.deepEqual(savedLayout(split([0, 1], [1, 3], "col"), 2), { dir: "col", children: [0, 1], sizes: [25, 75] });
  for (const [bad, why] of [
    [split([0, 1], [50, -50]), "a negative size"],
    [split([0, 1], [50, 0]), "a zero size"],
    [split([0, 1], [50, Infinity]), "Infinity"],
    [split([0, 1], [50, "50"]), "a string size"],
    [split([0, 1], [50]), "a size missing"],
    [split([0], [100]), "a split of one"],
    [split([0, null], [50, 50]), "a null child"],
    [split([0, "1"], [50, 50]), "a string child"],
    [split([0, { children: [1] }], [50, 50]), "a child without a direction"],
    [split([1, 0], [50, 50]), "out of reading order"],
    [split([0, 0], [50, 50]), "a pane twice"],
    [split([0, 1], [50, 50], "diagonal"), "no such direction"],
  ] as const)
    assert.deepEqual(savedLayout(bad, 2), flat, why);
  assert.deepEqual(savedLayout(split([0, 1], [50, 50]), 3), row([0, 1, 2]), "a pane missing from it");
  let deep: unknown = 1;
  for (let i = 0; i < 40; i++) deep = split([0, deep], [50, 50]);
  assert.deepEqual(savedLayout(deep, 2), flat, "nested past any screen");
});

test("focus moves to the pane drawn on that side", () => {
  const moves: Record<number, Partial<Record<Direction, number>>> = { 1: { right: 2, down: 3 }, 2: { left: 1, down: 4 }, 3: { right: 4, up: 1 }, 4: { left: 3, up: 2 } };
  const columns: Layout = { dir: "row", children: [{ dir: "col", children: [1, 3], sizes: [50, 50] }, { dir: "col", children: [2, 4], sizes: [50, 50] }], sizes: [50, 50] };
  const rows: Layout = { dir: "col", children: [{ dir: "row", children: [1, 2], sizes: [50, 50] }, { dir: "row", children: [3, 4], sizes: [50, 50] }], sizes: [50, 50] };
  for (const grid of [columns, rows])
    for (const from of [1, 2, 3, 4]) for (const dir of ["left", "right", "up", "down"] as const) assert.equal(neighbor(drawn(grid), from, dir), moves[from][dir], `${from} ${dir}`);
  for (const dir of ["left", "right", "up", "down"] as const) assert.equal(neighbor(drawn(1), 1, dir), undefined);
  assert.equal(neighbor(drawn(columns), 9, "left"), undefined, "a pane that's gone");
  // Three equal columns over one pane across: up from it is the first column.
  const over: Layout = { dir: "col", children: [{ dir: "row", children: [1, 2, 3], sizes: [100 / 3, 100 / 3, 100 / 3] }, 4], sizes: [50, 50] };
  assert.equal(neighbor(drawn(over), 4, "up"), 1);
  assert.equal(neighbor(drawn(over), 2, "down"), 4);
  assert.equal(neighbor(drawn(over), 3, "left"), 2);
  assert.equal(neighbor(drawn(over), 1, "left"), undefined);
});

test("with panes a few pixels wide, focus never goes the other way", () => {
  const box = (x: number, width: number) => ({ x, y: 0, width, height: 400 });
  // Reading order apart from where they're drawn: the nearest still wins.
  const thin = new Map([[3, box(6, 2)], [1, box(0, 2)], [2, box(3, 2)]]);
  assert.equal(neighbor(thin, 2, "left"), 1);
  assert.equal(neighbor(thin, 2, "right"), 3);
  assert.equal(neighbor(thin, 1, "right"), 2);
  assert.equal(neighbor(thin, 3, "left"), 2);
  assert.equal(neighbor(thin, 1, "left"), undefined);
  assert.equal(neighbor(thin, 3, "right"), undefined);
  // Hidden panes all measure nothing.
  const hidden = new Map([[1, box(0, 0)], [2, box(0, 0)]]);
  assert.equal(neighbor(hidden, 1, "right"), undefined);
});
