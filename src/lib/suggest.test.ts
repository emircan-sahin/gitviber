import assert from "node:assert/strict";
import { test } from "node:test";
import { commandLine, parseSuggestion, programOf } from "./suggest.ts";

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

test("a preset runs with its model", () => {
  assert.equal(commandLine(" claude -p ", {}), "claude -p --model claude-sonnet-5");
  assert.equal(commandLine("codex exec", { codex: "gpt-x" }), "codex exec -m gpt-x");
  // Empty leaves the model to the CLI; a custom command carries its own.
  assert.equal(commandLine("claude -p", { claude: " " }), "claude -p");
  assert.equal(commandLine("pi -p --model a/b", { claude: "x" }), "pi -p --model a/b");
  assert.equal(commandLine("pi -p --no-tools --no-session", {}), "pi -p --no-tools --no-session --model anthropic/claude-sonnet-5");
  assert.equal(commandLine("opencode run --agent plan", { opencode: "zai/glm-5.3" }), "opencode run --agent plan -m zai/glm-5.3");
});
