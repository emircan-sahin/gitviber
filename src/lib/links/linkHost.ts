// Where links (lib/links/links) resolve and what opening one does: the repo's file lists, loaded on the
// first ⌘-hover in a tree, and the workspace that opens files. Shared by the code view's Go to
// Definition (lib/editor/definitions) and the terminal (terminalLinks below).
import type { IDisposable, ILink, Terminal } from "@xterm/xterm";
import { api, github } from "../api";
import { IS_MAC, primaryKey } from "../platform";
import { type Alias, cellText, diskCandidates, diskTarget, type FileIndex, findTerminalLinks, hyperlinkTarget, indexFiles, type Link, LINK_WINDOW, loadAliases, resolveLink, resolveTerminalLink, type Target } from "./links";
import { dirname, slashes } from "../path";
import { failed } from "../app/toast";
import { revealInCode } from "../editor/reveal";
import { revealPath } from "../app/openIn";

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

/** The open workspace: its repo's absolute path, its working tree's revision, and how it opens a file or shows one in the explorer. */
interface LinkHost {
  root: string;
  revision: number;
  open(path: string, focus: boolean): void;
  /** `show`: the explorer comes up and takes the keys, as for a folder; else it follows along if it's there. */
  reveal(path: string, show: boolean): void;
}

let host: LinkHost | null = null;
// A file for a workspace that's on its way; a few seconds, so a repo that never shows can't
// have it open on a later visit.
let waiting: { root: string; target: Target; until: number } | null = null;
export function setLinkHost(h: LinkHost | null) {
  host = h;
  const w = waiting;
  if (!h || w?.root !== h.root) return;
  waiting = null;
  if (Date.now() < w.until) openTarget(w.target, true);
}

const indexes = new Map<string, Promise<FileIndex>>();
// Per index: aliases read from a tree that failed to list would outlive it.
const aliasSets = new WeakMap<FileIndex, Map<string, Promise<Alias[]>>>();

/** For a repo switch: the trees were the other repo's. */
export function resetLinks() {
  indexes.clear();
}

function cached<T>(map: Map<string, T>, key: string, make: () => T) {
  let value = map.get(key);
  if (value) return value;
  map.set(key, (value = make()));
  if (map.size > 16) map.delete(map.keys().next().value!);
  return value;
}

const NO_FILES = indexFiles([]);

/** A tree's files. A failure (a PR head not fetched yet) isn't kept: the next hover asks again. */
function indexOf(tree: LinkTree) {
  const key = tree.rev ?? `worktree@${tree.revision}`;
  // The working tree's list at an older revision is out of date, and one full copy of every path
  // piled up per change while an agent wrote files. Only older ones go: a late ask can't evict a newer.
  if (!tree.rev) for (const k of indexes.keys()) if (k.startsWith("worktree@") && Number(k.slice(9)) < tree.revision) indexes.delete(k);
  const loading = cached(indexes, key, () => (tree.rev ? api.treePaths(tree.rev) : api.listFiles()).then(indexFiles));
  return loading.catch(() => {
    if (indexes.get(key) === loading) indexes.delete(key);
    return NO_FILES;
  });
}

/** Resolves links in the file `side` shows. */
export async function resolverFor({ path, tree }: LinkSide) {
  const index = await indexOf(tree);
  const read = async (p: string) => {
    const f = await (tree.rev ? api.textAt(tree.rev, p) : api.readFile(p));
    return f.exists && !f.binary && !f.tooLarge ? f.text : null;
  };
  let byDir = aliasSets.get(index);
  if (!byDir) aliasSets.set(index, (byDir = new Map()));
  const aliases = await cached(byDir, dirname(path), () => loadAliases(path, index, read));
  const root = host?.root ?? "";
  return (link: Link) => resolveLink(link, path, index, aliases, root);
}


/** Opens a link's target: a page in the browser, or a file in a tab at its line. `focus`: the code view takes the keys. */
export function openTarget(target: Target, focus = false) {
  if ("url" in target) return void github.openUrl(target.url).catch(failed("Could not open the link"));
  if (!host) return;
  if (target.line) revealInCode({ path: target.path, line: target.line, column: target.column ?? 1 });
  host.open(target.path, focus);
}

/** Opens a file in the workspace of `root` (a repo's absolute path), now or once it's up: `gitviber a.ts:12`. */
export function openTargetIn(root: string, target: Target) {
  waiting = null;
  if (host?.root === root) openTarget(target, true);
  else waiting = { root, target, until: Date.now() + 10_000 };
}

const CLICK = `${IS_MAC ? "⌘" : "Ctrl"}-click`;

/** What ⌘-click does, for the hover: as VS Code's terminal says it. */
const hint = (target: Target) => `${"url" in target ? "Follow link" : target.dir ? "Show folder" : "Open file"} (${CLICK})`;

/** A terminal link: a page in the browser, a file in the code view and shown in the explorer, a folder in the explorer. */
function follow(target: Target) {
  if ("url" in target || !host) return openTarget(target, true);
  if (target.dir) return host.reveal(target.path, true);
  openTarget(target, true);
  host.reveal(target.path, false);
}

