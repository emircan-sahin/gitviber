import { isRecord } from "../storage.ts";

/**
 * Guided reviews: the user's agent CLI explains a commit or a branch as ordered sections, the
 * way a reviewer should read it (suggest.rs runs it, as for commit messages). Pure, so the
 * parser runs under node:test.
 */

/** What a section is about, each with the line the prompt explains it by. */
export const CATEGORIES = {
  ui: "screens, components and styles",
  api: "endpoints, commands and the interfaces between parts",
  core: "the logic and state the rest relies on",
  data: "schemas, models, migrations and storage",
  cli: "command-line commands and flags",
  security: "auth, permissions, secrets, input checks and sandboxing",
  tests: "tests and their fixtures",
  docs: "docs, READMEs, changelogs and comments",
  examples: "examples and demos",
  deps: "dependencies and lockfiles",
  build: "build, packaging and CI",
  scripts: "development and maintenance scripts",
  config: "settings and configuration files",
  i18n: "translations and locale strings",
  assets: "images, fonts, icons and media",
  other: "anything else",
} as const;

export type Category = keyof typeof CATEGORIES;

/** How carefully a section wants reading, most first. */
export const IMPORTANCE = ["high", "medium", "low"] as const;
export type Importance = (typeof IMPORTANCE)[number];

const str = (description: string) => ({ type: "string", description });
const STATUS = { type: "string", enum: ["new", "changed", "same"] };
const SECTION = { type: "integer", description: "the number of the section that covers it, 1 for the first", examples: [1] };
const CRITICAL = { type: "boolean", description: "true only for security, data loss or a change that's hard to revert", examples: [false] };
const note = (extra: Record<string, object>, required: string[]) => ({
  type: "array",
  items: {
    type: "object",
    required: ["path", ...required, "text"],
    properties: { path: str("one of the section's files"), ...extra, text: str("what to know there, in a sentence"), critical: CRITICAL },
  },
});

/**
 * The answer's shape: sent as a JSON Schema to a CLI that enforces one (Claude Code's
 * --json-schema), and drawn from it as the example in GUIDE_PROMPT for the rest, so the two
 * can't drift. Only what every guide has is required; the parser takes any of the rest.
 */
export const GUIDE_SCHEMA = {
  type: "object",
  required: ["title", "overview", "sections"],
  properties: {
    title: str("what the change does, under 72 characters"),
    overview: str("why the change was made, in 1-2 sentences of Markdown; a numbered list of its parts after them only when it has 3 or more separate ones"),
    diagram: {
      type: "object",
      description: "only for a change to a real flow or data model; left out for style, text, config or docs changes",
      properties: {
        models: {
          type: "array",
          items: {
            type: "object",
            required: ["name"],
            properties: {
              name: str("a type, table, schema or config the change adds or alters"),
              file: str("its path"),
              status: STATUS,
              note: str("what changed in it, in a few words"),
              section: SECTION,
              fields: { type: "array", items: { type: "object", required: ["name"], properties: { name: str("a field or member"), status: STATUS, note: str("a few words, if it changed"), section: SECTION } } },
            },
          },
        },
        flows: {
          type: "array",
          items: {
            type: "object",
            required: ["steps"],
            properties: {
              title: str("a path through the code the change adds or alters"),
              steps: {
                type: "array",
                items: {
                  type: "object",
                  required: ["id", "label"],
                  properties: {
                    id: str("a short id"),
                    label: str("the function or call, as code; for a decision, the question"),
                    file: str("its path"),
                    status: STATUS,
                    kind: { type: "string", enum: ["step", "decision"] },
                    section: SECTION,
                  },
                },
              },
              edges: { type: "array", items: { type: "object", required: ["from", "to"], properties: { from: str("a step id"), to: str("a step id"), label: str("yes or no after a decision; left out otherwise") } } },
            },
          },
        },
      },
    },
    sections: {
      type: "array",
      items: {
        type: "object",
        required: ["title", "category", "summary", "files"],
        properties: {
          title: str("a label of at most 4 words"),
          category: { type: "string", enum: Object.keys(CATEGORIES), description: "one of the categories below" },
          summary: str("1-3 sentences of Markdown: why, and what isn't obvious from the diff"),
          files: { type: "array", items: str("a path, as the list of changed files names it") },
          check: str("one line on how to verify it, only when there's something specific to check"),
          risk: str("what could break, only when something really could"),
          importance: { type: "string", enum: [...IMPORTANCE], description: "high, medium or low" },
          fileNotes: note({}, []),
          lineNotes: note({ side: { type: "string", enum: ["new", "old"] }, line: { type: "integer", description: "the line's number on that side", examples: [12] } }, ["side", "line"]),
        },
      },
    },
  },
};

interface Schema {
  type?: string;
  description?: string;
  enum?: string[];
  examples?: unknown[];
  properties?: Record<string, Schema>;
  items?: Schema;
}

/** `schema` as an example of itself: each value its description (or example, or choices). */
function exampleOf(schema: Schema): string {
  if (schema.properties)
    return `{${Object.entries(schema.properties)
      .map(([k, v]) => `"${k}": ${exampleOf(v)}`)
      .join(", ")}}`;
  if (schema.items) return `[${exampleOf(schema.items)}]`;
  if (schema.examples) return JSON.stringify(schema.examples[0]);
  return JSON.stringify(schema.description ?? schema.enum?.join(" or ") ?? "");
}

