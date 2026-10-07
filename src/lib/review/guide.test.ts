import assert from "node:assert/strict";
import { test } from "node:test";
import { CATEGORIES, describeFlow, flowSource, GUIDE_PROMPT, GUIDE_SCHEMA, type GuideSection, guidePrompt, LIMITS, mermaidText, parseGuide, placeFiles } from "./guide.ts";

// What a section from before categories and notes reads as.
const PLAIN: Pick<GuideSection, "category" | "check" | "importance" | "fileNotes" | "lineNotes"> = { category: "other", check: "", importance: "medium", fileNotes: [], lineNotes: [] };

const unplaced = (g: Parameters<typeof placeFiles>[0], paths: string[]) => placeFiles(g, paths).rest;

const answer = {
  title: "Add retry to the uploader",
  summary: "Uploads retry **three** times.",
  diagram: 'flowchart TD\n  A["upload.ts (changed)"] --> B["retry.ts (new)"]',
  sections: [
    { title: "The retry helper", summary: "Backoff math.", files: ["src/retry.ts"], risk: "A wrong delay." },
    { title: "Uploader", summary: "Calls it.", files: ["./src/upload.ts", 7, " "] },
  ],
};
const guide = {
  ...answer,
  models: [],
  flows: [],
  sections: [
    { title: "The retry helper", summary: "Backoff math.", files: ["src/retry.ts"], risk: "A wrong delay.", ...PLAIN },
    { title: "Uploader", summary: "Calls it.", files: ["src/upload.ts"], risk: "", ...PLAIN },
  ],
};

test("the JSON asked for", () => {
  assert.deepEqual(parseGuide(JSON.stringify(answer)), guide);
  assert.deepEqual(parseGuide(JSON.stringify(answer, null, 2).replace(/\n/g, "\r\n")), guide);
});

test("wrappers models add", () => {
  const json = JSON.stringify(answer, null, 2);
  assert.deepEqual(parseGuide("```json\n" + json + "\n```"), guide);
  assert.deepEqual(parseGuide("Here's the review:\n\n```\n" + json + "\n```\n\nLet me know."), guide);
  assert.deepEqual(parseGuide("Here's the review:\n" + json + "\nHope it helps."), guide);
  // A diagram fenced inside its string.
  const fenced = { ...answer, diagram: "```mermaid\n" + answer.diagram + "\n```" };
  assert.deepEqual(parseGuide(JSON.stringify(fenced)), guide);
});

test("missing or mistyped fields read as empty", () => {
  assert.deepEqual(parseGuide('{"title": "Only a title"}'), { title: "Only a title", summary: "", diagram: "", models: [], flows: [], sections: [] });
  assert.deepEqual(parseGuide('{"title": 3, "summary": "S", "sections": [null, "x", {"files": "a.ts"}, {"title": "T", "files": ["a.ts"]}]}'), {
    title: "",
    summary: "S",
    diagram: "",
    models: [],
    flows: [],
    sections: [{ title: "T", summary: "", files: ["a.ts"], risk: "", ...PLAIN }],
  });
});

test("anything else isn't a guide", () => {
  assert.equal(parseGuide(""), null);
  assert.equal(parseGuide("This commit adds a retry. {not json}"), null);
  assert.equal(parseGuide("[1, 2]"), null);
  assert.equal(parseGuide('{"sections": []}'), null);
});

test("files no section names", () => {
  assert.deepEqual(unplaced(guide, ["src/upload.ts", "README.md", "src/retry.ts", "docs/x.md"]), ["README.md", "docs/x.md"]);
});

const pretty = JSON.stringify(answer, null, 2);

test("odd wrappers that still read", () => {
  // A fenced snippet in the prose ahead of the fenced JSON.
  assert.deepEqual(parseGuide("The change:\n```ts\nretry()\n```\n\n```json\n" + pretty + "\n```"), guide);
  // A closing fence on the JSON's last line, and a byte-order mark.
  assert.deepEqual(parseGuide("```json\n" + pretty + "```"), guide);
  assert.deepEqual(parseGuide("﻿" + pretty), guide);
  // Backticks inside a Mermaid label are the diagram's, not a fence.
  const label = 'flowchart TD\n  A["`retry` (new)"] --> B';
  assert.equal(parseGuide(JSON.stringify({ ...answer, diagram: label }))?.diagram, label);
});

