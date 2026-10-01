import { test } from "node:test";
import assert from "node:assert/strict";
import { mouseLeftOn, parseMark } from "./commandMarks.ts";

test("OSC 133 marks, with their exit code and the extensions other shells add", () => {
  assert.deepEqual(parseMark("A"), { kind: "A" });
  // fish 4: kitty's click_events and cmdline_url.
  assert.deepEqual(parseMark("A;click_events=1"), { kind: "A" });
  assert.deepEqual(parseMark("C;cmdline_url=ls%20-l"), { kind: "C" });
  assert.deepEqual(parseMark("D;0"), { kind: "D", exit: 0 });
  assert.deepEqual(parseMark("D;130"), { kind: "D", exit: 130 });
  assert.deepEqual(parseMark("D;1;aid=12"), { kind: "D", exit: 1 });
  // No status given.
  assert.deepEqual(parseMark("D"), { kind: "D" });
  assert.deepEqual(parseMark("D;"), { kind: "D" });
  // Ghostty's continuation-line mark isn't read.
  assert.equal(parseMark("P;k=s"), null);
  assert.equal(parseMark(""), null);
});

test("a prompt turns off mouse reporting a dead program left on, but not under tmux", () => {
  assert.equal(mouseLeftOn("A", "normal", "any"), true);
  assert.equal(mouseLeftOn("A", "normal", "vt200"), true);
  assert.equal(mouseLeftOn("A", "normal", "x10"), true);
  assert.equal(mouseLeftOn("A", "normal", "none"), false);
  // The alternate buffer's prompt is a shell's under a full-screen program, which owns the mouse.
  assert.equal(mouseLeftOn("A", "alternate", "any"), false);
  assert.equal(mouseLeftOn("B", "normal", "any"), false);
  assert.equal(mouseLeftOn("C", "normal", "any"), false);
  assert.equal(mouseLeftOn("D", "normal", "drag"), false);
});
