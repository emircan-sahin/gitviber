import assert from "node:assert/strict";
import { test } from "node:test";
import { languageFor } from "../editor/language.ts";
import { hasConflictMarkers, oursText, parseConflicts } from "./conflicts.ts";

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
