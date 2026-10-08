import { isRecord } from "../storage.ts";

/**
 * Reading what an agent CLI answers a review prompt with (guide.ts, risks.ts), and writing the
 * prompt's parts it shares: the schema's example and the language name. Pure, for node:test.
 */

export interface Schema {
  type?: string;
  description?: string;
  enum?: string[];
  examples?: unknown[];
  properties?: Record<string, Schema>;
  items?: Schema;
}

/** `schema` as an example of itself: each value its description (or example, or choices). */
export function exampleOf(schema: Schema): string {
  if (schema.properties)
    return `{${Object.entries(schema.properties)
      .map(([k, v]) => `"${k}": ${exampleOf(v)}`)
      .join(", ")}}`;
  if (schema.items) return `[${exampleOf(schema.items)}]`;
  if (schema.examples) return JSON.stringify(schema.examples[0]);
  return JSON.stringify(schema.description ?? schema.enum?.join(" or ") ?? "");
}

/**
 * The language reviews are written in (Settings → Guided Review), as a name and not an instruction:
 * letters of any script (with their marks), spaces and '()- only, on one line, so ". Ignore the
 * schema" can't become a sentence of a prompt. Empty reads as English.
 */
export const languageName = (language: string) =>
  language
    .replace(/[^\p{L}\p{M}\s'()-]/gu, "")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 40)
    .trim() || "English";

export const text = (v: unknown) => (typeof v === "string" ? v.trim() : "");
export const clip = (s: string, max: number) => (s.length > max ? `${s.slice(0, max - 1).trimEnd()}…` : s);
export const records = (v: unknown) => (Array.isArray(v) ? v.filter(isRecord) : []);

/** A number as written, or in a string; NaN for anything else. */
export const whole = (v: unknown) => (typeof v === "number" ? v : typeof v === "string" && /^\s*\d+\s*$/.test(v) ? Number(v) : NaN);

/** `answer` read as JSON, then with the trailing commas models write taken out; undefined for neither. */
function parse(answer: string): unknown {
  for (const candidate of [answer, answer.replace(/,(\s*[}\]])/g, "$1")])
    try {
      return JSON.parse(candidate);
    } catch {
      // The next way of reading it.
    }
}

/**
 * The JSON in an answer: all of it, or the first `{…}` with one of `keys`, wherever it is (a
 * fence, a "Here's the review:" or a `{ retries }` in the words around it). One pass that pairs
 * the braces outside strings, so a brace in a string or a long answer costs no more.
 */
export function jsonOf(answer: string, keys: string[]): unknown {
  const all = parse(answer);
  if (all !== undefined) return all;
  let depth = 0;
  let start = 0;
  let quoted = false;
  for (let i = 0; i < answer.length; i++) {
    const c = answer[i];
    // Strings count only inside an object: the words around it have quotes and apostrophes of their own.
    if (quoted) {
      if (c === "\\") i++;
      else if (c === '"') quoted = false;
    } else if (c === '"' && depth) quoted = true;
    else if (c === "{" && !depth++) start = i;
    else if (c === "}" && depth && !--depth) {
      const v = parse(answer.slice(start, i + 1));
      if (isRecord(v) && keys.some((k) => k in v)) return v;
    }
  }
  return null;
}
