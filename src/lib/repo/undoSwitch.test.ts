import assert from "node:assert/strict";
import { test } from "node:test";
import type { FileChange } from "../api/types.ts";
import { landing, switchNote, switchQuestion } from "./undoSwitch.ts";

const at = (switchTo: string | null) => ({ switchTo });
const change = { path: "a.txt" } as FileChange;
const clean = { staged: [], unstaged: [] };
const dirty = { staged: [], unstaged: [change] };

test("landing is the last switch along the way", () => {
  assert.equal(landing([]), null);
  assert.equal(landing([at(null), at(null)]), null);
  assert.equal(landing([at("main"), at(null)]), "main");
  assert.equal(landing([at("main"), at("feat"), at(null)]), "feat");
});

test("switchNote says back only for undo", () => {
  assert.equal(switchNote(false, "main"), ": switches back to main");
  assert.equal(switchNote(true, "feat"), ": switches to feat");
  assert.equal(switchNote(false, null), "");
});

test("asks only when a switch takes uncommitted changes along", () => {
  assert.equal(switchQuestion(false, [at("main")], clean), null);
  assert.equal(switchQuestion(false, [at(null)], dirty), null);
  assert.equal(switchQuestion(false, [at("main")], null), null);
  assert.match(switchQuestion(false, [at("main")], dirty)!, /undo switches back to main/);
  assert.match(switchQuestion(true, [at("feat")], { staged: [change], unstaged: [] })!, /redo switches to feat/);
});
