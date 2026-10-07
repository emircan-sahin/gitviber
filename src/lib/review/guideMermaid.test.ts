import assert from "node:assert/strict";
import { readdirSync } from "node:fs";
import { test } from "node:test";
import { type Flow, flowSource } from "./guide.ts";

// Mermaid's own flowchart grammar, read without a DOM: its parser calls a stand-in for its
// database, so this sees every statement a source makes. The chunk's name changes per release.
const dir = new URL("../../../node_modules/mermaid/dist/chunks/mermaid.core/", import.meta.url);
const chunk = readdirSync(dir).find((f) => /^flowDiagram-\w+\.mjs$/.test(f));

async function statements(src: string) {
  const { diagram } = await import(new URL(chunk!, dir).href);
  const calls: [string, unknown[]][] = [];
  const db: Record<string, unknown> = { lex: { firstGraph: () => true } };
  for (const name of "addClass addLink addSubGraph addVertex destructLink setAccDescription setAccTitle setClass setClickEvent setDirection setLink setTooltip updateLink updateLinkInterpolate".split(" "))
    db[name] = (...args: unknown[]) => {
      calls.push([name, args]);
      return name === "destructLink" ? { type: "arrow_point", stroke: "normal", length: 1 } : undefined;
    };
  Object.assign(diagram.parser.yy, db);
  diagram.parser.parse(src);
  return calls;
}

const hostile = [
  '"] --> x["',
  'x"]\nclick s0 call alert(1)\n',
  '%%{init: {"securityLevel": "loose"}}%%',
  "<img src=x onerror=alert(1)>",
  "a|b`c`;{d}[e] #lt; $$x$$\nend\nstyle s0 fill:red",
];

test("Mermaid reads a hostile flow as its steps and edges, nothing else", { skip: !chunk && "mermaid's flowchart chunk not found" }, async () => {
  const flow: Flow = {
    title: "",
    steps: hostile.map((label, i) => ({ id: `${i}`, label, file: label, status: "new", kind: i === 2 ? "decision" : "step", section: i ? 1 : null })),
    edges: [
      { from: "0", to: "1", label: "yes" },
      { from: "1", to: "2", label: hostile[1] },
    ],
  };
  const calls = await statements(flowSource(flow));
  const names = new Set(calls.map(([name]) => name));
  assert.deepEqual([...names].sort(), ["addLink", "addVertex", "destructLink", "setClass", "setDirection"]);
  // A vertex with its label for each step, and the edges' ends as given.
  const labelled = calls.filter(([name, args]) => name === "addVertex" && args.length > 1).map(([, args]) => args[0]);
  assert.deepEqual(labelled, ["s0", "s1", "s2", "s3", "s4"]);
  assert.deepEqual(
    calls.filter(([name]) => name === "addLink").map(([, args]) => [args[0], args[1]]),
    [
      [["s0"], ["s1"]],
      [["s1"], ["s2"]],
    ],
  );
  assert.equal(calls.find(([name, args]) => name === "addVertex" && args[0] === "s2" && args.length > 1)?.[1][2], "hexagon");
});
