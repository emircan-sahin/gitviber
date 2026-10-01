import assert from "node:assert/strict";
import { test } from "node:test";
import { checkNotes, findLines, forTerminal, notesPrompt, type ReviewNote } from "./notes.ts";

const note = (over: Partial<ReviewNote> = {}): ReviewNote => ({ id: "1", path: "src/a.ts", start: 2, end: 3, code: ["b", "c"], body: "Use the helper here.", ...over });

test("a note's lines are found where they were, else where they moved nearest", () => {
  assert.equal(findLines(["a", "b", "c"], ["b", "c"], 2), 2);
  assert.equal(findLines(["x", "a", "b", "c"], ["b", "c"], 2), 3);
  // Two copies: the one nearer where it was.
  assert.equal(findLines(["b", "x", "x", "x", "x", "b"], ["b"], 5), 6);
  assert.equal(findLines(["a", "b", "x"], ["b", "c"], 2), null);
  assert.equal(findLines(["a"], [], 1), null);
});

test("live notes follow their lines on disk, turn outdated when they change, and back", () => {
  const notes = [note()];
  assert.equal(checkNotes(notes, "src/a.ts", ["a", "b", "c"]), notes);
  const moved = checkNotes(notes, "src/a.ts", ["new", "a", "b", "c"]);
  assert.deepEqual(moved[0], { ...note(), start: 3, end: 4, outdated: false });
  const rewritten = checkNotes(moved, "src/a.ts", ["a", "b", "C"]);
  assert.equal(rewritten[0].outdated, true);
  assert.equal(rewritten[0].start, 3);
  assert.equal(checkNotes(rewritten, "src/a.ts", ["a", "b", "c"])[0].outdated, false);
  assert.equal(checkNotes(notes, "src/a.ts", null)[0].outdated, true);
});

test("notes on other files, old sides, commits and resolved ones are left as they are", () => {
  const notes = [note({ path: "b.ts" }), note({ old: true }), note({ at: "commit 1a2b3c4" }), note({ resolved: true })];
  assert.equal(checkNotes(notes, "src/a.ts", ["x"]), notes);
});

test("the prompt gives each note's place, its code fenced, and the comment", () => {
  assert.equal(notesPrompt([note()]), "`src/a.ts:2-3`\n```ts\nb\nc\n```\nUse the helper here.");
  assert.equal(
    notesPrompt([note({ start: 7, end: 7, code: ["x"], body: "Why?\n", old: true, at: "commit 1a2b3c4" }), note({ path: "Makefile", outdated: true })]),
    "`src/a.ts:7` (the old version; in commit 1a2b3c4)\n```ts\nx\n```\nWhy?\n\n`Makefile:2-3` (outdated: these lines have changed since)\n```\nb\nc\n```\nUse the helper here.",
  );
});

test("code holding backticks gets a longer fence", () => {
  assert.match(notesPrompt([note({ code: ["const s = ```;"] })]), /\n````ts\nconst s = ```;\n````\n/);
});

test("text for the terminal loses control characters and the line break that would press Enter", () => {
  assert.equal(forTerminal("a\r\nb\x1b[201~c\x07\tok\n\n"), "a\nb[201~c\tok");
});
