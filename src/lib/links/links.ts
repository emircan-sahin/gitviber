/**
 * Paths and URLs that ⌘-click (Ctrl off macOS) opens in the terminal, and the paths it opens in the code view (whose
 * names go to their definitions, lib/editor/definitions): found a line at a time (only the line under the
 * pointer is asked for), and resolved against the repo's files or a commit's tree. Pure, so it
 * runs under node:test.
 */

import { basename, dirname, slashes } from "../path.ts";

/** How a link's text becomes a target: each kind has its own lookup rules. */
type LinkKind = "module" | "style" | "doc" | "path" | "rust" | "python" | "url" | "file";

/** What a link opens: a web page, or a repo file, at a line (1-based) when the link names one. */
export type Target = { url: string } | { path: string; line?: number; column?: number };

/** A link in a line: `[start, end)` are offsets in the line, `spec` the text between them. */
export interface Link {
  start: number;
  end: number;
  spec: string;
  kind: LinkKind;
}

/** A tsconfig `paths` entry, its targets relative to the repo root. */
export interface Alias {
  pattern: string;
  targets: string[];
}

/** The files links may point to, and the folders' tsconfig / jsconfig files. */
export interface FileIndex {
  files: ReadonlySet<string>;
  configs: ReadonlyMap<string, string[]>;
}

const JS = new Set(["javascript", "typescript", "jsx", "tsx", "vue", "svelte", "astro", "mdx"]);
const CSS = new Set(["css", "scss", "sass", "less", "postcss"]);
const DOCS = new Set(["markdown", "mdx"]);

type Pattern = [RegExp, LinkKind];