test("huge answers parse quickly, and brace soup isn't a guide", () => {
  const long = JSON.stringify({ ...answer, summary: "x".repeat(5_000_000) });
  let t = Date.now();
  assert.equal(parseGuide(long)?.summary.length, 5_000_000);
  assert.ok(Date.now() - t < 1000);
  t = Date.now();
  assert.equal(parseGuide("{".repeat(200_000) + "}".repeat(200_000)), null);
  assert.equal(parseGuide(("```\nx\n").repeat(20_000)), null);
  assert.ok(Date.now() - t < 1000);
});

test("empty and misnamed parts", () => {
  // A section with only a risk is dropped, as is one with blank files.
  const g = parseGuide(JSON.stringify({ title: "T", sections: [{ risk: "R" }, { files: ["", "  ", "./"] }] }));
  assert.deepEqual(g, { title: "T", summary: "", diagram: "", models: [], flows: [], sections: [] });
  assert.deepEqual(unplaced({ title: "", summary: "", diagram: "", models: [], flows: [], sections: [] }, ["a b/ü.ts"]), ["a b/ü.ts"]);
  // Paths with spaces and non-ASCII match as written.
  const named = parseGuide(JSON.stringify({ sections: [{ title: "x", files: ["docs/my notes/über.md"] }] }))!;
  assert.deepEqual(unplaced(named, ["docs/my notes/über.md"]), []);
});

// Real model output.
test("a brace in the prose before the JSON", () => {
  assert.deepEqual(parseGuide("I looked at the `{ retries }` option first. Here it is:\n" + pretty), guide);
});
test("a brace in the prose after the JSON", () => {
  assert.deepEqual(parseGuide(pretty + "\nAsk if you want more on `{ retries }`."), guide);
});
test("an object in the prose that isn't the guide", () => {
  assert.deepEqual(parseGuide('Options like `{}` or `{"retries": 3}` stay. Here it is:\n' + pretty), guide);
});
test("a trailing comma", () => {
  assert.deepEqual(parseGuide(pretty.replace('"src/retry.ts"\n', '"src/retry.ts",\n')), guide);
});
test("a file named with git's a/ or b/ prefix", () => {
  const g = parseGuide(JSON.stringify({ sections: [{ title: "x", files: ["b/src/retry.ts"] }] }))!;
  assert.deepEqual(unplaced(g, ["src/retry.ts"]), []);
});

// Structured diagrams (v2).
const v2 = {
  title: "One-claim codes",
  overview: "Codes claim once.\n\n1. Storage\n2. Lookup",
  diagram: {
    models: [
      {
        name: "PromoCode",
        file: "./src/promo/types.ts",
        status: "changed",
        note: "records retirement",
        section: 1,
        fields: [
          { name: "retiredAt", status: "new", note: "when it stopped", section: "01" },
          { name: "code", status: "weird", section: 9 },
          { status: "new" },
        ],
      },
      { file: "nameless.ts" },
    ],
    flows: [
      {
        title: "Claim",
        steps: [
          { id: "a", label: "normalizeClaimCode(wanted)", file: "src/promo/lookup.ts", status: "new", section: 2 },
          { id: "b", label: "Owner found?", kind: "decision", status: "same" },
          { id: "c", label: "claim()", status: "modified" },
          { id: "a", label: "a duplicate id" },
          { id: "d" },
        ],
        edges: [
          { from: "a", to: "b" },
          { from: "b", to: "c", label: "yes" },
          { from: "b", to: "nowhere", label: "no" },
          { from: "a", to: "b" },
        ],
      },
      { title: "Empty", steps: [] },
    ],
  },
  sections: [
    { title: "Storage", summary: "S1", files: ["src/promo/types.ts"] },
    { title: "Lookup", summary: "S2", files: ["src/promo/lookup.ts", "src/promo/types.ts", "src/gone.ts"] },
  ],
};

