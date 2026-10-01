import assert from "node:assert/strict";
import { test } from "node:test";
import { anchorAt, checkNotes, findNote, forTerminal, isNote, notesPrompt, placeNotes, type ReviewNote, shifted } from "./notes.ts";

const note = (over: Partial<ReviewNote> = {}): ReviewNote => ({ id: "1", path: "src/a.ts", start: 2, end: 3, code: ["b", "c"], body: "Use the helper here.", ...over });
/** A note written on lines `start`–`end` of `lines`. */
const on = (lines: string[], start: number, end = start, over: Partial<ReviewNote> = {}): ReviewNote => ({ id: "1", path: "src/a.ts", body: "x", ...anchorAt(lines, start, end), ...over });

// Two `return null;` and two `}` in one file: the shapes a note on a common line has to tell apart.
const FILE = ["function a() {", "  if (x) {", "    return null;", "  }", "}", "", "function b() {", "  if (y) {", "    return null;", "  }", "}", ""];

test("a note keeps the lines around it, fewer at the file's start and end", () => {
  assert.deepEqual(anchorAt(FILE, 3, 3), { start: 3, end: 3, code: ["    return null;"], before: ["function a() {", "  if (x) {"], after: ["  }", "}"] });
  assert.deepEqual(anchorAt(FILE, 1, 1).before, []);
  assert.deepEqual(anchorAt(FILE, 12, 12).after, []);
});

test("a long run keeps its start and end only, and the prompt says how much is left out", () => {
  const lines = Array.from({ length: 100 }, (_, i) => `l${i + 1}`);
  const n = on(lines, 1, 100);
  assert.equal(n.code.length, 20);
  assert.deepEqual(n.tail?.slice(-1), ["l100"]);
  assert.equal(findNote(["new", ...lines], n), 2);
  assert.equal(findNote(lines.map((l) => (l === "l100" ? "x" : l)), n), null);
  assert.match(notesPrompt([n]), /\nl20\n⋯ 60 more lines ⋯\nl81\n/);
});

test("a long run rewritten in its middle turns outdated; a note saved before the hash was kept still follows its ends", () => {
  const lines = Array.from({ length: 100 }, (_, i) => `l${i + 1}`);
  const n = on(lines, 1, 100);
  const edited = lines.map((l) => (l === "l50" ? "x" : l));
  assert.equal(findNote(edited, n), null);
  assert.equal(checkNotes([n], "src/a.ts", edited)[0].outdated, true);
  const { hash, ...older } = n;
  assert.equal(typeof hash, "string");
  assert.equal(findNote(edited, older), 1);
});

test("lines inserted between two same-looking lines: the note stays on its own", () => {
  const n = on(FILE, 9);
  const edited = [...FILE.slice(0, 6), ...Array.from({ length: 30 }, (_, i) => `// ${i}`), ...FILE.slice(6)];
  assert.equal(findNote(edited, n), 39);
  assert.deepEqual(checkNotes([n], "src/a.ts", edited).map((x) => [x.start, x.outdated]), [[39, false]]);
});

test("its line rewritten while a same-looking one is elsewhere: outdated, not moved there", () => {
  const n = on(FILE, 9);
  const edited = FILE.map((l, i) => (i === 8 ? "    return fallback;" : l));
  assert.equal(findNote(edited, n), null);
  assert.equal(checkNotes([n], "src/a.ts", edited)[0].outdated, true);
  assert.equal(checkNotes([n], "src/a.ts", edited, FILE)[0].outdated, true);
});

test("a line edited right next to it keeps it where it was; moved with its surroundings rewritten, it's lost", () => {
  const n = on(FILE, 9);
  assert.equal(findNote(FILE.map((l, i) => (i === 7 ? "  if (z) {" : l)), n), 9);
  const rewritten = FILE.map((l, i) => (i === 7 ? "  if (z) {" : i === 9 ? "  } // done" : l));
  assert.equal(findNote(rewritten, n), 9);
  assert.equal(findNote(["// moved", ...rewritten], n), null);
  assert.equal(findNote(["// moved", ...rewritten], n, shifted(FILE, ["// moved", ...rewritten], 9, 9)), 10, "unless the edits say it moved there");
});

