import { clip, exampleOf, jsonOf, languageName, records, text, whole } from "./guide.ts";

/**
 * Find Risks: the user's agent CLI hunts a change for bugs and lists them most severe first, beside
 * its guided review (the same range, suggest.rs runs it). Pure, so the parser runs under node:test.
 */

export const SEVERITY = ["high", "medium", "low"] as const;
export type Severity = (typeof SEVERITY)[number];

/** As many as the prompt asks for; past it, the list stops being a short list. */
export const MAX_RISKS = 8;
const MAX_TEXT = 500;

const str = (description: string) => ({ type: "string", description });

/** The answer's shape: Claude Code's --json-schema, and the example in RISKS_PROMPT for the rest. */
export const RISKS_SCHEMA = {
  type: "object",
  required: ["risks"],
  properties: {
    risks: {
      type: "array",
      description: "most severe first; empty when there's no real risk",
      items: {
        type: "object",
        required: ["title", "severity", "path", "why"],
        properties: {
          title: str("the bug, in at most 8 words"),
          severity: { type: "string", enum: [...SEVERITY], description: "high, medium or low" },
          path: str("the changed file it's in, as the list of changed files names it"),
          line: { type: "integer", description: "the line's number on that side, when it's at one line", examples: [12] },
          side: { type: "string", enum: ["new", "old"] },
          why: str("what goes wrong and when, in 1-2 sentences"),
          check: str("how to confirm it, in one line, when there's a quick way"),
        },
      },
    },
  },
};

/** Sent ahead of the change, as GUIDE_PROMPT is; Settings shows it. */
export const RISKS_PROMPT = [
  `Hunt this change for bugs a reviewer would want to know about before merging. Answer with only a JSON object, with no code fences or other text, shaped like this: ${exampleOf(RISKS_SCHEMA)}.`,
  `List only concrete problems the change brings in or leaves: wrong logic, unhandled edge cases (empty, null, zero, very large, concurrent), races, broken error handling, security holes, data loss, resource leaks, misuse of an API, and callers the change breaks. Never list style, naming, formatting, missing comments or tests, or a guess you can't tie to the code. Each risk names the changed file and, when it sits at one line, that line's number: side new for an added or unchanged line, old for a removed one. Order them most severe first, at most ${MAX_RISKS}; high only for security, data loss or a likely crash or wrong result, low for an unlikely corner. An empty list is the right answer for a change without real risks.`,
].join("\n\n");

/** RISKS_PROMPT with the language the risks are written in (Settings → Guided Review). */
export const risksPrompt = (language: string) =>
  `${RISKS_PROMPT}\n\nWrite the prose (title, why, check) in ${languageName(language)}; keep the JSON keys, the values picked from a list (severity, side), code, identifiers and paths as they are.`;

export interface Risk {
  title: string;
  severity: Severity;
  path: string;
  /** Null when it isn't at one line. */
  line: number | null;
  side: "new" | "old";
  why: string;
  /** "" for none. */
  check: string;
}

const severity = (v: unknown): Severity => {
  const s = text(v).toLowerCase();
  return (SEVERITY as readonly string[]).includes(s) ? (s as Severity) : "medium";
};

/**
 * A model's answer as its risks, most severe first (the model's order within each), at most
 * MAX_RISKS; null when it isn't the JSON asked for. A risk needs a title or a why.
 */
export function parseRisks(output: string): Risk[] | null {
  const v = jsonOf(output.replace(/\r\n?/g, "\n").trim(), ["risks"]);
  const list = Array.isArray(v) ? v : v && typeof v === "object" && "risks" in v ? (v as { risks: unknown }).risks : undefined;
  if (!Array.isArray(list)) return null;
  const risks = records(list)
    .map((r): Risk => {
      const line = whole(r.line);
      return {
        title: clip(text(r.title).replace(/\s+/g, " "), 120),
        severity: severity(r.severity),
        path: text(r.path).replace(/^\.\//, ""),
        line: Number.isSafeInteger(line) && line > 0 ? line : null,
        side: text(r.side).toLowerCase() === "old" ? "old" : "new",
        why: clip(text(r.why), MAX_TEXT),
        check: clip(text(r.check).replace(/\s+/g, " "), MAX_TEXT),
      };
    })
    .filter((r) => r.title || r.why);
  // A stable sort: the model's own order stays within a severity.
  return risks.sort((a, b) => SEVERITY.indexOf(a.severity) - SEVERITY.indexOf(b.severity)).slice(0, MAX_RISKS);
}
