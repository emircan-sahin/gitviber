import assert from "node:assert/strict";
import { test } from "node:test";
import { parseGuide, unplaced } from "./guide.ts";

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
  sections: [
    { title: "The retry helper", summary: "Backoff math.", files: ["src/retry.ts"], risk: "A wrong delay." },
    { title: "Uploader", summary: "Calls it.", files: ["src/upload.ts"], risk: "" },
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
  assert.deepEqual(parseGuide('{"title": "Only a title"}'), { title: "Only a title", summary: "", diagram: "", sections: [] });
  assert.deepEqual(parseGuide('{"title": 3, "summary": "S", "sections": [null, "x", {"files": "a.ts"}, {"title": "T", "files": ["a.ts"]}]}'), {
    title: "",
    summary: "S",
    diagram: "",
    sections: [{ title: "T", summary: "", files: ["a.ts"], risk: "" }],
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
  assert.deepEqual(g, { title: "T", summary: "", diagram: "", sections: [] });
  assert.deepEqual(unplaced({ title: "", summary: "", diagram: "", sections: [] }, ["a b/ü.ts"]), ["a b/ü.ts"]);
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
