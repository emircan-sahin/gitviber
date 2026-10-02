import assert from "node:assert/strict";
import { test } from "node:test";
import { languageFor } from "../editor/language.ts";
import { forTerminal } from "../review/notes.ts";
import { basesFor, endsWithNewline, hasConflictMarkers, mergingWhat, oursText, parseConflicts, resolvePrompt } from "./conflicts.ts";

test("conflicted files are sniffed without their markers", () => {
  const detect = (path: string, text: string) => languageFor(path, oursText(parseConflicts(text)!.segments));
  const json = '{\n<<<<<<< HEAD\n  "semi": false\n=======\n  "semi": true\n>>>>>>> main\n}\n';
  assert.equal(detect(".prettierrc", json), "jsonc");
  assert.equal(detect("bin/run", "<<<<<<< HEAD\n#!/usr/bin/env node\n=======\n#!/usr/bin/env bun\n>>>>>>> main\n"), "javascript");
});

test("a file is resolved once no conflict block is left", () => {
  assert.equal(hasConflictMarkers("a\n<<<<<<< HEAD\nb\n=======\nc\n>>>>>>> main\n"), true);
  assert.equal(hasConflictMarkers("a\n<<<<<<< HEAD\nb\n"), true);
  assert.equal(hasConflictMarkers("a\n=======\nb\n"), false);
  assert.equal(hasConflictMarkers("a\nb\n"), false);
});

const conflicts = (text: string) => parseConflicts(text)!.segments.filter((s) => s.t === "conflict");

test("each conflict gets the base of the diff3 conflict that holds its sides", () => {
  // Git's default style trimmed the shared "x" off and split the region in two; diff3 keeps it whole.
  const file = "a\n<<<<<<< HEAD\nB1\n=======\nb1\n>>>>>>> f\nx\n<<<<<<< HEAD\nB2\n=======\nb2\n>>>>>>> f\nz\n<<<<<<< HEAD\nedited\n=======\nq\n>>>>>>> f\n";
  const rebuilt = conflicts(
    "a\n<<<<<<< current\nB1\nx\nB2\n||||||| base\nb\nx\nc\n=======\nb1\nx\nb2\n>>>>>>> incoming\nz\n<<<<<<< current\np\n||||||| base\no\n=======\nq\n>>>>>>> incoming\n",
  );
  assert.deepEqual(basesFor(conflicts(file), rebuilt), [["b", "x", "c"], ["b", "x", "c"], null]);
  // diff3 style wrote its own: that one stands.
  const own = "<<<<<<< HEAD\nm\n||||||| base\nmine\n=======\nt\n>>>>>>> f\n";
  assert.deepEqual(basesFor(conflicts(own), []), [["mine"]]);
  // CRLF in the working file, LF in the stages.
  assert.deepEqual(basesFor(conflicts("<<<<<<< HEAD\r\nB\r\n=======\r\nb\r\n>>>>>>> f\r\n"), conflicts("<<<<<<< c\nB\n||||||| base\no\n=======\nb\n>>>>>>> i\n")), [["o"]]);
});

test("the prompt names the files and what's being merged", () => {
  assert.equal(
    resolvePrompt(["a.ts", "b.ts"], mergingWhat("merge", "Merge branch 'feat'", "main", "feat")),
    "Resolve the merge conflicts in a.ts, b.ts (merging feat into main); keep both sides' intent, then stage the files.",
  );
  assert.equal(mergingWhat("rebase", "feat", null, "1a2b3c4 (Add login)"), "rebasing feat, replaying 1a2b3c4 (Add login)");
  assert.equal(mergingWhat("cherry-pick", null, "main", null), "cherry-picking a commit");
  assert.equal(resolvePrompt(["a.ts"], null), "Resolve the merge conflicts in a.ts; keep both sides' intent, then stage the files.");
});

// From a real `git merge` (default style) and `git merge-file -p --diff3` on its stages: both functions
// got the same two edits, from different originals.
const twinFile = "fn a() {\n<<<<<<< HEAD\n  ours();\n=======\n  theirs();\n>>>>>>> feat\n}\n1\n2\n3\nfn b() {\n<<<<<<< HEAD\n  ours();\n=======\n  theirs();\n>>>>>>> feat\n}\n";
const twinRebuilt =
  "fn a() {\n<<<<<<< current\n  ours();\n||||||| base\n  old_a();\n=======\n  theirs();\n>>>>>>> incoming\n}\n1\n2\n3\nfn b() {\n<<<<<<< current\n  ours();\n||||||| base\n  old_b();\n=======\n  theirs();\n>>>>>>> incoming\n}\n";

test("two conflicts with the same text each get their own base", () => {
  assert.deepEqual(basesFor(conflicts(twinFile), conflicts(twinRebuilt)), [["  old_a();"], ["  old_b();"]]);
});

