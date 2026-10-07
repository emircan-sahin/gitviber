import { isRecord } from "../storage.ts";

/**
 * Guided reviews: the user's agent CLI explains a commit or a branch as ordered sections, the
 * way a reviewer should read it (suggest.rs runs it, as for commit messages). Pure, so the
 * parser runs under node:test.
 */

/** Sent ahead of the commit's message or the branch's commits, and the diff; Settings shows it. */
export const GUIDE_PROMPT =
  'Explain this change to a reviewer and guide them through it. Answer with only a JSON object, with no code fences or other text, shaped like this: {"title": "what the change does, under 72 characters", "overview": "what changed and why in a short paragraph of Markdown, then a numbered list of the change\'s parts", "diagram": {"models": [{"name": "a type, table, schema or config the change adds or alters", "file": "its path", "status": "new, changed or same", "note": "what changed in it, in a few words", "section": 1, "fields": [{"name": "a field or member", "status": "new, changed or same", "note": "a few words, if it changed", "section": 1}]}], "flows": [{"title": "a path through the code the change adds or alters", "steps": [{"id": "a short id", "label": "the function or call, as code; for a decision, the question", "file": "its path", "status": "new, changed or same", "kind": "step, or decision for a branch", "section": 1}], "edges": [{"from": "a step id", "to": "a step id", "label": "yes or no after a decision; leave it out otherwise"}]}]}, "sections": [{"title": "one part of the change", "summary": "what it does and what to check, in Markdown", "files": ["the paths of its files, as the diff names them"], "risk": "what could break, if anything; leave it out otherwise"}]}. Each "section" is the number of the section that covers that part, 1 for the first. Keep the diagram small: at most 6 models of at most 12 fields, and at most 2 flows of at most 12 steps; leave out "models" or "flows" when the change has none worth drawing. Order the sections the way a reviewer should read them, and put every changed file in one of them.';

export interface GuideSection {
  title: string;
  summary: string;
  files: string[];
  /** "" when the model saw none. */
  risk: string;
}

export type PartStatus = "new" | "changed" | "same";

/** Where a diagram's part is explained: a section's number, from 1; null for none. */
type SectionRef = number | null;

export interface ModelField {
  name: string;
  note: string;
  status: PartStatus;
  section: SectionRef;
}

/** A data type, table or schema the change touches, drawn as a card of its fields. */
export interface Model {
  name: string;
  file: string;
  status: PartStatus;
  note: string;
  section: SectionRef;
  fields: ModelField[];
}

export interface FlowStep {
  /** The model's, for its edges. */
  id: string;
  label: string;
  file: string;
  status: PartStatus;
  kind: "step" | "decision";
  section: SectionRef;
}

export interface Flow {
  title: string;
  steps: FlowStep[];
  edges: { from: string; to: string; label: string }[];
}

export interface Guide {
  title: string;
  /** The overview: a paragraph and the change's parts, in Markdown. */
  summary: string;
  /** A guide written before diagrams were structured: its Mermaid source; "" for none. */
  diagram: string;
  models: Model[];
  flows: Flow[];
  sections: GuideSection[];
}

/** How much of a diagram is drawn, as the prompt asks (past this it stops explaining), and how many sections. */
export const LIMITS = { models: 6, fields: 12, flows: 2, steps: 12, edges: 24, sections: 50 };

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
      if (isRecord(v) && ("title" in v || "summary" in v || "overview" in v || "sections" in v)) return v;
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

const clip = (s: string, max: number) => (s.length > max ? `${s.slice(0, max - 1).trimEnd()}…` : s);
/** One line of text, as long as a diagram's box can hold. */
const line = (v: unknown, max: number) => clip(text(v).replace(/\s+/g, " "), max);
const records = (v: unknown) => (Array.isArray(v) ? v.filter(isRecord) : []);

function partStatus(v: unknown): PartStatus {
  const s = text(v).toLowerCase();
  return s === "new" || s === "added" ? "new" : s === "changed" || s === "modified" ? "changed" : "same";
}

