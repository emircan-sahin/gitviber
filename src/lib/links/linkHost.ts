// Where links (lib/links/links) resolve and what opening one does: the repo's file lists, loaded on the
// first ⌘-hover in a tree, and the workspace that opens files. Shared by the code view's Go to
// Definition (lib/editor/definitions) and the terminal (terminalLinks below).
import type { IDisposable, ILink, Terminal } from "@xterm/xterm";
import { api, github } from "../api";
import { primaryKey } from "../platform";
import { type Alias, cellText, type FileIndex, findTerminalLinks, hyperlinkTarget, indexFiles, type Link, LINK_WINDOW, loadAliases, resolveLink, resolveTerminalLink, type Target } from "./links";
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

/** The open workspace: its repo's absolute path, its working tree's revision, and how it opens a file. */
interface LinkHost {
  root: string;
  revision: number;
  open(path: string, focus: boolean): void;
}

let host: LinkHost | null = null;
export function setLinkHost(h: LinkHost | null) {
  host = h;
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

/**
 * ⌘-click (Ctrl off macOS) in terminal output: URLs, and repo files by paths from the shell's starting folder
 * (it can't be told where a `cd` went) or the repo root, with a line when one follows.
 */
export function terminalLinks(term: Terminal, cwd: string): IDisposable {
  // OSC 8 hyperlinks, which xterm finds itself: by the same rule, but their text needn't be where
  // they go, so that shows on hover. Other schemes than http(s) come through for file://, and
  // hyperlinkTarget drops the rest (javascript:, custom ones).
  const hyperlink = (uri: string) => (host ? hyperlinkTarget(uri, host.root) : null);
  term.options.linkHandler = {
    allowNonHttpProtocols: true,
    hover: (_, uri) => hyperlink(uri) && term.element?.setAttribute("title", uri),
    leave: () => term.element?.removeAttribute("title"),
    activate: (e, uri) => {
      const [h, target] = [host, hyperlink(uri)];
      if (!primaryKey(e) || !h || !target) return;
      if ("url" in target) return openTarget(target, true);
      // A file opens in the code view; a folder, or a file not listed (ignored), is revealed in the file manager.
      void indexOf({ rev: null, revision: h.revision }).then((index) => (index.files.has(target.path) ? openTarget(target, true) : revealPath(target.path)));
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
      void (needsIndex ? indexOf({ rev: null, revision: h.revision }) : Promise.resolve(NO_FILES)).then((index) => {
        const links = found.flatMap((l): ILink[] => {
          const target = resolveTerminalLink(l, dir, index, root);
          if (!target) return [];
          return [{ range: range(l), text: l.spec, activate: (e) => primaryKey(e) && openTarget(target, true) }];
        });
        callback(links.length ? links : undefined);
      });
    },
  });
}