/** Asks the disk about paths the file list doesn't have: ignored files and folders. */
const kindsOf = (paths: string[]) => (paths.length ? api.pathKinds(paths).catch(() => paths.map(() => null)) : Promise.resolve([]));

/**
 * ⌘-click (Ctrl off macOS) in terminal output: URLs, and repo files and folders by paths from the
 * shell's starting folder (it can't be told where a `cd` went) or the repo root, with a line when
 * one follows. A plain click stays the terminal's: it selects, or goes to a program using the mouse.
 */
export function terminalLinks(term: Terminal, cwd: string): IDisposable {
  const title = (text: string | null) => (text ? term.element?.setAttribute("title", text) : term.element?.removeAttribute("title"));
  // OSC 8 hyperlinks, which xterm finds itself: by the same rule, but their text needn't be where
  // they go, so that shows on hover. Other schemes than http(s) come through for file://, and
  // hyperlinkTarget drops the rest (javascript:, custom ones).
  const hyperlink = (uri: string) => (host ? hyperlinkTarget(uri, host.root) : null);
  term.options.linkHandler = {
    allowNonHttpProtocols: true,
    hover: (_, uri) => hyperlink(uri) && title(`${uri}\n${hint({ url: uri })}`),
    leave: () => title(null),
    activate: (e, uri) => {
      const [h, target] = [host, hyperlink(uri)];
      if (!primaryKey(e) || !h || !target) return;
      if ("url" in target) return follow(target);
      // The repo's own folder has no row in the explorer.
      if (!target.path) return void revealPath("");
      void indexOf({ rev: null, revision: h.revision }).then(async (index) => {
        const kind = index.files.has(target.path) ? "file" : index.dirs.has(target.path) ? "dir" : (await kindsOf([target.path]))[0];
        if (kind) follow(kind === "file" ? target : { path: target.path, dir: true });
      });
    },
  };
  return term.registerLinkProvider({
    provideLinks(y, callback) {
      const h = host;
      const buf = term.buffer.active;
      // A long line wraps over several rows: read the rows of it around this one, as far as links
      // are looked for (LINK_WINDOW).
      const reach = Math.ceil(LINK_WINDOW / term.cols) + 1;
      let first = y - 1;
      while (first > y - 1 - reach && first > 0 && buf.getLine(first)?.isWrapped) first--;
      let last = y - 1;
      while (last < y - 1 + reach && buf.getLine(last + 1)?.isWrapped) last++;
      // Rows cut off either side: a link at that edge may go on past it.
      const [cutBefore, cutAfter] = [!!buf.getLine(first)?.isWrapped, !!buf.getLine(last + 1)?.isWrapped];
      const rows = Array.from({ length: last - first + 1 }, (_, i) => buf.getLine(first + i));
      const { text, cells, starts } = cellText(rows, term.cols, buf.getNullCell());
      const cellAt = (c: number) => ({ x: (c % term.cols) + 1, y: first + Math.floor(c / term.cols) + 1 });
      // From a link's first cell to its last, the second half of a wide character included.
      const range = (l: Link) => ({ start: cellAt(cells[l.start]), end: cellAt(cells[l.end] - 1) });
      const row = y - 1 - first;
      const found = findTerminalLinks(text, { start: starts[row], end: starts[row + 1] }).filter((l) => {
        const { start, end } = range(l);
        return start.y <= y && end.y >= y && (!cutBefore || l.start > 0) && (!cutAfter || l.end < text.length);
      });
      if (!found.length || !h) return callback(undefined);
      // Windows paths come with backslashes; the index and the links have forward ones.
      const [root, from] = [slashes(h.root), slashes(cwd)];
      const dir = from === root ? "" : from.startsWith(`${root}/`) ? from.slice(root.length + 1) : null;
      const needsIndex = found.some((l) => l.kind !== "url");
      void (needsIndex ? indexOf({ rev: null, revision: h.revision }) : Promise.resolve(NO_FILES)).then(async (index) => {
        const targets = found.map((l) => resolveTerminalLink(l, dir, index, root));
        // What the list lacks may be on disk, ignored: one call for the line's paths, only when hovered.
        const asks = found.map((l, i) => (targets[i] ? [] : diskCandidates(l, dir, root)));
        const kinds = await kindsOf(asks.flat());
        let at = 0;
        asks.forEach((paths, i) => {
          const hit = paths.findIndex((_, j) => kinds[at + j]);
          if (hit >= 0) targets[i] = diskTarget(found[i].spec, paths[hit], kinds[at + hit]!);
          at += paths.length;
        });
        const links = found.flatMap((l, i): ILink[] => {
          const target = targets[i];
          if (!target) return [];
          return [{ range: range(l), text: l.spec, hover: () => title(hint(target)), leave: () => title(null), activate: (e) => primaryKey(e) && follow(target) }];
        });
        callback(links.length ? links : undefined);
      });
    },
  });
}
