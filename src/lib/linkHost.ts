// Where links (lib/links) resolve and what opening one does: the repo's file lists, loaded on the
// first ⌘-hover in a tree, and the workspace that opens files. Shared by the code view
// (lib/codeLinks) and the terminal (terminalLinks below).
import type { IDisposable, ILink, Terminal } from "@xterm/xterm";
import { api, errorMessage, github } from "./api";
import { IS_MAC } from "./commands";
import { type Alias, dirname, type FileIndex, findTerminalLinks, indexFiles, type Link, loadAliases, resolveLink, resolveTerminalLink, type Target } from "./links";
import { toast } from "./toast";

/** Where a file's paths resolve: a commit's tree (`<sha>`, `<sha>^`), or the working tree (null) as of `revision`. */
export interface LinkTree {
  rev: string | null;
  revision: number;
}

/** One side of the code view: the file it shows, by its name there, and the tree it's from. */
export interface LinkSide {
  path: string;
  tree: LinkTree;
}

/** The open workspace: its repo's absolute path, its working tree's revision, and how it opens a file. */
export interface LinkHost {
  root: string;
  revision: number;
  open(path: string, focus: boolean): void;
}

let host: LinkHost | null = null;
export function setLinkHost(h: LinkHost | null) {
  host = h;
}

// A failure (a PR head that isn't fetched) stays cached as no files, rather than running git on every hover.
const indexes = new Map<string, Promise<FileIndex>>();
const aliasSets = new Map<string, Promise<Alias[]>>();

/** For a repo switch: the trees were the other repo's. */
export function resetLinks() {
  indexes.clear();
  aliasSets.clear();
}

function cached<T>(map: Map<string, T>, key: string, make: () => T) {
  let value = map.get(key);
  if (value) return value;
  map.set(key, (value = make()));
  if (map.size > 16) map.delete(map.keys().next().value!);
  return value;
}

const treeKey = (tree: LinkTree) => tree.rev ?? `worktree@${tree.revision}`;
const indexOf = (tree: LinkTree) => cached(indexes, treeKey(tree), () => (tree.rev ? api.treePaths(tree.rev) : api.listFiles()).then(indexFiles, () => indexFiles([])));

/** Resolves links in the file `side` shows. */
export async function resolverFor({ path, tree }: LinkSide) {
  const index = await indexOf(tree);
  const read = async (p: string) => {
    const f = await (tree.rev ? api.textAt(tree.rev, p) : api.readFile(p));
    return f.exists && !f.binary && !f.tooLarge ? f.text : null;
  };
  const aliases = await cached(aliasSets, `${treeKey(tree)}\0${dirname(path)}`, () => loadAliases(path, index, read));
  const root = host?.root ?? "";
  return (link: Link) => resolveLink(link, path, index, aliases, root);
}

/** ⌘ on macOS, Ctrl elsewhere: what makes a click follow a link. */
export const linkKey = (e: { metaKey: boolean; ctrlKey: boolean }) => (IS_MAC ? e.metaKey : e.ctrlKey);

// A line to show once the file opens (MonacoView takes it). Another file shown first drops it: a
// file that opened in a preview instead mustn't jump there some later time it's shown.
let reveal: { path: string; line: number; column: number } | null = null;
const revealListeners = new Set<() => void>();

/** The line waiting for `path`, taken; `shown`: `path` is now on show, so a line for another file is dropped. */
export function takeReveal(path: string, shown: boolean) {
  const r = reveal;
  if (r && (r.path === path || shown)) reveal = null;
  return r?.path === path ? { lineNumber: r.line, column: r.column } : null;
}

/** Called when a line is asked for, so a view already showing that file can go there. */
export function onReveal(fn: () => void) {
  revealListeners.add(fn);
  return () => void revealListeners.delete(fn);
}

/** Opens a link's target: a page in the browser, or a file in a tab at its line. `focus`: the code view takes the keys. */
export function openTarget(target: Target, focus = false) {
  if ("url" in target) return void github.openUrl(target.url).catch((e) => toast("error", "Could not open the link", errorMessage(e)));
  if (!host) return;
  if (target.line) {
    reveal = { path: target.path, line: target.line, column: target.column ?? 1 };
    revealListeners.forEach((l) => l());
  }
  host.open(target.path, focus);
}

/**
 * ⌘-click in terminal output: URLs, and repo files by paths from the shell's starting folder
 * (it can't be told where a `cd` went) or the repo root, with a line when one follows.
 */
export function terminalLinks(term: Terminal, cwd: string): IDisposable {
  return term.registerLinkProvider({
    provideLinks(y, callback) {
      const h = host;
      const buf = term.buffer.active;
      // A long line wraps over several rows: read it whole. Its rows are full, so offsets map back
      // by the width (wide characters take two cells and would shift what follows them).
      let first = y - 1;
      while (first > 0 && buf.getLine(first)?.isWrapped) first--;
      let last = y - 1;
      while (buf.getLine(last + 1)?.isWrapped) last++;
      let text = "";
      for (let r = first; r <= last; r++) text += buf.getLine(r)?.translateToString(r === last) ?? "";
      const at = (i: number) => ({ x: (i % term.cols) + 1, y: first + Math.floor(i / term.cols) + 1 });
      const found = findTerminalLinks(text).filter((l) => at(l.start).y <= y && at(l.end - 1).y >= y);
      if (!found.length || !h) return callback(undefined);
      const dir = cwd === h.root ? "" : cwd.startsWith(`${h.root}/`) ? cwd.slice(h.root.length + 1) : null;
      const needsIndex = found.some((l) => l.kind !== "url");
      void (needsIndex ? indexOf({ rev: null, revision: h.revision }) : Promise.resolve(indexFiles([]))).then((index) => {
        const links = found.flatMap((l): ILink[] => {
          const target = resolveTerminalLink(l, dir, index, h.root);
          if (!target) return [];
          return [{ range: { start: at(l.start), end: at(l.end - 1) }, text: l.spec, activate: (e) => linkKey(e) && openTarget(target, true) }];
        });
        callback(links.length ? links : undefined);
      });
    },
  });
}
