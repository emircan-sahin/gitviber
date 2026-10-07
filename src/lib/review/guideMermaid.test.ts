import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
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

// What a label shows once drawn: Mermaid hides `#NN;` from its grammar (encodeEntities), and its
// SVG gets `&#NN;` back (decodeEntities), which the HTML parser prints as the character.
const entities = readdirSync(dir).find((f) => f.endsWith(".mjs") && readFileSync(new URL(f, dir), "utf8").includes("var encodeEntities ="));

async function drawnText(src: string) {
  const { encodeEntities, decodeEntities } = await import(new URL(entities!, dir).href);
  const calls = await statements(encodeEntities(src) + "\n");
  const shown = (s: string) =>
    decodeEntities(s)
      .replace(/<[^>]*>/g, "")
      .replace(/&#(\d+);/g, (_: string, n: string) => String.fromCodePoint(Number(n)));
  return {
    calls,
    nodes: calls.filter(([name, args]) => name === "addVertex" && args.length > 1).map(([, args]) => [args[0], shown((args[1] as { text: string }).text)]),
    edges: calls.filter(([name]) => name === "addLink").map(([, args]) => shown((args[2] as { text?: { text: string } }).text?.text ?? "")),
  };
}

const step = (label: string, i: number, file = ""): Flow["steps"][number] => ({ id: `${i}`, label, file, status: "changed", kind: i % 3 === 2 ? "decision" : "step", section: i % 2 ? 1 : null });
const meta = (s: Flow["steps"][number]) => `${s.section ? "01 " : ""}${[s.kind === "decision" ? "Decision" : "Changed", s.file.slice(s.file.lastIndexOf("/") + 1)].filter(Boolean).join(" · ")}`;

/** Each label, as a step and as an edge's label, draws as its own statement and reads back as written. */
async function roundTrips(labels: string[], files: string[] = []) {
  const steps = labels.map((l, i) => step(l, i, files[i % (files.length || 1)] ?? ""));
  const edges = steps.slice(1).map((s, i) => ({ from: `${i}`, to: s.id, label: labels[(i + 3) % labels.length] }));
  const { calls, nodes, edges: drawn } = await drawnText(flowSource({ title: "", steps, edges }));
  assert.deepEqual([...new Set(calls.map(([name]) => name))].sort(), edges.length ? ["addLink", "addVertex", "destructLink", "setClass", "setDirection"] : ["addVertex", "setClass", "setDirection"]);
  assert.deepEqual(nodes, steps.map((s, i) => [`s${i}`, meta(s) + s.label.replace(/\s+/g, " ")]));
  assert.deepEqual(drawn, edges.map((e) => e.label.replace(/\s+/g, " ")));
}

const lookalikes = [
  "＂] --> x[＂", // fullwidth quote and brackets
  "“quoted” «x» ‹y› ［a］ ｛b｝ ＜img＞ ｜pipe｜",
  "a --> b --- c ==> d -.-> e ~~~ f",
  "end",
  "subgraph x\nend",
  "click",
  "linkStyle 0 stroke:red",
  "%% comment",
  "A".repeat(5000),
  "😀 👨‍👩‍👧 🇹🇷 é (e + combining)",
  "‮evil‬ ‏ RTL مرحبا שלום ​ ﻿",
  "nul \u0000 lone \uD800 surrogate",
  "ﬂ°°60¶ß ﬂ°amp¶ß #60; &#60; &lt;",
];

test("labels Mermaid's grammar could take for syntax draw as written, through its entity passes", { skip: !chunk && "mermaid's flowchart chunk not found" }, async () => {
  await roundTrips(lookalikes, ["src/a b/ü.ts", 'x"].ts', ""]);
});

test("random labels draw as written, as one statement each", { skip: !chunk && "mermaid's flowchart chunk not found" }, async () => {
  let seed = 42;
  const rand = (n: number) => ((seed = (seed * 1103515245 + 12345) >>> 0) >>> 8) % n;
  const ascii = Array.from({ length: 95 }, (_, i) => String.fromCharCode(32 + i));
  // Words with a meaning to the grammar; "style", "classDef" and "direction" have tests of their own below.
  const words = ["end", "subgraph", "click", "call", "href", "-->", "---", "==>", "-.->", "|", ":::", "&", "%%", "{{", "}}", "((", "))", "[(", ")]", "\n", "\t"];
  const wide = [..."＂“”«»［］｛｝＜＞｜😀‮‏عשé́​ ﻿ﬂ°¶ß"];
  const pool = [...ascii, ...words, ...wide];
  for (let round = 0; round < 150; round++) {
    const labels = Array.from({ length: 1 + rand(12) }, () => Array.from({ length: 1 + rand(40) }, () => pool[rand(pool.length)]).join("").trim() || "x");
    await roundTrips(labels);
  }
});

// Mermaid's encodeEntities cuts the last ";" off a line matching /style.*:\S*#.*;/ (meant for
// `style a fill:#fff;`), so a "style" word, then a colon and an escaped character, loses one.
test("a label with a colon in a step of style.css draws as written", { todo: "mermaidText leaves ':' as is; escape it (#58;) so Mermaid's style/classDef rewrite never matches" }, async () => {
  await roundTrips(["color:#fff <b>", "classDef:#x>"], ["src/style.css"]);
});

// Mermaid's lexer reads any line holding "direction TB" (or BT, RL, LR, TD) as a direction
// statement, quotes or not: the step, or the edge, on that line is dropped without an error.
test("a label naming a direction draws as written", { todo: "mermaidText leaves the space in 'direction LR'; escape it (direction#32;LR) so the line stays a step" }, async () => {
  await roundTrips(["direction TB", "flex direction RL", "next"]);
});
