import assert from "node:assert/strict";
import { test } from "node:test";
import { bounds, canvasColor, edgePath, facing, parseCanvas } from "./canvas.ts";

test("a canvas keeps what the spec allows, groups first", () => {
  const canvas = parseCanvas(
    JSON.stringify({
      nodes: [
        { id: "t", type: "text", x: 0, y: 0, width: 200, height: 100, text: "# Hi", color: "4" },
        { id: "g", type: "group", x: -20, y: -40, width: 500, height: 300, label: "Plan" },
        { id: "f", type: "file", x: 300, y: 0, width: 200, height: 100, file: "Notes/Idea.md", subpath: "#Part" },
        { id: "bad", type: "text", x: 0, y: 0 },
        { id: "w", type: "widget", x: 0, y: 0, width: 1, height: 1 },
        { id: "t", type: "text", x: 9, y: 9, width: 50, height: 50, text: "same id" },
        { id: "n", type: "text", x: 0, y: 0, width: -10, height: 50 },
      ],
      edges: [
        { id: "e1", fromNode: "t", toNode: "f", fromSide: "right", toSide: "left" },
        { id: "e2", fromNode: "t", toNode: "missing" },
        { id: "e3", fromNode: "f", toNode: "t", fromEnd: "arrow", toEnd: "none", fromSide: "middle" },
      ],
    }),
  );
  assert.deepEqual(
    canvas.nodes.map((n) => n.id),
    ["g", "t", "f"],
  );
  assert.equal(canvas.nodes[2].subpath, "#Part");
  assert.deepEqual(
    canvas.edges.map((e) => [e.id, e.fromEnd, e.toEnd, e.fromSide]),
    [
      ["e1", "none", "arrow", "right"],
      ["e3", "arrow", "none", undefined],
    ],
  );
  assert.deepEqual(parseCanvas(""), { nodes: [], edges: [] });
  assert.throws(() => parseCanvas("{nope"));
});

test("geometry: bounds, facing sides and edge curves", () => {
  const a = { x: 0, y: 0, width: 100, height: 50 };
  const b = { x: 300, y: 0, width: 100, height: 50 };
  assert.deepEqual(bounds([a, b]), { x: 0, y: 0, width: 400, height: 50 });
  const many = Array.from({ length: 200_000 }, (_, i) => ({ x: i, y: -i, width: 1, height: 1 }));
  assert.deepEqual(bounds(many), { x: 0, y: -199_999, width: 200_000, height: 200_000 });
  assert.equal(facing(a, b), "right");
  assert.equal(facing(b, a), "left");
  assert.equal(facing(a, { x: 0, y: 400, width: 10, height: 10 }), "bottom");
  const e = edgePath(a, b);
  assert.match(e.d, /^M 100 25 C /);
  assert.match(e.d, / 300 25$/);
  assert.deepEqual(e.mid, [200, 25]);
});

test("colors: Obsidian's presets by theme, hex as written, nothing else", () => {
  assert.equal(canvasColor("1", true), "rgb(251 70 76 / 1)");
  assert.equal(canvasColor("4", false, 0.1), "rgb(8 185 78 / 0.1)");
  assert.equal(canvasColor("#ff8800", true), "#ff8800");
  assert.equal(canvasColor("red; background: url(x)", true), null);
  assert.equal(canvasColor(undefined, true), null);
  assert.equal(canvasColor("constructor", true), null);
});
