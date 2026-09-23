/**
 * Commit message suggestions from the user's own agent CLI (suggest.rs runs it). Pure, so
 * the parser runs under node:test.
 */

export const SUGGEST_PRESETS = {
  claude: { label: "Claude Code", command: "claude -p" },
  codex: { label: "Codex", command: "codex exec" },
} as const;

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
