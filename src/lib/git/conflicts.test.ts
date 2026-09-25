import assert from "node:assert/strict";
import { test } from "node:test";
import { languageFor } from "../editor/language.ts";
import { oursText, parseConflicts } from "./conflicts.ts";

test("conflicted files are sniffed without their markers", () => {
  const detect = (path: string, text: string) => languageFor(path, oursText(parseConflicts(text)!.segments));
  const json = '{\n<<<<<<< HEAD\n  "semi": false\n=======\n  "semi": true\n>>>>>>> main\n}\n';
  assert.equal(detect(".prettierrc", json), "jsonc");
  assert.equal(detect("bin/run", "<<<<<<< HEAD\n#!/usr/bin/env node\n=======\n#!/usr/bin/env bun\n>>>>>>> main\n"), "javascript");
});