test("a structured diagram", () => {
  const g = parseGuide(JSON.stringify(v2))!;
  assert.equal(g.summary, v2.overview);
  assert.equal(g.diagram, "");
  assert.deepEqual(g.models, [
    {
      name: "PromoCode",
      file: "src/promo/types.ts",
      status: "changed",
      note: "records retirement",
      section: 1,
      fields: [
        { name: "retiredAt", note: "when it stopped", status: "new", section: 1 },
        { name: "code", note: "", status: "same", section: null },
      ],
    },
  ]);
  assert.equal(g.flows.length, 1);
  const [flow] = g.flows;
  assert.deepEqual(
    flow.steps.map((s) => [s.id, s.kind, s.status, s.section]),
    [
      ["a", "step", "new", 2],
      ["b", "decision", "same", null],
      ["c", "step", "changed", null],
    ],
  );
  assert.deepEqual(flow.edges, [
    { from: "a", to: "b", label: "" },
    { from: "b", to: "c", label: "yes" },
  ]);
});

test("a diagram past its limits is cut", () => {
  const many = (n: number, f: (i: number) => object) => Array.from({ length: n }, (_, i) => f(i));
  const big = {
    title: "T",
    diagram: {
      models: many(20, (i) => ({ name: `M${i}`, fields: many(50, (j) => ({ name: `f${j}` })) })),
      flows: many(5, () => ({ steps: many(40, (i) => ({ id: `s${i}`, label: "x".repeat(500) })), edges: many(39, (i) => ({ from: `s${i}`, to: `s${i + 1}` })) })),
    },
  };
  const g = parseGuide(JSON.stringify(big))!;
  assert.equal(g.models.length, 6);
  assert.ok(g.models.every((m) => m.fields.length === 12));
  assert.equal(g.flows.length, 2);
  assert.ok(g.flows.every((f) => f.steps.length === 12 && f.steps[0].label.length === 100));
  // Only the edges between steps kept.
  assert.ok(g.flows.every((f) => f.edges.length === 11));
});

test("mistyped diagrams read as none", () => {
  for (const diagram of [null, 3, [], { models: "x", flows: { steps: [] } }, { flows: [{ steps: "a" }, null] }]) {
    const g = parseGuide(JSON.stringify({ title: "T", diagram }))!;
    assert.deepEqual([g.diagram, g.models, g.flows], ["", [], []]);
  }
});

test("a v1 guide, as kept, still reads with its Mermaid diagram", () => {
  const g = parseGuide(JSON.stringify(answer))!;
  assert.equal(g.summary, answer.summary);
  assert.equal(g.diagram, answer.diagram);
  assert.deepEqual([g.models, g.flows], [[], []]);
});

test("files placed in sections", () => {
  const g = parseGuide(JSON.stringify(v2))!;
  assert.deepEqual(placeFiles(g, ["src/promo/types.ts", "src/promo/lookup.ts", "README.md"]), {
    shown: [["src/promo/types.ts"], ["src/promo/lookup.ts"]],
    named: [
      [],
      [
        { path: "src/promo/types.ts", at: 1 },
        { path: "src/gone.ts", at: null },
      ],
    ],
    rest: ["README.md"],
  });
});

test("the Mermaid source of a flow", () => {
  const [flow] = parseGuide(JSON.stringify(v2))!.flows;
  const src = flowSource(flow);
  assert.match(src, /^flowchart LR\n/);
  assert.match(src, /\n {2}s0\["<span class='gf-meta'><span class='gf-badge'>02<\/span> New #183; lookup\.ts<\/span><br\/><span class='gf-code'>normalizeClaimCode\(wanted\)<\/span>"\]:::gNew\n/);
  assert.match(src, /\n {2}s1\{\{"<span class='gf-meta'>Decision<\/span><br\/><span class='gf-ask'>Owner found\?<\/span>"\}\}:::gSame\n/);
  assert.match(src, /\n {2}s0 --> s1\n {2}s1 -->\|"yes"\| s2$/);
});

