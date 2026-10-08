/**
 * Commit messages and pull request descriptions from the user's own agent CLI (suggest.rs runs
 * it). Pure, so the parser runs under node:test.
 */

/**
 * `model` is only the default: new models come out every few months, so Settings takes any id
 * and links `models`, a list of current ones. `effort` is the reasoning effort flag and its levels
 * (from each CLI's --help) with the level a preset runs at until the user picks one (CLI default
 * passes none); null where the CLI has no general one. `lean`: flags every run gets, for a faster
 * start, with the tools it may use (`tools`: for a guided review, which reads a patch file; `none`:
 * for the rest). `other` ones sit under Others. Each reads the prompt and the diff from stdin (tried by hand);
 * Gemini CLI (individual sign-in retired), Copilot CLI (ignores stdin) and Ollama (pulls a
 * mistyped model unasked) are left to Custom.
 */
export const SUGGEST_PRESETS = {
  claude: {
    label: "Claude Code",
    command: "claude -p",
    modelFlag: "--model",
    // The alias follows the newest Sonnet; a pinned id ("claude-sonnet-5") stayed on the older one.
    model: "sonnet",
    models: { label: "Anthropic's model list", url: "https://platform.claude.com/docs/en/about-claude/models/overview" },
    effort: { flag: "--effort", levels: ["low", "medium", "high", "xhigh", "max"], default: "medium" },
    // Measured on a 5-line commit: 50 s and 6 turns with the user's MCP servers, skills and xhigh
    // effort; 8 s and 1 turn with these and medium. Read is all a guided review needs.
    lean: { flags: "--strict-mcp-config --no-session-persistence --disable-slash-commands", tools: "--tools Read,Grep,Glob", none: '--tools ""' },
    other: false,
  },
  codex: {
    label: "Codex",
    command: "codex exec",
    modelFlag: "-m",
    model: "gpt-6-luna",
    models: { label: "OpenAI's Codex models", url: "https://developers.openai.com/codex/models" },
    // A config override, one argument: suggest.rs runs argv without a shell.
    effort: { flag: "-c model_reasoning_effort=", levels: ["minimal", "low", "medium", "high", "xhigh"], default: "medium" },
    other: false,
    // Its bare program is an interactive session of its own (Ask Agent opens it).
    session: true,
  },
  // The plan agent can't edit files; the default build agent can.
  opencode: {
    label: "opencode",
    command: "opencode run --agent plan",
    modelFlag: "-m",
    model: "anthropic/claude-sonnet-5",
    models: { label: "models.dev", url: "https://models.dev" },
    effort: { flag: "--variant", levels: ["minimal", "low", "medium", "high", "xhigh", "max"], default: "" },
    other: true,
    session: true,
  },
  pi: {
    label: "pi",
    command: "pi -p --no-tools --no-session",
    modelFlag: "--model",
    model: "anthropic/claude-sonnet-5",
    models: { label: "pi's model list", url: "https://pi.dev/models" },
    effort: { flag: "--thinking", levels: ["off", "minimal", "low", "medium", "high", "xhigh", "max"], default: "medium" },
    other: true,
    session: true,
  },
  llm: {
    label: "llm",
    command: "llm",
    modelFlag: "-m",
    model: "gpt-6-luna",
    models: { label: "llm models", url: "https://llm.datasette.io/en/stable/usage.html#listing-available-models" },
    // Only `-o reasoning_effort` for some OpenAI models.
    effort: null,
    other: true,
  },
} as const;

export type SuggestPreset = keyof typeof SUGGEST_PRESETS;
/** A value per preset, as Settings keeps models and efforts. */
type Models = Partial<Record<SuggestPreset, string>>;

/** Every provider's current model ids on one page, for a Custom command. */
export const ALL_MODELS = { label: "models.dev", url: "https://models.dev" };

export const presetOf = (command: string) => (Object.keys(SUGGEST_PRESETS) as SuggestPreset[]).find((k) => SUGGEST_PRESETS[k].command === command.trim());

/** The model a preset runs with: the user's id, or the default until they type one. "" is the CLI's own default. */
export const modelOf = (preset: SuggestPreset, models: Models) => (models[preset] ?? SUGGEST_PRESETS[preset].model).trim();

/** The levels a preset's effort takes; none when it has no flag. */
export const effortLevels = (preset: SuggestPreset): readonly string[] => SUGGEST_PRESETS[preset].effort?.levels ?? [];

/** The effort a preset runs with (settings.ts checks it's one of its levels): the user's, or its default until they pick one; "" is the CLI's own default. */
export const effortOf = (preset: SuggestPreset, efforts: Models) => {
  const effort = SUGGEST_PRESETS[preset].effort;
  return effort ? (efforts[preset] ?? effort.default) : "";
};

/** The flags a preset adds to every run (`reads`: a guided review's, which may read files); "" for none. */
export function leanFlags(preset: SuggestPreset, reads: boolean) {
  const p = SUGGEST_PRESETS[preset];
  return "lean" in p ? `${p.lean.flags} ${reads ? p.lean.tools : p.lean.none}` : "";
}

/** `--effort high`, or `-c model_reasoning_effort=high` for a flag that takes its value joined. */
export function effortArg(preset: SuggestPreset, level: string) {
  const flag = SUGGEST_PRESETS[preset].effort?.flag ?? "";
  return flag.endsWith("=") ? flag + level : `${flag} ${level}`;
}