/** A part's section number, when it names one of the `count` there are. */
function sectionRef(v: unknown, count: number): SectionRef {
  const n = typeof v === "number" ? v : typeof v === "string" && /^\s*\d+\s*$/.test(v) ? Number(v) : NaN;
  return Number.isInteger(n) && n >= 1 && n <= count ? n : null;
}

function parseModels(v: unknown, count: number): Model[] {
  return records(v)
    .map((m) => ({
      name: line(m.name, 80),
      file: line(m.file, 200).replace(/^\.\//, ""),
      status: partStatus(m.status),
      note: line(m.note, 120),
      section: sectionRef(m.section, count),
      fields: records(m.fields)
        .map((f) => ({ name: line(f.name, 80), note: line(f.note, 120), status: partStatus(f.status), section: sectionRef(f.section, count) }))
        .filter((f) => f.name)
        .slice(0, LIMITS.fields),
    }))
    .filter((m) => m.name)
    .slice(0, LIMITS.models);
}

/** Flows with steps; a step needs a label and an id of its own, an edge two steps of its flow. */
function parseFlows(v: unknown, count: number): Flow[] {
  return records(v)
    .map((f) => {
      const ids = new Set<string>();
      const steps: FlowStep[] = [];
      for (const [i, s] of records(f.steps).entries()) {
        const id = line(s.id, 80) || `#${i}`;
        const label = line(s.label, 100);
        if (!label || ids.has(id) || steps.length >= LIMITS.steps) continue;
        ids.add(id);
        steps.push({ id, label, file: line(s.file, 200).replace(/^\.\//, ""), status: partStatus(s.status), kind: text(s.kind).toLowerCase() === "decision" ? "decision" : "step", section: sectionRef(s.section, count) });
      }
      const seen = new Set<string>();
      const edges = records(f.edges)
        .map((e) => ({ from: line(e.from, 80), to: line(e.to, 80), label: line(e.label, 40) }))
        .filter((e) => {
          const key = `${e.from}\0${e.to}\0${e.label}`;
          if (!ids.has(e.from) || !ids.has(e.to) || seen.has(key)) return false;
          seen.add(key);
          return true;
        })
        .slice(0, LIMITS.edges);
      return { title: line(f.title, 100), steps, edges };
    })
    .filter((f) => f.steps.length)
    .slice(0, LIMITS.flows);
}

/**
 * A model's answer as a guide; null when it isn't the JSON asked for (the view then shows it
 * as Markdown). Fields it left out or mistyped read as empty, parts of a diagram it got wrong
 * are dropped and past LIMITS cut. A guide from before structured diagrams keeps its Mermaid
 * source (unwrapped if it fenced it anyway), and its summary stands for the overview.
 */
export function parseGuide(output: string): Guide | null {
  const v = jsonOf(output.replace(/\r\n?/g, "\n").trim());
  if (!isRecord(v)) return null;
  const sections = records(v.sections)
    .map((s) => ({
      title: text(s.title),
      summary: text(s.summary),
      files: [...new Set((Array.isArray(s.files) ? s.files : []).map((f) => text(f).replace(/^\.\//, "")).filter(Boolean))],
      risk: text(s.risk),
    }))
    .filter((s) => s.title || s.summary || s.files.length)
    .slice(0, LIMITS.sections);
  const diagram = isRecord(v.diagram) ? v.diagram : {};
  const guide = {
    title: text(v.title),
    summary: text(v.overview) || text(v.summary),
    diagram: text(v.diagram)
      .replace(/^(`{3,}|~{3,})\s*(mermaid)?\s*\n/i, "")
      .replace(/\n(`{3,}|~{3,})$/, "")
      .trim(),
    models: parseModels(diagram.models, sections.length),
    flows: parseFlows(diagram.flows, sections.length),
    sections,
  };
  return guide.title || guide.summary || guide.sections.length ? guide : null;
}

/**
 * Where each changed file (`paths`) shows: `shown`, per section, the ones whose diff it shows (a
 * file named twice shows in the first); `named`, per section, the others it names, with the
 * section showing it (`at`, from 1) or null when the diff doesn't have it; `rest`, the files no
 * section names, so none is left out of the review.
 */
export function placeFiles(guide: Guide, paths: string[]) {
  const all = new Set(paths);
  const at = new Map<string, number>();
  const shown: string[][] = [];
  const named: { path: string; at: number | null }[][] = [];
  guide.sections.forEach((s, i) => {
    shown.push([]);
    named.push([]);
    for (const f of s.files) {
      const path = matchPath(f, all);
      if (!path) named[i].push({ path: f, at: null });
      else if (!at.has(path)) {
        at.set(path, i + 1);
        shown[i].push(path);
      } else if (at.get(path) !== i + 1) named[i].push({ path, at: at.get(path)! });
    }
  });
  return { shown, named, rest: paths.filter((p) => !at.has(p)) };
}

const STATUS_WORDS: Record<PartStatus, string> = { new: "New", changed: "Changed", same: "Unchanged" };
const CLASSES: Record<PartStatus, string> = { new: "gNew", changed: "gChanged", same: "gSame" };
export const statusWord = (s: PartStatus) => STATUS_WORDS[s];
export const sectionBadge = (n: number) => String(n).padStart(2, "0");
const baseName = (path: string) => path.slice(path.lastIndexOf("/") + 1);

/**
 * A model's text as Mermaid label text: whatever could end the quoted string, start a directive
 * (`%%{`), math (`$$`), an entity or markup is written as a character code (`#60;`), which Mermaid
 * prints as that character. Newlines go, so nothing can start a statement (`click`, `style`).
 * Mermaid also reads its own syntax inside quotes: any line with "direction LR" (or TB…) as a
 * direction, dropping the step, and a "style…:…#…;" line loses its last ";". So ":" and the space
 * after "direction" are codes too. Bidi controls go: they'd reorder the label's text on screen.
 */
export const mermaidText = (s: string) =>
  s
    .replace(/[\u202A-\u202E\u2066-\u2069]/g, "")
    .replace(/\s+/g, " ")
    .replace(/[^\p{L}\p{N} .,_\-+*/=!?'()]/gu, (c) => `#${c.codePointAt(0)};`)
    .replace(/direction /g, "direction#32;");

/**
 * A flow as a Mermaid flowchart, left to right: each step a box (a hexagon for a decision) with
 * its section, status and file over its label; classed by status for the view's colors. Node ids
 * are the steps' places (`s0`…), never the model's ids, so the view can find each step's node.
 */
export function flowSource(flow: Flow) {
  const ids = new Map(flow.steps.map((s, i) => [s.id, `s${i}`]));
  const out = ["flowchart LR"];
  flow.steps.forEach((s, i) => {
    const decision = s.kind === "decision";
    const meta = [decision ? "Decision" : statusWord(s.status), baseName(s.file)].filter(Boolean).join(" · ");
    const badge = s.section ? `<span class='gf-badge'>${sectionBadge(s.section)}</span> ` : "";
    const label = `<span class='gf-meta'>${badge}${mermaidText(meta)}</span><br/><span class='${decision ? "gf-ask" : "gf-code"}'>${mermaidText(s.label)}</span>`;
    out.push(`  s${i}${decision ? `{{"${label}"}}` : `["${label}"]`}:::${CLASSES[s.status]}`);
  });
  for (const e of flow.edges) out.push(`  ${ids.get(e.from)} -->${e.label ? `|"${mermaidText(e.label)}"|` : ""} ${ids.get(e.to)}`);
  return out.join("\n");
}

/** A flow in words, a line a step, for screen readers and for when it can't be drawn. */
export function describeFlow(flow: Flow) {
  const label = new Map(flow.steps.map((s) => [s.id, s.label]));
  return flow.steps.map((s) => {
    const where = [s.kind === "decision" ? "decision" : statusWord(s.status).toLowerCase(), s.file && `in ${s.file}`].filter(Boolean).join(" ");
    const next = flow.edges.filter((e) => e.from === s.id).map((e) => (e.label ? `${e.label}: ${label.get(e.to)}` : label.get(e.to)));
    return `${s.label} (${where})${next.length ? `, then ${next.join("; ")}` : ""}`;
  });
}