test("hostile labels stay text", () => {
  const hostile = [
    '"] --> x["',
    'x"]\nclick s0 call alert(1)\n%% ',
    "%%{init: {\"securityLevel\": \"loose\"}}%%",
    "<img src=x onerror=alert(1)>",
    "<script>alert(1)</script>",
    "$$\\href{javascript:alert(1)}{x}$$",
    "#lt;script#gt; &lt; ﬂ°°60¶ß",
    "a|b`c`;{d}[e]",
  ];
  for (const label of hostile) {
    const out = mermaidText(label);
    // Nothing that can end a string, start markup, an entity, a directive or a statement.
    assert.doesNotMatch(out, /["<>&%$|`\n{}[\]]/, label);
    assert.doesNotMatch(out, /#(?!\d+;)/, label);
    // Every character comes back as Mermaid prints it.
    assert.equal(
      out.replace(/#(\d+);/g, (_, n) => String.fromCodePoint(Number(n))),
      label.replace(/\s+/g, " "),
    );
  }
  const flow = { title: "", steps: hostile.map((label, i) => ({ id: `${i}`, label, file: label, status: "new" as const, kind: "step" as const, section: null })), edges: [{ from: "0", to: "1", label: hostile[1] }] };
  const src = flowSource(flow);
  // One statement a step and one for the edge, with every quote the source's own: a "click" is a label's word.
  assert.equal(src.split("\n").length, 1 + hostile.length + 1);
  assert.doesNotMatch(src, /%%|<script|<img/);
  assert.match(src.split("\n").at(-1)!, /^ {2}s0 -->\|"[^"]*"\| s1$/);
  for (const row of src.split("\n").slice(1, -1)) assert.match(row, /^ {2}s\d+\["[^"]*"\]:::gNew$/);
});

test("a flow in words", () => {
  const [flow] = parseGuide(JSON.stringify(v2))!.flows;
  assert.deepEqual(describeFlow(flow), [
    "normalizeClaimCode(wanted) (new in src/promo/lookup.ts), then Owner found?",
    "Owner found? (decision), then yes: claim()",
    "claim() (changed)",
  ]);
});

test("a hostile 5 MB answer parses fast, with its diagram capped", () => {
  const many = (n: number, f: (i: number) => object) => Array.from({ length: n }, (_, i) => f(i));
  const word = "x".repeat(200);
  const hostile = {
    title: "T",
    overview: "o",
    diagram: {
      models: many(2_000, (i) => ({ name: `M${i}`, section: i, fields: many(5, (j) => ({ name: `f${j} ${word}`, section: "1" })) })),
      flows: many(10, () => ({ steps: many(1_000, (i) => ({ id: `s${i % 900}`, label: word, kind: "decision", section: 2 })), edges: many(1_000, (i) => ({ from: `s${i}`, to: `s${i + 1}` })) })),
    },
    sections: many(10_000, (i) => ({ title: `S${i}`, summary: "s", files: [`src/f${i % 40}.ts`, `src/f${(i + 1) % 40}.ts`] })),
  };
  const json = JSON.stringify(hostile);
  assert.ok(json.length > 5_000_000, String(json.length));
  let t = performance.now();
  const g = parseGuide(json)!;
  assert.ok(performance.now() - t < 1500, `parsed in ${performance.now() - t} ms`);
  assert.equal(g.models.length, 6);
  assert.ok(g.models.every((m) => m.fields.length === 5 && m.fields.every((f) => f.name.length === 80 && f.section === 1)));
  // Section 0 isn't one; the rest name sections that exist.
  assert.deepEqual(
    g.models.map((m) => m.section),
    [null, 1, 2, 3, 4, 5],
  );
  assert.equal(g.flows.length, 2);
  assert.ok(g.flows.every((f) => f.steps.length === 12 && f.edges.length === 11));
  // 40 files over 10,000 sections: each shows once, in the first section naming it.
  t = performance.now();
  const placed = placeFiles(g, Array.from({ length: 40 }, (_, i) => `src/f${i}.ts`));
  assert.ok(performance.now() - t < 500, `placed in ${performance.now() - t} ms`);
  assert.equal(placed.shown.flat().length, 40);
  assert.deepEqual(placed.rest, []);
  assert.deepEqual(placed.shown.slice(0, 2), [["src/f0.ts", "src/f1.ts"], ["src/f2.ts"]]);
});

test("a hostile answer's sections are capped too", () => {
  const g = parseGuide(JSON.stringify({ title: "T", sections: Array.from({ length: 10_000 }, (_, i) => ({ title: `S${i}` })) }))!;
  assert.equal(g.sections.length, LIMITS.sections);
});

test("a file in three sections, and a guide of no sections", () => {
  const g = parseGuide(
    JSON.stringify({
      title: "T",
      sections: [
        { title: "a", files: ["b/x.ts"] },
        { title: "b", files: ["x.ts", "y.ts"] },
        { title: "c", files: ["a/x.ts", "gone.ts"] },
      ],
    }),
  )!;
  assert.deepEqual(placeFiles(g, ["x.ts", "y.ts", "z.ts"]), {
    shown: [["x.ts"], ["y.ts"], []],
    named: [
      [],
      [{ path: "x.ts", at: 1 }],
      [
        { path: "x.ts", at: 1 },
        { path: "gone.ts", at: null },
      ],
    ],
    rest: ["z.ts"],
  });
  const none = parseGuide(JSON.stringify({ title: "T", overview: "o", sections: [] }))!;
  assert.deepEqual(placeFiles(none, ["x.ts", "y.ts"]), { shown: [], named: [], rest: ["x.ts", "y.ts"] });
});

test("labels Mermaid would read as its own syntax, and bidi controls", () => {
  assert.equal(mermaidText("flex direction LR"), "flex direction#32;LR");
  assert.equal(mermaidText("style a fill:#fff;"), "style a fill#58;#35;fff#59;");
  assert.equal(mermaidText("a‮b⁦c⁩ d"), "abc d");
});

test("categories, importance, check and notes", () => {
  const answer = {
    title: "T",
    sections: [
      {
        title: "Token check",
        category: "Security",
        summary: "Why.",
        files: ["src/auth.ts", "src/token.ts"],
        check: "Sign in with\n an expired token.",
        importance: "High",
        fileNotes: [{ path: "./src/auth.ts", text: "Read first." }, { path: "elsewhere.ts", text: "Not its file." }, { path: "src/token.ts", text: " " }],
        lineNotes: [
          { path: "b/src/token.ts", side: "OLD", line: "12", text: "Was the only check.", critical: "true" },
          { path: "src/token.ts", line: 3, text: "New side by default." },
          { path: "src/token.ts", line: 0, text: "No line 0." },
          { path: "src/token.ts", line: -2, text: "Negative." },
          { path: "src/token.ts", line: 1.5, text: "Not whole." },
          { path: "src/token.ts", line: "x", text: "Not a number." },
          { path: "src/token.ts", line: [4], text: "Not one either." },
        ],
      },
      { title: "Odd", category: "frontend", importance: "urgent", critical: "no", files: ["a.ts"] },
    ],
  };
  const [s, odd] = parseGuide(JSON.stringify(answer))!.sections;
  assert.equal(s.category, "security");
  assert.equal(s.check, "Sign in with an expired token.");
  assert.equal(s.importance, "high");
  assert.deepEqual(s.fileNotes, [{ path: "src/auth.ts", text: "Read first.", critical: false }]);
  assert.deepEqual(s.lineNotes, [
    { path: "src/token.ts", text: "Was the only check.", critical: true, side: "old", line: 12 },
    { path: "src/token.ts", text: "New side by default.", critical: false, side: "new", line: 3 },
  ]);
  // A category the prompt doesn't have reads as other; an importance it doesn't have as medium.
  assert.equal(odd.category, "other");
  assert.equal(odd.importance, "medium");
});

test("a guide from before importance: critical reads as high, the rest as medium", () => {
  const sections = [
    { title: "Auth", files: ["a.ts"], critical: true },
    { title: "Old string", files: ["b.ts"], critical: "true" },
    { title: "Not critical", files: ["c.ts"], critical: false },
    { title: "Never said", files: ["d.ts"] },
    // A guide that has both goes by importance.
    { title: "Both", files: ["e.ts"], critical: true, importance: "low" },
  ];
  assert.deepEqual(
    parseGuide(JSON.stringify({ title: "T", sections }))!.sections.map((s) => s.importance),
    ["high", "high", "medium", "medium", "low"],
  );
});

test("the language line ends the prompt, one line of the user's text", () => {
  assert.ok(guidePrompt("Turkish").startsWith(`${GUIDE_PROMPT}\n\n`));
  assert.match(guidePrompt("Turkish"), /\n\nWrite all prose \(the title, overview, summaries, checks, risks and notes\) in Turkish; keep the JSON keys .*code, identifiers, paths and quoted strings as they are\.$/);
  for (const blank of ["", "  \n "]) assert.ok(guidePrompt(blank).includes(" in English; "));
  const odd = guidePrompt(" Brazilian\n\nPortuguese ").slice(GUIDE_PROMPT.length);
  assert.ok(odd.includes(" in Brazilian Portuguese; "));
  assert.equal(odd.split("\n").length, 3);
  assert.ok(!guidePrompt("x".repeat(500)).includes("x".repeat(41)));
});

test("notes are clamped", () => {
  const many = Array.from({ length: 30 }, (_, i) => ({ path: "a.ts", text: `n${i}` }));
  const long = "x".repeat(2000);
  const g = parseGuide(JSON.stringify({ title: "T", sections: [{ files: ["a.ts"], fileNotes: [...many, { path: "a.ts", text: long }], lineNotes: many.map((n, i) => ({ ...n, line: i + 1 })) }] }))!;
  const [s] = g.sections;
  assert.equal(s.fileNotes.length + s.lineNotes.length, LIMITS.notes);
  assert.equal(s.fileNotes.length, 30 + 1);
  assert.equal(s.fileNotes[30].text.length, LIMITS.note);
  assert.ok(s.fileNotes[30].text.endsWith("…"));
});

test("the prompt draws its shape from the schema it's checked against", () => {
  const sections = GUIDE_SCHEMA.properties.sections.items;
  assert.deepEqual(sections.properties.category.enum, Object.keys(CATEGORIES));
  // Only what every guide has is required.
  assert.deepEqual(GUIDE_SCHEMA.required, ["title", "overview", "sections"]);
  assert.deepEqual(sections.required, ["title", "category", "summary", "files"]);
  for (const key of Object.keys(sections.properties)) assert.ok(GUIDE_PROMPT.includes(`"${key}": `), key);
  for (const key of Object.keys(CATEGORIES)) assert.ok(GUIDE_PROMPT.includes(`${key} (`), key);
  // The example is valid JSON with every part the schema has.
  const shape = GUIDE_PROMPT.slice(GUIDE_PROMPT.indexOf("{"), GUIDE_PROMPT.indexOf("}.\n\n") + 1);
  const example = JSON.parse(shape);
  assert.equal(example.sections[0].lineNotes[0].line, 12);
  assert.equal(example.sections[0].importance, "high, medium or low");
  assert.equal(example.sections[0].fileNotes[0].critical, false);
  assert.ok(parseGuide(shape));
  // Small enough to pass as one argument.
  assert.ok(JSON.stringify(GUIDE_SCHEMA).length < 8000);
});

test("10,000 notes on a section parse fast and keep only its own files' first 40", () => {
  const notes = Array.from({ length: 10_000 }, (_, i) => ({ path: i % 2 ? "src/own.ts" : "src/other.ts", line: i + 1, text: `n${i}` }));
  const sections = [
    { title: "Own", files: ["src/own.ts"], fileNotes: notes, lineNotes: notes },
    { title: "Other", files: ["src/other.ts"] },
  ];
  const start = performance.now();
  const [own, other] = parseGuide(JSON.stringify({ title: "T", sections }))!.sections;
  assert.ok(performance.now() - start < 500);
  // A note on a changed file of another section isn't this section's to make.
  assert.ok([...own.fileNotes, ...own.lineNotes].every((n) => n.path === "src/own.ts"));
  assert.equal(own.fileNotes.length, LIMITS.notes);
  assert.equal(own.lineNotes.length, 0);
  assert.deepEqual([other.fileNotes, other.lineNotes], [[], []]);
});

test("line numbers past what a file can have are still whole lines; unsafe ones go", () => {
  const lineNotes = [1e6, 2 ** 53, 1e300, "007", " 9 ", "1e3"].map((line) => ({ path: "a.ts", line, text: "x" }));
  const [s] = parseGuide(JSON.stringify({ title: "T", sections: [{ files: ["a.ts"], lineNotes }] }))!.sections;
  assert.deepEqual(s.lineNotes.map((n) => n.line), [1e6, 7, 9]);
});

test("a category named like an Object property reads as other", () => {
  const sections = ["constructor", "__proto__", "toString", "hasOwnProperty"].map((category) => ({ title: category, category, files: [] }));
  const g = parseGuide(JSON.stringify({ title: "T", sections }))!;
  assert.deepEqual(g.sections.map((s) => s.category), ["other", "other", "other", "other"]);
});
