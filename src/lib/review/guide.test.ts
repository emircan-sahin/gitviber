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
