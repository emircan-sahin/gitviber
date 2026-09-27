import { test } from "node:test";
import assert from "node:assert/strict";
import { parseMark } from "./commandMarks.ts";

test("OSC 133 marks, with their exit code and the extensions other shells add", () => {
  assert.deepEqual(parseMark("A"), { kind: "A" });
  // fish 4: kitty's click_events and cmdline_url.
  assert.deepEqual(parseMark("A;click_events=1"), { kind: "A" });
  assert.deepEqual(parseMark("C;cmdline_url=ls%20-l"), { kind: "C" });
  assert.deepEqual(parseMark("D;0"), { kind: "D", exit: 0 });
  assert.deepEqual(parseMark("D;130"), { kind: "D", exit: 130 });
  assert.deepEqual(parseMark("D;1;aid=12"), { kind: "D", exit: 1 });
  // bash with nothing run, and VS Code's: no status.
  assert.deepEqual(parseMark("D"), { kind: "D" });
  assert.deepEqual(parseMark("D;"), { kind: "D" });
  // VS Code's 633 kinds GitViber doesn't read.
  assert.equal(parseMark("P;Cwd=/tmp"), null);
  assert.equal(parseMark("E;ls"), null);
  assert.equal(parseMark(""), null);
});
