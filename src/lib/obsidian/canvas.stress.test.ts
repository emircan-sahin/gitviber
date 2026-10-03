// Canvases as they turn up: hand-edited, merged by sync, made by plugins, or broken.
import assert from "node:assert/strict";
import { test } from "node:test";
import { bounds, edgePath, parseCanvas } from "./canvas.ts";

const node = (id: string, x: number, y: number, extra: Record<string, unknown> = {}) => ({ id, type: "text", x, y, width: 250, height: 60, text: id, ...extra });

test("malformed canvases: not JSON throws (the view says so), wrong shapes are skipped", () => {
  for (const bad of ["{", '{"nodes": [', "nul"]) assert.throws(() => parseCanvas(bad), bad);
  for (const odd of ["null", "[]", "42", '"text"', '{"nodes": {}, "edges": "x"}', '{"nodes": [null, 1, "a", []]}']) assert.deepEqual(parseCanvas(odd), { nodes: [], edges: [] }, odd);
  const c = parseCanvas(
    JSON.stringify({
      nodes: [{ ...node("s", 0, 0), x: "10" }, { ...node("z", 0, 0), width: 0, height: 0 }, { ...node("n", 0, 0), x: null }, node("ok", 0, 0)],
    }),
  );
  assert.deepEqual(
    c.nodes.map((n) => n.id),
    ["ok"],
  );
});

test("cards point at missing files, URLs of any kind, and markup: kept as data for the view", () => {
  const c = parseCanvas(
    JSON.stringify({
      nodes: [
        node("f", 0, 0, { type: "file", file: "../../etc/passwd" }),
        node("u", 0, 100, { type: "link", url: "javascript:alert(1)" }),
        node("t", 0, 200, { text: "<img src=x onerror=alert(1)> [[Note]]" }),
        node("c", 0, 300, {
          color: "red; background: url(https://example.com/x)",
        }),
      ],
    }),
  );
  assert.equal(c.nodes.length, 4);
  assert.equal(c.nodes.find((n) => n.id === "c")!.color, "red; background: url(https://example.com/x)");
});

test("far-apart cards still have a finite box to fit", () => {
  const c = parseCanvas(JSON.stringify({ nodes: [node("a", -1e9, -1e9), node("b", 1e9, 1e9)] }));
  const box = bounds(c.nodes);
  assert.ok(Number.isFinite(box.width) && Number.isFinite(box.height), JSON.stringify(box));
  assert.equal(box.width, 2e9 + 250);
});

test("a big generated canvas (10,000 cards, 10,000 edges) parses and measures quickly", () => {
  const nodes = Array.from({ length: 10000 }, (_, i) => node(`n${i}`, (i % 100) * 300, Math.floor(i / 100) * 100));
  const edges = Array.from({ length: 10000 }, (_, i) => ({
    id: `e${i}`,
    fromNode: `n${i}`,
    toNode: `n${(i + 1) % 10000}`,
  }));
  const start = performance.now();
  const c = parseCanvas(JSON.stringify({ nodes, edges }));
  const box = bounds(c.nodes);
  for (const e of c.edges) edgePath(c.nodes[0], c.nodes[1], e.fromSide, e.toSide);
  assert.equal(c.edges.length, 10000);
  assert.ok(box.width > 0);
  assert.ok(performance.now() - start < 1000, `${Math.round(performance.now() - start)} ms`);
});

test("an edge from a card to itself draws a loop, not NaN", () => {
  const c = parseCanvas(
    JSON.stringify({
      nodes: [node("a", 0, 0)],
      edges: [{ id: "e", fromNode: "a", toNode: "a" }],
    }),
  );
  const { d, mid } = edgePath(c.nodes[0], c.nodes[0]);
  assert.doesNotMatch(d, /NaN|Infinity/);
  assert.ok(mid.every(Number.isFinite));
});