/** Sent ahead of the commit's message or the branch's commits, the changed files and their diffs; Settings shows it. */
export const GUIDE_PROMPT = [
  `Explain this change to a reviewer so they can read it quickly. Answer with only a JSON object, with no code fences or other text, shaped like this: ${exampleOf(GUIDE_SCHEMA)}.`,
  "Size the guide to the change: one section for a small change (1-3 files, or under about 150 changed lines) unless it has truly separate concerns, and more only for separate concerns. Put mechanical edits (docs, changelog, formatting, renames) together in the last section, never in one of their own for a line. Order the sections the way a reviewer should read them, and put every path of the list of changed files in exactly one section.",
  "Write for a developer who scans: never restate what the diff shows (values, strings, numbers, styles); say why, and point at what isn't obvious. Most sections have no check or risk. Give importance sparingly, most sections medium or low: high only for security, data loss, or a change that breaks things, is hard to revert or is easy to get wrong; medium for a change in behavior worth a careful read; low for mechanical edits, docs or style. A note's critical is true only for security, data loss or a change that's hard to revert. Add a fileNote or lineNote only where it saves the reviewer real time, a few in the whole guide at most; a lineNote's side is new for an added or unchanged line and old for a removed one, and its line is the line's number on that side.",
  "Draw the diagram only when the change alters a real flow or data model, and leave it out for style, text, config or docs changes. Each of its parts' \"section\" is the number of the section that covers it, 1 for the first. Keep it small: at most 6 models of at most 12 fields, and at most 2 flows of at most 12 steps; leave out models or flows the change has none of.",
  `Categories: ${Object.entries(CATEGORIES)
    .map(([k, v]) => `${k} (${v})`)
    .join(", ")}.`,
].join("\n\n");

/** GUIDE_PROMPT with the language the guide's prose is written in (Settings → Guided Review); empty reads as English. */
export function guidePrompt(language: string) {
  // One line of the user's own text: a newline in it can't start a paragraph of its own.
  const lang = language.replace(/\s+/g, " ").trim().slice(0, 40) || "English";
  return `${GUIDE_PROMPT}\n\nWrite all prose (the title, overview, summaries, checks, risks and notes) in ${lang}; keep the JSON keys and the values picked from a list (category, importance, side, status, kind), code, identifiers, paths and quoted strings as they are.`;
}

export interface FileNote {
  /** As the section names it. */
  path: string;
  text: string;
  critical: boolean;
}

export interface LineNote extends FileNote {
  side: "new" | "old";
  line: number;
}

export interface GuideSection {
  title: string;
  /** "other" for a guide written before categories. */
  category: Category;
  summary: string;
  files: string[];
  /** How to verify it, one line; "" for none. */
  check: string;
  /** "" when the model saw none. */
  risk: string;
  /** High: review carefully (security, data loss, hard to revert). */
  importance: Importance;
  fileNotes: FileNote[];
  lineNotes: LineNote[];
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

/** How much of a diagram is drawn, as the prompt asks (past this it stops explaining), how many sections, and their notes. */
export const LIMITS = { models: 6, fields: 12, flows: 2, steps: 12, edges: 24, sections: 50, notes: 40, note: 500 };

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

/** A number as written, or in a string; NaN for anything else. */
const whole = (v: unknown) => (typeof v === "number" ? v : typeof v === "string" && /^\s*\d+\s*$/.test(v) ? Number(v) : NaN);

/** A part's section number, when it names one of the `count` there are. */
function sectionRef(v: unknown, count: number): SectionRef {
  const n = whole(v);
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

const flag = (v: unknown) => v === true || text(v).toLowerCase() === "true";
/** As the model gave it; a guide from before importance was asked: high when it was critical, else medium, the neutral one. */
const importance = (v: unknown, critical: unknown): Importance => {
  const i = text(v).toLowerCase();
  return (IMPORTANCE as readonly string[]).includes(i) ? (i as Importance) : flag(critical) ? "high" : "medium";
};
const category = (v: unknown): Category => {
  const c = text(v).toLowerCase();
  return Object.hasOwn(CATEGORIES, c) ? (c as Category) : "other";
};

/** A section, with notes only on its own files and lines that can be, at most LIMITS.notes of them. */
function parseSection(s: Record<string, unknown>): GuideSection {
  const files = [...new Set((Array.isArray(s.files) ? s.files : []).map((f) => text(f).replace(/^\.\//, "")).filter(Boolean))];
  const own = new Set(files);
  const note = (n: Record<string, unknown>) => ({ path: matchPath(text(n.path).replace(/^\.\//, ""), own) ?? "", text: clip(text(n.text), LIMITS.note), critical: flag(n.critical) });
  const fileNotes = records(s.fileNotes)
    .map(note)
    .filter((n) => n.path && n.text)
    .slice(0, LIMITS.notes);
  const lineNotes = records(s.lineNotes)
    .map((n) => ({ ...note(n), side: text(n.side).toLowerCase() === "old" ? ("old" as const) : ("new" as const), line: whole(n.line) }))
    .filter((n) => n.path && n.text && Number.isSafeInteger(n.line) && n.line > 0)
    .slice(0, LIMITS.notes - fileNotes.length);
  return { title: text(s.title), category: category(s.category), summary: text(s.summary), files, check: clip(text(s.check).replace(/\s+/g, " "), LIMITS.note), risk: text(s.risk), importance: importance(s.importance, s.critical), fileNotes, lineNotes };
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
    .map(parseSection)
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
