import assert from "node:assert/strict";
import { test } from "node:test";
import { commandLine, effortArg, effortLevels, effortOf, parseSuggestion, programOf, runDetails, type SuggestPreset } from "./suggest.ts";

const LEAN = "--strict-mcp-config --no-session-persistence --disable-slash-commands --tools Read,Grep,Glob";

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
  assert.equal(programOf("~/.local/bin/claude -p"), "claude");
});

test("a preset runs with its model and effort", () => {
  // Medium until another is picked; Claude Code's runs are lean.
  assert.equal(commandLine(" claude -p ", {}, {}), `claude -p ${LEAN} --model claude-sonnet-5 --effort medium`);
  assert.equal(commandLine("claude -p", {}, { claude: "high" }), `claude -p ${LEAN} --model claude-sonnet-5 --effort high`);
  assert.equal(commandLine("codex exec", { codex: "gpt-x" }, { codex: "high" }), "codex exec -m gpt-x -c model_reasoning_effort=high");
  assert.equal(commandLine("codex exec", {}, {}), "codex exec -m gpt-6-luna -c model_reasoning_effort=medium");
  // Empty leaves the model or the effort to the CLI; a custom command carries its own.
  assert.equal(commandLine("claude -p", { claude: " " }, { claude: "" }), `claude -p ${LEAN}`);
  assert.equal(commandLine("pi -p --model a/b", { claude: "x" }, { pi: "high" }), "pi -p --model a/b");
  assert.equal(commandLine("pi -p --no-tools --no-session", {}, { pi: "off" }), "pi -p --no-tools --no-session --model anthropic/claude-sonnet-5 --thinking off");
  // opencode's levels are per model: none until one is picked.
  assert.equal(commandLine("opencode run --agent plan", { opencode: "zai/glm-5.3" }, {}), "opencode run --agent plan -m zai/glm-5.3");
  assert.equal(commandLine("opencode run --agent plan", {}, { opencode: "max" }), "opencode run --agent plan -m anthropic/claude-sonnet-5 --variant max");
  // llm has no effort flag.
  assert.equal(commandLine("llm", {}, { llm: "high" }), "llm -m gpt-6-luna");
});

test("a markdown description keeps its code blocks", () => {
  const answer = "Add x\n\n## Why\nBecause.\n\n```ts\nx();\n```";
  assert.deepEqual(parseSuggestion(answer, true), { summary: "Add x", body: "## Why\nBecause.\n\n```ts\nx();\n```" });
  assert.deepEqual(parseSuggestion("```markdown\n" + answer + "\n```", true), parseSuggestion(answer, true));
  assert.deepEqual(parseSuggestion("Title: Add x\n\nDescription: Because.", true), { summary: "Add x", body: "Because." });
  // Words around the fence, or code right under the title.
  const inner = "```markdown\n" + answer + "\n```";
  assert.deepEqual(parseSuggestion("Here's the pull request:\n\n" + inner, true), parseSuggestion(answer, true));
  assert.deepEqual(parseSuggestion(inner + "\n\nLet me know if you want changes.", true), parseSuggestion(answer, true));
  assert.deepEqual(parseSuggestion("Here it is:\n" + inner + "\nAnything else?", true), parseSuggestion(answer, true));
  assert.deepEqual(parseSuggestion("Add x\n\n```\nx();\n```", true), { summary: "Add x", body: "```\nx();\n```" });
});

test("medium until another effort is picked", () => {
  // settings.ts drops a stored level the preset doesn't take as it loads.
  assert.equal(effortOf("claude", {}), "medium");
  assert.equal(effortOf("pi", {}), "medium");
  assert.equal(effortOf("opencode", {}), "");
  assert.equal(effortOf("claude", { claude: "" }), "");
  assert.equal(effortOf("claude", { claude: "high" }), "high");
  assert.equal(effortOf("llm", { llm: "high" }), "");
  assert.deepEqual(effortLevels("llm"), []);
});

test("effort flags split into the argv the CLI expects", () => {
  // suggest.rs splits on whitespace without a shell: codex's override must stay one key=value word.
  const words = (preset: SuggestPreset, level: string) => effortArg(preset, level).split(/\s+/);
  assert.deepEqual(words("codex", "xhigh"), ["-c", "model_reasoning_effort=xhigh"]);
  assert.deepEqual(words("claude", "max"), ["--effort", "max"]);
  assert.deepEqual(words("pi", "off"), ["--thinking", "off"]);
  assert.deepEqual(words("opencode", "minimal"), ["--variant", "minimal"]);
  // A custom command carries its own flags, whatever was picked for the presets.
  assert.equal(commandLine("claude -p --effort low", {}, { claude: "max" }), "claude -p --effort low");
  // Effort without a model.
  assert.equal(commandLine("pi -p --no-tools --no-session", { pi: "" }, { pi: "xhigh" }), "pi -p --no-tools --no-session --thinking xhigh");
});

test("the model and effort a command line names", () => {
  assert.deepEqual(runDetails(commandLine("claude -p", {}, {})), ["claude-sonnet-5", "medium"]);
  assert.deepEqual(runDetails(commandLine("codex exec", { codex: "" }, { codex: "high" })), ["high"]);
  assert.deepEqual(runDetails("opencode run --agent plan -m a/b --variant max"), ["a/b", "max"]);
  assert.deepEqual(runDetails("pi -p --model=x --thinking off"), ["x", "off"]);
  assert.deepEqual(runDetails("~/bin/claude -p"), []);
  assert.deepEqual(runDetails("llm -m"), []);
});