// Each regex's last group is the link. `d` gives the groups' offsets.
const JS_PATTERNS: Pattern[] = [
  // import … from "x", export … from "x", import "x"; a multi-line import ends in `} from "x"`.
  [/\b(?:from|import)\s*(["'])([^"'\s]+)\1/dg, "module"],
  [/\b(?:import|require)\s*\(\s*(["'])([^"'\s]+)\1/dg, "module"],
  // Any other relative path in a string (vi.mock("./x"), new URL("./x", …)).
  [/(["'`])(\.\.?\/[^"'`\s]*)\1/dg, "module"],
];
const CSS_PATTERNS: Pattern[] = [
  [/@(?:import|use|forward)\s+(["'])([^"']+)\1/dg, "style"],
  [/\burl\(\s*(["']?)([^"')\s]+)\1\s*\)/dg, "style"],
];
const DOC_PATTERNS: Pattern[] = [
  // [text](x "title"), ![alt](<x>)
  [/\]\(\s*<?([^)\s>]+)/dg, "doc"],
  // [ref]: x
  [/^\s{0,3}\[[^\]]+\]:\s*<?([^\s>]+)/dg, "doc"],
  // Inline HTML: <img src="x">, <a href="x">
  [/\b(?:src|href)\s*=\s*(["'])([^"']+)\1/dg, "doc"],
];
const RUST_PATTERNS: Pattern[] = [[/^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(?:r#)?([A-Za-z_]\w*)\s*;/dg, "rust"]];
// Dots, then a name that starts with a letter: `\.+[\w.]*` backtracks over a long run of dots.
const PYTHON_PATTERNS: Pattern[] = [[/^\s*from\s+(\.+(?:[A-Za-z_][\w.]*)?|[A-Za-z_][\w.]*)\s+import\b/dg, "python"]];
const PATH_PATTERNS: Pattern[] = [[/(["'`])(\.\.?\/[^"'`\s]*)\1/dg, "path"]];

function patternsFor(lang: string): Pattern[] {
  const own = JS.has(lang) ? JS_PATTERNS : CSS.has(lang) ? CSS_PATTERNS : lang === "rust" ? RUST_PATTERNS : lang === "python" ? PYTHON_PATTERNS : [];
  // MDX is both: its imports, and markdown links.
  return [...own, ...(DOCS.has(lang) ? DOC_PATTERNS : []), ...PATH_PATTERNS];
}

type Add = (start: number, spec: string, kind: LinkKind) => void;

/** Collects links, kept in order; the first to claim some text keeps it. */
function collector() {
  const out: Link[] = [];
  const add: Add = (start, spec, kind) => {
    const end = start + spec.length;
    if (!spec) return;
    // The first link starting at or after `start`; only it and the one before can overlap.
    let lo = 0;
    for (let hi = out.length; lo < hi; ) {
      const mid = (lo + hi) >> 1;
      if (out[mid].start < start) lo = mid + 1;
      else hi = mid;
    }
    if ((lo > 0 && out[lo - 1].end > start) || (lo < out.length && out[lo].start < end)) return;
    out.splice(lo, 0, { start, end, spec, kind });
  };
  return { add, links: out };
}

/** Where a lookup looks: `[start, end)` offsets in the line, the pointer's or the cursor's. */
export interface Near {
  start: number;
  end: number;
}

// Characters either side of `near` that are read. A minified bundle is one line of a megabyte, and
// terminal output wraps into one: reading all of it on every pointer move froze the UI.
export const LINK_WINDOW = 2000;

/**
 * Runs `find` over the part of `line` around `near` (all of it without one), with offsets in `line`.
 * `find` is told whether the part starts the line, for patterns anchored there. A link the window
 * cuts isn't one: its tail could name another file.
 */
function windowed(line: string, near: Near | undefined, find: (text: string, whole: boolean, add: Add) => void): Link[] {
  const from = near ? Math.max(0, near.start - LINK_WINDOW) : 0;
  const to = near ? Math.min(line.length, near.end + LINK_WINDOW) : line.length;
  const { add, links } = collector();
  find(from || to < line.length ? line.slice(from, to) : line, from === 0, add);
  return links.filter((l) => (from === 0 || l.start > 0) && (to === line.length || l.end < to - from)).map((l) => ({ ...l, start: l.start + from, end: l.end + from }));
}

/** The links in one line of a `lang` file (a Shiki id), in order; with `near`, only those around it. */
export function findLinks(line: string, lang: string, near?: Near): Link[] {
  return windowed(line, near, (text, whole, add) => {
    // First: a URL in an import or a markdown link is still a URL.
    urls(text, add);
    for (const [re, kind] of patternsFor(lang)) {
      if (!whole && re.source.startsWith("^")) continue;
      for (const m of text.matchAll(re)) {
        const g = m.length - 1;
        add(m.indices![g]![0], m[g], kind);
      }
    }
    if (lang === "python" && whole) pythonImports(text, add);
    // src/lib/api.ts:42 in a comment or a doc. A bare name.ext only with a line, or `a.b` would be one.
    filePaths(text, add, lang === "markdown" || lang === "text");
  });
}

/** The links in a line of terminal output: URLs, and paths with or without a line (`ls` prints bare names). */
export function findTerminalLinks(line: string, near?: Near): Link[] {
  return windowed(line, near, (text, _, add) => {
    urls(text, add);
    filePaths(text, add, true);
  });
}

// Left off a URL's end: the sentence around it.
const TRAILING = new Set([".", ",", ";", ":", "!", "?", "*", "'", '"']);

function urls(line: string, add: Add) {
  for (const m of line.matchAll(/\bhttps?:\/\/[^\s"'`<>{}|\\^]+/gi)) {
    const url = m[0];
    // A closing bracket belongs to the URL only if it opened one: (see https://x.com/a_(b)).
    const count = (c: string) => url.split(c).length - 1;
    let [parens, brackets] = [count(")") - count("("), count("]") - count("[")];
    let end = url.length;
    for (; end > 0; end--) {
      const c = url[end - 1];
      if (c === ")" && parens > 0) parens--;
      else if (c === "]" && brackets > 0) brackets--;
      else if (!TRAILING.has(c)) break;
    }
    add(m.index, url.slice(0, end), "url");
  }
}

// A path, then maybe where in it: `:12`, `:12:5`, `(12,5)` (tsc), `#L12`, `#L12C5`, `#L12-L20` (GitHub).
// Windows too: a drive letter, and backslashes.
const FILE = /(?<![\w./\\@~+-])(?:[A-Za-z]:[\\/]|\.{1,2}[\\/]|[\\/])?[\w@+-][\w@.+-]*(?:[\\/][\w@.+-]+)*(?::\d+(?::\d+)?|\(\d+(?:,\s*\d+)?\)|#L\d+(?:C\d+)?(?:-L?\d+(?:C\d+)?)?)?/g;
const POSITION = /(?::(\d+)(?::(\d+))?|\((\d+)(?:,\s*(\d+))?\)|#L(\d+)(?:C(\d+))?(?:-L?\d+(?:C\d+)?)?)$/;

function filePaths(line: string, add: Add, bareNames: boolean) {
  for (const m of line.matchAll(FILE)) {
    const at = POSITION.exec(m[0]);
    let path = at ? m[0].slice(0, at.index) : m[0];
    // A sentence's full stop isn't the extension's (a loop: /\.+$/ backtracks over a run of dots).
    let end = path.length;
    while (end > 0 && path[end - 1] === ".") end--;
    path = path.slice(0, end);
    const slash = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
    if (!/\.[A-Za-z][\w-]*$/.test(path.slice(slash + 1)) && !at) continue;
    if (slash < 0 && !at && !bareNames) continue;
    add(m.index, at && path.length === at.index ? m[0] : path, "file");
  }
}

/** A path and the line / column its link names (`a.ts:12:5`, `a.md#L12`). */
export function splitPosition(spec: string): { path: string; line?: number; column?: number } {
  const at = POSITION.exec(spec);
  if (!at) return { path: spec };
  const [line, column] = [at[1] ?? at[3] ?? at[5], at[2] ?? at[4] ?? at[6]].map((n) => (n ? Number(n) : undefined));
  return { path: spec.slice(0, at.index), line, column };
}

/** `import a.b, c as d`: one link per module. */
function pythonImports(line: string, add: (start: number, spec: string, kind: LinkKind) => void) {
  const head = /^\s*import\s+/.exec(line);
  if (!head) return;
  const hash = line.indexOf("#");
  const tail = line.slice(head[0].length, hash < 0 ? undefined : hash);
  let at = head[0].length;
  for (const part of tail.split(",")) {
    const m = /^\s*([A-Za-z_][\w.]*)/.exec(part);
    if (m) add(at + m[0].length - m[1].length, m[1], "python");
    at += part.length + 1;
  }
}

// ---------------------------------------------------------------- resolving

/** `rel` from folder `dir`, normalized; null when it climbs out of the repo. */
export function join(dir: string, rel: string): string | null {
  const parts = dir ? dir.split("/") : [];
  for (const seg of rel.split("/")) {
    if (seg === "" || seg === ".") continue;
    if (seg !== "..") parts.push(seg);
    else if (!parts.length) return null;
    else parts.pop();
  }
  return parts.join("/");
}

const MODULE_EXTS = [".ts", ".tsx", ".d.ts", ".js", ".jsx", ".mjs", ".cjs", ".mts", ".cts", ".json", ".vue", ".svelte", ".css"];
// TypeScript's ESM imports name the output: "./a.js" is a.ts.
const EMITTED: Record<string, string[]> = { ".js": [".ts", ".tsx"], ".jsx": [".tsx"], ".mjs": [".mts"], ".cjs": [".cts"] };
const STYLE_EXTS = [".css", ".scss", ".sass", ".less"];

const firstIn = (files: ReadonlySet<string>, candidates: string[]) => candidates.find((c) => files.has(c)) ?? null;
// A query (vite's "?worker", "?raw") or a fragment isn't part of the path.
const bare = (spec: string) => spec.replace(/[?#].*$/, "");
const isRelative = (spec: string) => /^\.\.?(\/|$)/.test(spec);
// A URL (https:, data:, mailto:), protocol-relative, or a fragment of this page.
const isExternal = (spec: string) => /^([a-z][a-z\d+.-]*:|\/\/|#)/i.test(spec);

function moduleCandidates(base: string) {
  const dir = base ? `${base}/` : "";
  const ext = /\.[cm]?jsx?$/.exec(base)?.[0];
  const emitted = ext ? (EMITTED[ext] ?? []).map((e) => base.slice(0, -ext.length) + e) : [];
  return [...(base ? [base, ...MODULE_EXTS.map((e) => base + e)] : []), ...emitted, ...MODULE_EXTS.map((e) => `${dir}index${e}`)];
}

function styleCandidates(base: string) {
  const dir = dirname(base);
  const partial = (dir ? `${dir}/_` : "_") + basename(base);
  return [base, ...STYLE_EXTS.map((e) => base + e), ...STYLE_EXTS.map((e) => partial + e), ...STYLE_EXTS.flatMap((e) => [`${base}/index${e}`, `${base}/_index${e}`])];
}

/** Where a tsconfig alias sends `spec`, most specific pattern first (TypeScript's order). */
function aliasTargets(spec: string, aliases: Alias[]): string[] {
  const hits: [number, string[]][] = [];
  for (const { pattern, targets } of aliases) {
    const star = pattern.indexOf("*");
    if (star < 0) {
      if (pattern === spec) hits.push([Infinity, targets]);
      continue;
    }
    const [pre, post] = [pattern.slice(0, star), pattern.slice(star + 1)];
    if (spec.length < pre.length + post.length || !spec.startsWith(pre) || !spec.endsWith(post)) continue;
    const middle = spec.slice(pre.length, spec.length - post.length);
    hits.push([pre.length, targets.map((t) => t.replace("*", middle))]);
  }
  return hits.sort((a, b) => b[0] - a[0]).flatMap(([, t]) => t);
}

/**
 * Where `link` in file `from` goes, or null: unresolved links aren't links. `root` is the repo's
 * absolute path, for absolute paths in the text.
 */
export function resolveLink(link: Link, from: string, index: FileIndex, aliases: Alias[] = [], root = ""): Target | null {
  if (link.kind === "url") return { url: link.spec };
  if (link.kind === "file") return resolveFile(link.spec, dirname(from), index, root);
  const { path: spec, line, column } = link.kind === "doc" ? splitPosition(link.spec) : { path: link.spec };
  const path = resolvePath({ ...link, spec }, from, index, aliases);
  return path ? { path, line, column } : null;
}

/**
 * A link in terminal output: paths from the shell's folder `cwd` (repo-relative; null when it's
 * outside the repo, where only absolute paths into it resolve), or the repo root, as agents print them.
 */
export function resolveTerminalLink(link: Link, cwd: string | null, index: FileIndex, root: string): Target | null {
  return link.kind === "url" ? { url: link.spec } : resolveFile(link.spec, cwd, index, root);
}

function resolveFile(spec: string, dir: string | null, index: FileIndex, root: string): Target | null {
  const { path: raw, line, column } = splitPosition(spec);
  const path = slashes(raw);
  const base = slashes(root);
  const absolute = path.startsWith("/") || /^[A-Za-z]:\//.test(path);
  const candidates = absolute ? [base && path.startsWith(`${base}/`) ? path.slice(base.length + 1) : null] : dir == null ? [] : [join(slashes(dir), path), join("", path)];
  const found = firstIn(index.files, candidates.filter((c): c is string => !!c));
  return found ? { path: found, line, column } : null;
}

function resolvePath(link: Link, from: string, index: FileIndex, aliases: Alias[]): string | null {
  const { files } = index;
  const dir = dirname(from);
  const spec = link.spec;
  switch (link.kind) {
    case "module": {
      const path = bare(spec);
      // Bare package names stay unlinked: node_modules isn't in the index.
      const bases = isRelative(path) ? [join(dir, path)] : aliasTargets(path, aliases);
      return firstIn(files, bases.flatMap((b) => (b == null ? [] : moduleCandidates(b))));
    }
    case "style": {
      const path = bare(spec);
      // "~pkg" is a package; a leading "/" is the site's root, not the repo's.
      if (isExternal(path) || path.startsWith("~") || path.startsWith("/")) return null;
      // @import "a.css" means "./a.css" in CSS; Vite also takes the tsconfig aliases.
      const bases = [join(dir, path), ...aliasTargets(path, aliases)];
      return firstIn(files, bases.flatMap((b) => (b ? styleCandidates(b) : [])));
    }
    case "doc": {
      if (isExternal(spec)) return null;
      let path = bare(spec);
      try {
        path = decodeURI(path);
      } catch {
        // Not percent-encoded after all.
      }
      // On GitHub a leading "/" is the repo root.
      const target = path.startsWith("/") ? join("", path) : join(dir, path);
      return target && files.has(target) ? target : null;
    }
    case "path": {
      const target = join(dir, spec);
      return target && files.has(target) ? target : null;
    }
    case "rust": {
      // mod.rs, lib.rs and main.rs own their folder; a.rs owns a/.
      const name = basename(from);
      const owner = /^(mod|lib|main)\.rs$/.test(name) ? dir : (dir ? `${dir}/` : "") + name.replace(/\.rs$/, "");
      const base = (owner ? `${owner}/` : "") + spec;
      return firstIn(files, [`${base}.rs`, `${base}/mod.rs`]);
    }
    case "python": {
      const dots = /^\.*/.exec(spec)![0].length;
      const rest = spec.slice(dots).split(".").filter(Boolean).join("/");
      const py = (base: string | null) => (base == null ? [] : rest ? [`${base}.py`, `${base}.pyi`, `${base}/__init__.py`] : [`${base}/__init__.py`].map((p) => p.replace(/^\//, "")));
      if (dots) return firstIn(files, py(join(dir, "../".repeat(dots - 1) + rest)));
      // Absolute: from the file's folder up to the root, where its package root is unknown.
      const roots: string[] = [];
      for (let d = dir; ; d = dirname(d)) {
        roots.push(d);
        if (!d) break;
      }
      return firstIn(files, roots.flatMap((r) => py(join(r, rest))));
    }
    default:
      return null;
  }
}

// ---------------------------------------------------------------- tsconfig paths

const CONFIG = /^(tsconfig(\.[^/]+)?|jsconfig)\.json$/;

/** An index over `files`, repo-relative paths. */
export function indexFiles(files: string[]): FileIndex {
  const configs = new Map<string, string[]>();
  for (const f of files) {
    if (!CONFIG.test(basename(f))) continue;
    const dir = dirname(f);
    configs.set(dir, [...(configs.get(dir) ?? []), f]);
  }
  return { files: new Set(files), configs };
}

/**
 * The `compilerOptions.paths` aliases for file `from`: those of the nearest folder whose configs
 * have some (tsconfig.json often only references tsconfig.app.json, which has them).
 * `read` gives a config's text, or null.
 */
export async function loadAliases(from: string, index: FileIndex, read: (path: string) => Promise<string | null>): Promise<Alias[]> {
  for (let dir = dirname(from); ; dir = dirname(dir)) {
    const found = (await Promise.all((index.configs.get(dir) ?? []).map((c) => configAliases(c, index, read, 0)))).flat();
    // tsconfig.json extending tsconfig.base.json next to it brings the same ones twice.
    if (found.length || !dir) return found.filter((a, i) => found.findIndex((b) => b.pattern === a.pattern) === i);
  }
}

async function configAliases(path: string, index: FileIndex, read: (path: string) => Promise<string | null>, depth: number): Promise<Alias[]> {
  const text = await read(path).catch(() => null);
  const json = text == null ? null : parseJsonc(text);
  if (!json || typeof json !== "object") return [];
  const dir = dirname(path);
  const options = (json as { compilerOptions?: { paths?: unknown; baseUrl?: unknown } }).compilerOptions;
  const paths = options?.paths;
  if (paths && typeof paths === "object") {
    // Targets are relative to baseUrl, or to the config that sets `paths`.
    const base = typeof options.baseUrl === "string" ? join(dir, options.baseUrl) : dir;
    if (base == null) return [];
    return Object.entries(paths).map(([pattern, targets]) => ({
      pattern,
      targets: (Array.isArray(targets) ? targets : []).flatMap((t) => (typeof t === "string" ? (join(base, t) ?? []) : [])),
    }));
  }
  // Inherited from a config in the repo; a package's ("@tsconfig/node20") isn't in the index.
  const parents = [(json as { extends?: unknown }).extends].flat().filter((e): e is string => typeof e === "string" && isRelative(e));
  if (depth > 4) return [];
  // The last one wins in TypeScript.
  for (const e of parents.reverse()) {
    const p = join(dir, e);
    const parent = p && (p.endsWith(".json") ? p : `${p}.json`);
    const found = parent && index.files.has(parent) ? await configAliases(parent, index, read, depth + 1) : [];
    if (found.length) return found;
  }
  return [];
}

/** JSON with comments and trailing commas, as tsconfig files are written; null if it isn't. */
export function parseJsonc(text: string): unknown {
  let out = "";
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (c === '"') {
      const end = stringEnd(text, i);
      out += text.slice(i, end);
      i = end - 1;
    } else if (c === "/" && text[i + 1] === "/") {
      while (i < text.length && text[i] !== "\n") i++;
      out += "\n";
    } else if (c === "/" && text[i + 1] === "*") {
      const end = text.indexOf("*/", i + 2);
      i = end < 0 ? text.length : end + 1;
    } else out += c;
  }
  try {
    return JSON.parse(out.replace(/,(\s*[}\]])/g, "$1"));
  } catch {
    return null;
  }
}

/** Just past the string starting at `i`. */
function stringEnd(text: string, i: number) {
  for (let j = i + 1; j < text.length; j++) {
    if (text[j] === "\\") j++;
    else if (text[j] === '"') return j + 1;
  }
  return text.length;
}
