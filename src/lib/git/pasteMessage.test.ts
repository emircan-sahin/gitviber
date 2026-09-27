import assert from "node:assert/strict";
import { test } from "node:test";
import { pasteMessage } from "./pasteMessage.ts";

test("a pasted message fills the summary at the caret and the description", () => {
  const empty = { summary: "", body: "" };
  assert.equal(pasteMessage(empty, "", "one line", 0, 0), null);
  // Blank lines before it are dropped, as git does.
  assert.deepEqual(pasteMessage(empty, "", "\n  \nfix: it\nWhy.", 0, 0), { summary: "fix: it", body: "Why.", caret: 7 });
  assert.deepEqual(pasteMessage(empty, "", "fix: it\r\n\r\n\r\nWhy.\nMore.\n", 0, 0), { summary: "fix: it", body: "Why.\nMore.", caret: 7 });
  // Over a selection, like any paste; a first line alone leaves the description be.
  assert.deepEqual(pasteMessage({ summary: "fix: old thing", body: "kept" }, "", "new\n", 5, 8), { summary: "fix: new thing", body: "kept", caret: 8 });
});

test("the rest replaces an untouched starting description and goes before the user's own text", () => {
  const template = "Refs: #";
  assert.equal(pasteMessage({ summary: "", body: template }, template, "a\nb", 0, 0)?.body, "b");
  assert.equal(pasteMessage({ summary: "", body: "mine" }, template, "a\n\nb", 0, 0)?.body, "b\n\nmine");
});
