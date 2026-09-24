/**
 * Commit message suggestions from the user's own agent CLI (suggest.rs runs it). Pure, so
 * the parser runs under node:test.
 */

/**
 * `model` is only the default: new models come out every few months, so Settings takes any id
 * and links `models`, a list of current ones.
 * `other` ones sit under Others. Each reads the prompt and the diff from stdin (tried by hand);
 * Gemini CLI (individual sign-in retired), Copilot CLI (ignores stdin) and Ollama (pulls a
 * mistyped model unasked) are left to Custom.
 */
export const SUGGEST_PRESETS = {
  claude: {
    label: "Claude Code",
    command: "claude -p",
    modelFlag: "--model",
    model: "claude-sonnet-5",
    models: { label: "Anthropic's model list", url: "https://platform.claude.com/docs/en/about-claude/models/overview" },
    other: false,
  },
  codex: {
    label: "Codex",
    command: "codex exec",
    modelFlag: "-m",
    model: "gpt-6-luna",
    models: { label: "OpenAI's Codex models", url: "https://developers.openai.com/codex/models" },
    other: false,
  },
  // The plan agent can't edit files; the default build agent can.
  opencode: {
    label: "opencode",
    command: "opencode run --agent plan",
    modelFlag: "-m",
    model: "anthropic/claude-sonnet-5",
    models: { label: "models.dev", url: "https://models.dev" },
    other: true,
  },
  pi: {
    label: "pi",
    command: "pi -p --no-tools --no-session",
    modelFlag: "--model",
    model: "anthropic/claude-sonnet-5",
    models: { label: "pi's model list", url: "https://pi.dev/models" },
    other: true,
  },
  llm: {
    label: "llm",
    command: "llm",
    modelFlag: "-m",
    model: "gpt-6-luna",
    models: { label: "llm models", url: "https://llm.datasette.io/en/stable/usage.html#listing-available-models" },
    other: true,
  },
} as const;

export type SuggestPreset = keyof typeof SUGGEST_PRESETS;

/** Every provider's current model ids on one page, for a Custom command. */
export const ALL_MODELS = { label: "models.dev", url: "https://models.dev" };

export const presetOf = (command: string) => (Object.keys(SUGGEST_PRESETS) as SuggestPreset[]).find((k) => SUGGEST_PRESETS[k].command === command.trim());

/** The model a preset runs with: the user's id, or the default until they type one. "" is the CLI's own default. */
export const modelOf = (preset: SuggestPreset, models: Partial<Record<SuggestPreset, string>>) => (models[preset] ?? SUGGEST_PRESETS[preset].model).trim();

/** The command as run: a preset's with its model flag; a custom one carries its model itself. */
export function commandLine(command: string, models: Partial<Record<SuggestPreset, string>>) {
  const preset = presetOf(command);
  const model = preset ? modelOf(preset, models) : "";
  return preset && model ? `${command.trim()} ${SUGGEST_PRESETS[preset].modelFlag} ${model}` : command;
}

/** Sent ahead of the diff, word for word; Settings shows it. */
export const SUGGEST_PROMPT =
  "Write a git commit message for this diff: a summary line under 72 characters, a blank line, then a short body saying what changed and why. Output only the commit message, with no quotes or code fences.";

/** suggest.rs MAX_DIFF. */
export const SUGGEST_LIMIT_KB = 100;

/** The program a template runs, for messages ("claude" from "claude -p"). */
export const programOf = (command: string) => command.trim().split(/\s+/)[0] ?? "";

/**
 * A model's answer as summary and description. Models wrap it anyway at times: a code fence,
 * quotes, a "Subject:" label or Markdown emphasis on the first line. Null when it's empty.
 */
export function parseSuggestion(output: string): { summary: string; body: string } | null {
  let text = output.replace(/\r\n?/g, "\n").trim();
  // A fenced block anywhere ("Here's a message:\n```\n…\n```") is the message.
  const fence = /^(`{3,}|~{3,})[^\n]*\n([\s\S]*?)\n\1\s*$/m.exec(text);
  if (fence) text = fence[2].trim();
  const quoted = /^(["'`])([\s\S]*)\1$/.exec(text);
  if (quoted && !quoted[2].includes(quoted[1])) text = quoted[2].trim();
  const [first = "", ...rest] = text.split("\n");
  const summary = first
    .replace(/^#+\s+/, "")
    .replace(/^(\*\*|__)(.*)\1$/, "$2")
    .replace(/^(summary|subject|title|commit message):\s*/i, "")
    .trim();
  const body = rest
    .join("\n")
    .trim()
    .replace(/^(body|description):\s*/i, "");
  return summary ? { summary, body } : null;
}
