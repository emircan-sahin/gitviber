import { test } from "node:test";
import assert from "node:assert/strict";
import { cwdFromOsc7, parseMark, unescape633 } from "./commandMarks.ts";

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

test("the folder an OSC 7 or VS Code's 633 reports", () => {
  assert.equal(cwdFromOsc7("file://mac.local/Users/me/my%20app"), "/Users/me/my app");
  assert.equal(cwdFromOsc7("file:///tmp"), "/tmp");
  // kitty's form takes the path as it is: a % in a folder's name stays.
  assert.equal(cwdFromOsc7("kitty-shell-cwd://mac.local/Users/me/100%25"), "/Users/me/100%25");
  // Not percent-encoded after all: kept as sent.
  assert.equal(cwdFromOsc7("file://h/tmp/50%"), "/tmp/50%");
  assert.equal(cwdFromOsc7("http://example.com/x"), null);
  assert.equal(cwdFromOsc7("file://host"), null);
  assert.equal(unescape633("/Users/me/a\\x3bb"), "/Users/me/a;b");
  assert.equal(unescape633("C:\\\\Users"), "C:\\Users");
});
