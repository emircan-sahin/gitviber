import { isRecord } from "../storage.ts";

/**
 * Guided reviews: the user's agent CLI explains a commit or a branch as ordered sections, the
 * way a reviewer should read it (suggest.rs runs it, as for commit messages). Pure, so the
 * parser runs under node:test.
 */

/** Sent ahead of the commit's message or the branch's commits, and the diff; Settings shows it. */
export const GUIDE_PROMPT =
  'Explain this change to a reviewer and guide them through it. Answer with only a JSON object, with no code fences or other text, shaped like this: {"title": "what the change does, under 72 characters", "summary": "what changed and why, in a few sentences of Markdown", "diagram": "a Mermaid flowchart (flowchart TD) of the changed parts and how they connect: modules, components or functions as nodes with their labels in double quotes, new ones marked (new) and changed ones (changed); at most 15 nodes", "sections": [{"title": "one part of the change", "summary": "what it does and what to check, in Markdown", "files": ["the paths of its files, as the diff names them"], "risk": "what could break, if anything; leave it out otherwise"}]}. Order the sections the way a reviewer should read them, and put every changed file in one of them.';

export interface GuideSection {
  title: string;
  summary: string;
  files: string[];
  /** "" when the model saw none. */
  risk: string;
}

export interface Guide {
  title: string;
  summary: string;
  /** Mermaid source; "" for none. */
  diagram: string;
  sections: GuideSection[];
}

const text = (v: unknown) => (typeof v === "string" ? v.trim() : "");

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
 * The JSON in an answer: all of it, or the first `{…}` with a guide's fields, wherever it is (a
 * fence, a "Here's the review:" or a `{ retries }` in the words around it). One pass that pairs
 * the braces outside strings, so a brace in a string or a long answer costs no more.
 */
function jsonOf(answer: string): unknown {
  const whole = parse(answer);
  if (whole !== undefined) return whole;
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
      if (isRecord(v) && ("title" in v || "summary" in v || "sections" in v)) return v;
    }
  }
  return null;
}

/** The changed file (of `paths`) a model's path names: as written, or without git's a/ or b/. */
export function matchPath(path: string, paths: Set<string>) {
  if (paths.has(path)) return path;
  const bare = path.replace(/^[ab]\//, "");
  return paths.has(bare) ? bare : null;
}

/**
 * A model's answer as a guide; null when it isn't the JSON asked for (the view then shows it
 * as Markdown). Fields it left out or mistyped read as empty, and a diagram it fenced anyway is
 * unwrapped.
 */
export function parseGuide(output: string): Guide | null {
  const v = jsonOf(output.replace(/\r\n?/g, "\n").trim());
  if (!isRecord(v)) return null;
  const sections = (Array.isArray(v.sections) ? v.sections : []).filter(isRecord).map((s) => ({
    title: text(s.title),
    summary: text(s.summary),
    files: [...new Set((Array.isArray(s.files) ? s.files : []).map((f) => text(f).replace(/^\.\//, "")).filter(Boolean))],
    risk: text(s.risk),
  }));
  const guide = {
    title: text(v.title),
    summary: text(v.summary),
    diagram: text(v.diagram)
      .replace(/^(`{3,}|~{3,})\s*(mermaid)?\s*\n/i, "")
      .replace(/\n(`{3,}|~{3,})$/, "")
      .trim(),
    sections: sections.filter((s) => s.title || s.summary || s.files.length),
  };
  return guide.title || guide.summary || guide.sections.length ? guide : null;
}

/** The changed files (`paths`) no section names, so none is left out of the review. */
export const unplaced = (guide: Guide, paths: string[]) => {
  const all = new Set(paths);
  const named = new Set(guide.sections.flatMap((s) => s.files.map((f) => matchPath(f, all))));
  return paths.filter((p) => !named.has(p));
};