test("bases: zdiff3 and diff3 files keep their own, edited blocks get none", () => {
  // zdiff3 wrote the base itself; nothing rebuilt is needed or used.
  const z = "<<<<<<< HEAD\nx\n||||||| 1a2b3c4\no\n=======\ny\n>>>>>>> feat\n";
  assert.deepEqual(basesFor(conflicts(z), conflicts("<<<<<<< c\nx\n||||||| b\nWRONG\n=======\ny\n>>>>>>> i\n")), [["o"]]);
  // A block someone edited since (an agent half-way through) matches no rebuilt one.
  const edited = "<<<<<<< HEAD\nours, edited\n=======\ntheirs\n>>>>>>> feat\n";
  assert.deepEqual(basesFor(conflicts(edited), conflicts("<<<<<<< c\nours\n||||||| b\nbase\n=======\ntheirs\n>>>>>>> i\n")), [null]);
  // The stages couldn't be merged again (binary, a deleted side): no bases, no crash.
  assert.deepEqual(basesFor(conflicts("<<<<<<< HEAD\na\n=======\nb\n>>>>>>> f\n"), []), [null]);
  // One side deleted the lines: its empty side is within anything, the other side still has to match.
  const del = "<<<<<<< HEAD\n=======\nkept\n>>>>>>> f\n";
  const delRebuilt = "<<<<<<< c\nother\n||||||| b\nb1\n=======\nunrelated\n>>>>>>> i\nmid\n<<<<<<< c\n||||||| b\nb2\n=======\nkept\n>>>>>>> i\n";
  assert.deepEqual(basesFor(conflicts(del), conflicts(delRebuilt)), [["b2"]]);
});

test("a conflict at the end of a file without a trailing newline still parses and gets its base", () => {
  const file = "a\n<<<<<<< HEAD\nlast-ours\n=======\nlast-theirs\n>>>>>>> feat";
  const parsed = parseConflicts(file)!;
  assert.equal(parsed.trailingNewline, false);
  const rebuilt = "a\n<<<<<<< current\nlast-ours\n||||||| base\nlast\n=======\nlast-theirs\n>>>>>>> incoming\n";
  assert.deepEqual(basesFor(conflicts(file), conflicts(rebuilt)), [["last"]]);
});

test("the agent prompt pasted into a terminal carries no control characters and no Enter", () => {
  const names = ['it\'s "quoted".ts', "back`tick`$(id).ts", "new\nline.ts", "esc\x1b[201~\rrun.ts", "trail\n"];
  const pasted = forTerminal(resolvePrompt(names, mergingWhat("merge", null, "main", "feat\x1b]0;x\x07")));
  // Nothing that ends a bracketed paste or acts as a key: only text, tabs and inner line breaks.
  assert.doesNotMatch(pasted, /[\x00-\x08\x0b-\x1f\x7f]/);
  assert.doesNotMatch(pasted, /\n$/);
  assert.match(pasted, /^Resolve the merge conflicts in /);
  assert.ok(pasted.includes("back`tick`$(id).ts"));
  // Cherry-pick and revert name the commit from the incoming label; an unknown operation says nothing extra.
  assert.equal(mergingWhat("revert", null, "main", "parent of 1a2b3c4 (Fix)"), "reverting parent of 1a2b3c4 (Fix)");
  assert.equal(mergingWhat(undefined, null, "main", "x"), null);
  assert.equal(mergingWhat("merge", null, null, null), "merging the incoming branch into HEAD");
});

test("a conflict at the end of a file keeps the file's own last newline, or its lack", () => {
  // Neither side's file ends with a newline, but git ends the closing marker line with one.
  const parsed = parseConflicts("a\n<<<<<<< HEAD\nend-ours\n=======\nend-theirs\n>>>>>>> feat\n")!;
  assert.equal(parsed.trailingNewline, true);
  const none = { oursNewline: false, theirsNewline: false };
  for (const kind of ["ours", "theirs", "both", "custom"] as const) assert.equal(endsWithNewline(parsed, kind, none), false, kind);
  const mixed = { oursNewline: true, theirsNewline: false };
  assert.equal(endsWithNewline(parsed, "ours", mixed), true);
  assert.equal(endsWithNewline(parsed, "theirs", mixed), false);
  // Both: the incoming lines come last.
  assert.equal(endsWithNewline(parsed, "both", mixed), false);
  assert.equal(endsWithNewline(parsed, "custom", mixed), true);
  // Not known yet: as the file reads.
  assert.equal(endsWithNewline(parsed, "ours", null), true);
  // Text after the last conflict is the file's own end.
  const inner = parseConflicts("<<<<<<< HEAD\nx\n=======\ny\n>>>>>>> feat\nlast")!;
  assert.equal(endsWithNewline(inner, "ours", none), false);
  assert.equal(endsWithNewline(parseConflicts("<<<<<<< HEAD\nx\n=======\ny\n>>>>>>> feat\nlast\n")!, "ours", none), true);
});
