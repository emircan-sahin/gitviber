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
const isObject = (v: unknown): v is Record<string, unknown> => !!v && typeof v === "object" && !Array.isArray(v);

/** The JSON in an answer: all of it, a fenced block's, or what lies between its first `{` and last `}` (a "Here's the review:" before it). */
function jsonOf(answer: string): unknown {
  const fence = /^(`{3,}|~{3,})[^\n]*\n([\s\S]*?)\n\1\s*$/m.exec(answer);
  const open = answer.indexOf("{");
  const braces = open < 0 ? "" : answer.slice(open, answer.lastIndexOf("}") + 1);
  for (const candidate of [answer, fence?.[2], braces]) {
    if (!candidate) continue;
    try {
      return JSON.parse(candidate);
    } catch {
      // The next way of reading it.
    }
  }
  return null;
}

/**
 * A model's answer as a guide; null when it isn't the JSON asked for (the view then shows it
 * as Markdown). Fields it left out or mistyped read as empty, and a diagram it fenced anyway is
 * unwrapped.
 */
export function parseGuide(output: string): Guide | null {
  const v = jsonOf(output.replace(/\r\n?/g, "\n").trim());
  if (!isObject(v)) return null;
  const sections = (Array.isArray(v.sections) ? v.sections : []).filter(isObject).map((s) => ({
    title: text(s.title),
    summary: text(s.summary),
    files: (Array.isArray(s.files) ? s.files : []).map((f) => text(f).replace(/^\.\//, "")).filter(Boolean),
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
  const named = new Set(guide.sections.flatMap((s) => s.files));
  return paths.filter((p) => !named.has(p));
};