/** The command as run: a preset's with its lean (`reads`: a guided review's), model and effort flags; a custom one carries its own. */
export function commandLine(command: string, models: Models, efforts: Models, reads = false, lean = true) {
  const preset = presetOf(command);
  if (!preset) return command;
  const model = modelOf(preset, models);
  const effort = effortOf(preset, efforts);
  return [command.trim(), lean && leanFlags(preset, reads), model && `${SUGGEST_PRESETS[preset].modelFlag} ${model}`, effort && effortArg(preset, effort)].filter(Boolean).join(" ");
}

/** What a guided review runs: its own command, models and efforts, or Commit Messages' where it has none. */
export function reviewAgent(s: { suggestCommand: string; suggestModels: Models; suggestEfforts: Models; reviewCommand: string | null; reviewModels: Models; reviewEfforts: Models }) {
  return { command: s.reviewCommand ?? s.suggestCommand, models: { ...s.suggestModels, ...s.reviewModels }, efforts: { ...s.suggestEfforts, ...s.reviewEfforts } };
}

/**
 * `ask` with the command line as run; when a Claude Code preset's run fails on a flag its CLI is
 * too old to know (commander's "unknown option '--…'"), once more without the lean flags
 * (`lean` false: leave out other new flags too), keeping the model and effort. `old`: it took
 * the second try.
 */
export async function withLeanFallback<T>(
  command: string,
  models: Models,
  efforts: Models,
  reads: boolean,
  ask: (line: string, lean: boolean) => Promise<T>,
): Promise<{ value: T; old: boolean }> {
  try {
    return { value: await ask(commandLine(command, models, efforts, reads), true), old: false };
  } catch (e) {
    if (presetOf(command) !== "claude" || !/unknown option '--/.test(String(e))) throw e;
    return { value: await ask(commandLine(command, models, efforts, reads, false), false), old: true };
  }
}

/** The model and effort a command line names, as each preset's flags (or `--model=x`) write them, for "written by claude (x, medium)". */
export function runDetails(line: string) {
  const words = line.trim().split(/\s+/);
  const value = (flags: string[]) => {
    for (const [i, w] of words.entries()) {
      if (flags.includes(w) && words[i + 1] && !words[i + 1].startsWith("-")) return words[i + 1];
      const joined = flags.find((f) => f.startsWith("--") && w.startsWith(`${f}=`));
      if (joined) return w.slice(joined.length + 1);
      if (w.startsWith("model_reasoning_effort=") && flags.includes("--effort")) return w.slice("model_reasoning_effort=".length);
    }
    return "";
  };
  return [value(["--model", "-m"]), value(["--effort", "--thinking", "--variant"])].filter(Boolean);
}

/** Sent ahead of the diff, word for word; Settings shows it. */
export const SUGGEST_PROMPT =
  "Write a git commit message for this diff: a summary line under 72 characters, a blank line, then a short body saying what changed and why. Output only the commit message, with no quotes or code fences.";

/** Sent ahead of the branch's commits, its PR template if any, and its diff; Settings shows it. */
export const PULL_PROMPT =
  "Write a GitHub pull request title and description for this branch: a title under 72 characters on the first line, a blank line, then a description in Markdown of what changed and why. Output only the title and the description, with no quotes or code fences around them.";

/** suggest.rs MAX_DIFF. */
export const SUGGEST_LIMIT_KB = 100;
/** suggest.rs MAX_GUIDE_INPUT. */
export const GUIDE_LIMIT_KB = 400;

/** The program a template runs, by name, for messages ("claude" from "~/.local/bin/claude -p"). */
export const programOf = (command: string) => command.trim().split(/\s+/)[0]?.split("/").at(-1) ?? "";

/**
 * A Markdown answer inside a fence that wraps it: the first fence (after at most a "Here's the PR:"
 * line) to the last, words after it allowed. One after the title, or marked with a language, is
 * the description's own code.
 */
function unwrapMarkdown(text: string) {
  const lines = text.split("\n");
  const open = lines.findIndex((l) => /^(`{3,}|~{3,})/.test(l));
  if (open < 0) return text;
  const [, fence, info] = /^(`{3,}|~{3,})\s*(\S*)/.exec(lines[open])!;
  const preface = lines.slice(0, open).join("\n").trim();
  if (!["", "markdown", "md", "text"].includes(info.toLowerCase()) || (preface && (preface.includes("\n") || !preface.endsWith(":")))) return text;
  const close = lines.map((l) => l.trim()).lastIndexOf(fence);
  return close > open ? lines.slice(open + 1, close).join("\n").trim() : text;
}

/**
 * A model's answer as summary and description. Models wrap it anyway at times: a code fence,
 * quotes, a "Subject:" label or Markdown emphasis on the first line. Null when it's empty.
 * A `markdown` description has code blocks of its own, which stay.
 */
export function parseSuggestion(output: string, markdown = false): { summary: string; body: string } | null {
  let text = output.replace(/\r\n?/g, "\n").trim();
  if (markdown) text = unwrapMarkdown(text);
  else {
    // A fenced block anywhere ("Here's a message:\n```\n…\n```") is the message.
    const fence = /^(`{3,}|~{3,})[^\n]*\n([\s\S]*?)\n\1\s*$/m.exec(text);
    if (fence) text = fence[2].trim();
  }
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