test("the lines a short side kept at the file's start still have to match", () => {
  const n = on(["x", "a"], 2);
  assert.equal(findNote(["x", "a", "more"], n), 2);
  assert.equal(findNote(["new", "x", "a"], n), 3);
  assert.equal(findNote(["y", "z", "a"], n), null);
});

test("of several equal places, the nearest to where it's expected", () => {
  const lines = ["a", "b", "c", "a", "b", "c", "a", "b", "c", "a", "b", "c"];
  const n = on(lines, 5);
  assert.equal(findNote(lines, n), 5);
  assert.equal(findNote(lines, n, { expect: 8, lo: 0, hi: 99 }), 8);
  // A tie in distance goes to the first.
  assert.equal(findNote(lines, n, { expect: 6.5, lo: 0, hi: 99 }), 5);
  assert.equal(findNote(["a", "b", "c"], on(["a", "b", "c"], 3)), 3, "the last line");
});

test("notes from before the lines around were kept are placed by their own lines, nearest first", () => {
  const n = note();
  assert.equal(findNote(["a", "b", "c"], n), 2);
  assert.equal(findNote(["b", "c", "x", "x", "x", "b", "c"], { ...n, start: 5, end: 6 }), 6);
  assert.equal(findNote(["a"], { ...n, code: [] }), null);
});

test("edits move a note's expected place by the lines around it each version has once", () => {
  assert.deepEqual(shifted(["u", "x", "x"], ["new", "new", "u", "x", "x"], 3, 3), { expect: 5, lo: 3, hi: 6 });
  assert.deepEqual(shifted(["x", "x", "v"], ["x", "x", "w", "v"], 2, 2), { expect: 3, lo: 0, hi: 4 }, "counted from below");
  assert.deepEqual(shifted(["x", "", "x"], ["x", "", "x"], 2, 2), { expect: 2, lo: 0, hi: 4 }, "repeated and blank lines anchor nothing");
  // Lines inserted right above a note whose block repeats below: the note keeps to the first.
  const prev = ["head", "a", "b", "c", "a", "b", "c"];
  const next = ["head", "1", "2", "3", "a", "b", "c", "a", "b", "c"];
  assert.deepEqual(checkNotes([on(prev, 3)], "src/a.ts", next, prev).map((x) => [x.start, x.outdated]), [[6, false]]);
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

test("a file on show places its open notes on the side and under the name they were written on", () => {
  const newer = note({ id: "new", start: 5, end: 6 });
  const old = note({ id: "old", path: "src/old.ts", old: true, code: ["x"], start: 1, end: 1 });
  const placed = placeNotes([newer, old, note({ id: "done", resolved: true }), note({ id: "other", path: "c.ts" })], {
    path: "src/a.ts",
    oldPath: "src/old.ts",
    oldLines: ["x"],
    newLines: ["a", "b", "c"],
  });
  assert.deepEqual(
    placed.map((p) => [p.note.id, p.old, p.start, p.end]),
    [
      ["new", false, 2, 3],
      ["old", true, 1, 1],
    ],
  );
  assert.deepEqual(placeNotes([newer], { path: "src/a.ts", oldPath: "src/a.ts", oldLines: null, newLines: null }), []);
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
  assert.equal(forTerminal("a\rb\x7fc\r"), "a\nbc");
});

test("stored notes that aren't notes are dropped", () => {
  assert.equal(isNote(note()), true);
  assert.equal(isNote({ ...anchorAt(["a"], 1, 1), id: "1", path: "p", body: "" }), true);
  for (const bad of [null, [], "x", { ...note(), code: "b" }, { ...note(), start: "2" }, { ...note(), end: 1 }, { ...note(), before: [1] }, { ...note(), hash: 1 }, { ...note(), id: undefined }])
    assert.equal(isNote(bad), false, JSON.stringify(bad));
});
