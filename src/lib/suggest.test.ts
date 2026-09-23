import assert from "node:assert/strict";
import { test } from "node:test";
import { parseSuggestion, programOf } from "./suggest.ts";

test("summary and body", () => {
  assert.deepEqual(parseSuggestion("Fix the thing\n\nIt was broken.\nNow it isn't.\n"), { summary: "Fix the thing", body: "It was broken.\nNow it isn't." });
  assert.deepEqual(parseSuggestion("  Only a summary  \n"), { summary: "Only a summary", body: "" });
  assert.deepEqual(parseSuggestion("Summary\r\n\r\n\r\nBody\r\n"), { summary: "Summary", body: "Body" });
  // A body that starts right under the summary still counts.
  assert.deepEqual(parseSuggestion("Summary\n- one\n- two"), { summary: "Summary", body: "- one\n- two" });
});

test("wrappers models add", () => {
  const plain = { summary: "feat: add x", body: "Because y." };
  assert.deepEqual(parseSuggestion("```\nfeat: add x\n\nBecause y.\n```"), plain);
  assert.deepEqual(parseSuggestion("Here's a message:\n\n```text\nfeat: add x\n\nBecause y.\n```\n"), plain);
  assert.deepEqual(parseSuggestion('"feat: add x\n\nBecause y."'), plain);
  assert.deepEqual(parseSuggestion("**feat: add x**\n\nBecause y."), plain);
  assert.deepEqual(parseSuggestion("# feat: add x\n\nBecause y."), plain);
  assert.deepEqual(parseSuggestion("Subject: feat: add x\n\nBody: Because y."), plain);
  // Quotes and backticks inside the message are its own.
  assert.deepEqual(parseSuggestion('Rename "a" to "b"'), { summary: 'Rename "a" to "b"', body: "" });
  assert.deepEqual(parseSuggestion("Use `x` over `y`"), { summary: "Use `x` over `y`", body: "" });
});

test("nothing to use", () => {
  assert.equal(parseSuggestion(""), null);
  assert.equal(parseSuggestion("  \n\n "), null);
  assert.equal(parseSuggestion("```\n\n```"), null);
});

test("program of a template", () => {
  assert.equal(programOf("  claude -p"), "claude");
  assert.equal(programOf("codex exec"), "codex");
});
