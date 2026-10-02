// Where links (lib/links/links) resolve and what opening one does: the repo's file lists, loaded on the
// first ⌘-hover in a tree, and the workspace that opens files. Shared by the code view's Go to
// Definition (lib/editor/definitions) and the terminal (terminalLinks below).
import type { IBufferRange, IDisposable, ILink, Terminal } from "@xterm/xterm";
import { api, fullName, type GitHubAccount, github, type Issue, issues, type Pull } from "../api";
import { IS_MAC, primaryKey } from "../platform";
import { type Alias, cellText, diskCandidates, diskTarget, type FileIndex, findTerminalLinks, folders, githubItem, hyperlinkTarget, indexCase, indexFiles, type Link, LINK_WINDOW, loadAliases, resolveLink, resolveTerminalLink, type Target } from "./links";
import { dirname, slashes } from "../path";
import { failed } from "../app/toast";
import { revealInCode } from "../editor/reveal";
import { revealPath } from "../app/openIn";
import { cached } from "../github/githubCache";

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
  /** The repo's worktrees, by absolute path: a shell in one inside `root` isn't in `root`'s checkout. */
  worktrees: string[];
  revision: number;
  open(path: string, focus: boolean): void;
  /** `show`: the explorer comes up and takes the keys, as for a folder; else it follows along if it's there. */
  reveal(path: string, show: boolean): void;
  /** origin's page on GitHub (null: it isn't there): #123, and its pull requests' and issues' URLs, open here. */
  github: string | null;
  /** A commit, by its full id, opened in History. */
  showCommit(sha: string): void;
  /** A pull request or an issue, in its tab. */
  openItem(item: { kind: "pull"; pull: Pull } | { kind: "issue"; issue: Issue }): void;
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

function cachedIn<T>(map: Map<string, T>, key: string, make: () => T) {
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
  const loading = cachedIn(indexes, key, () => (tree.rev ? api.treePaths(tree.rev) : api.listFiles()).then(indexFiles));
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
  const aliases = await cachedIn(byDir, dirname(path), () => loadAliases(path, index, read));
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

/**
 * What only terminal links open: a commit in History, and a pull request or an issue in its tab
 * (`pull`: the URL said which; `repo`: its https://github.com/owner/name).
 */
type GitTarget = { commit: string } | { number: number; pull: boolean; repo: string };
export type TerminalTarget = Target | GitTarget;

const CLICK = `${IS_MAC ? "⌘" : "Ctrl"}-click`;

/** What ⌘-click does, for the hover: as VS Code's terminal says it. */
function hint(target: TerminalTarget) {
  const does = "commit" in target ? "Show in History" : "number" in target ? `Open #${target.number}` : "url" in target ? "Follow link" : target.dir ? "Show folder" : "Open file";
  return `${does} (${CLICK})`;
}

/** A terminal link: a page in the browser, a file in the code view and shown in the explorer, a folder in the explorer, a commit in History, a pull request or issue in its tab. */
export function followTerminalLink(target: TerminalTarget) {
  if ("commit" in target) return host?.showCommit(target.commit);
  if ("number" in target) return void openItem(target);
  if ("url" in target || !host) return openTarget(target, true);
  if (target.dir) return host.reveal(target.path, true);
  openTarget(target, true);
  host.reveal(target.path, false);
}

/**
 * #123 or a pull request's or issue's URL, in its tab: as a pull request unless GitHub says there's
 * none of that number, then as an issue (whose read would take a pull request for one). Asked only
 * on a click. Not signed in, or GitHub unreachable: the page, in the browser.
 */
async function openItem({ number, pull, repo }: { number: number; pull: boolean; repo: string }) {
  const h = host;
  if (!h) return;
  // null: origin; else the fork's parent, as owner/name.
  const to = repo === h.github ? null : repo.replace("https://github.com/", "");
  try {
    const { title, state, draft, author, headRef, headSha, headRepo, baseRef, baseSha, createdAt, updatedAt, url } = await github.detail(to, number);
    return h.openItem({ kind: "pull", pull: { number, title, state, draft, author, headRef, headSha, headRepo, baseRef, baseSha, createdAt, updatedAt, url } });
  } catch {
    // No such pull request: an issue, or GitHub can't be asked.
  }
  if (!pull)
    try {
      const { title, state, stateReason, author, labels, assignees, comments, createdAt, updatedAt, url } = await issues.detail(to, number);
      return h.openItem({ kind: "issue", issue: { number, title, state, stateReason, author, labels, assignees, comments, createdAt, updatedAt, url } });
    } catch {
      // GitHub can't be asked.
    }
  openTarget({ url: `${repo}/${pull ? "pull" : "issues"}/${number}` });
}

// What the repo said of SHAs, until the working tree changes, as `disk` below: a full id, or null.
let commits = { revision: -1, known: new Map<string, string | null>() };

/** The commits `shas` name in the open repo, one git call for those not asked yet. */
async function commitIds(shas: string[], revision: number) {
  if (commits.revision !== revision || commits.known.size > 2000) commits = { revision, known: new Map() };
  const { known } = commits;
  // api.knownCommits takes 500 at most: a screen of hints has far fewer.
  const ask = [...new Set(shas.filter((s) => !known.has(s)))].slice(0, 500);
  if (ask.length) {
    const ids = await api.knownCommits(ask).catch(() => ask.map(() => null));
    ask.forEach((s, i) => known.set(s, ids[i] ?? null));
  }
  return shas.map((s) => known.get(s) ?? null);
}

/** The GitHub repos whose #123 and URLs open here: origin, then a fork's parent once the account says. */
function githubRepos(h: LinkHost) {
  if (!h.github) return [];
  const parent = cached<GitHubAccount>("account")?.parent;
  return parent ? [h.github, `https://github.com/${fullName(parent.repo)}`] : [h.github];
}

type OnDisk = { path: string; kind: "file" | "dir" } | null;
// What the disk said about paths the file list lacks, until the working tree changes: a line
// hovered again, or redrawn under the pointer while output streams, asks nothing.
let disk = { revision: -1, known: new Map<string, OnDisk>() };

/**
 * What's at `paths` on disk (ignored files and folders), spelled as the index has them when it has
 * them in another case (APFS finds either).
 */
async function onDisk(paths: string[], index: FileIndex, revision: number): Promise<OnDisk[]> {
  if (disk.revision !== revision || disk.known.size > 2000) disk = { revision, known: new Map() };
  const { known } = disk;
  const ask = [...new Set(paths.filter((p) => !known.has(p)))];
  if (ask.length) {
    const kinds = await api.pathKinds(ask).catch(() => ask.map(() => null));
    const cased = indexCase(ask.filter((_, i) => kinds[i]), index);
    ask.forEach((p, i) => {
      const kind = kinds[i];
      known.set(p, kind ? { path: cased.get(p.toLowerCase()) ?? p, kind } : null);
    });
  }
  return paths.map((p) => known.get(p) ?? null);
}

/** What a repo path is: from the file list, else the disk. */
async function kindOf(path: string, revision: number): Promise<OnDisk> {
  const index = await indexOf({ rev: null, revision });
  if (index.files.has(path)) return { path, kind: "file" };
  if (folders(index).has(path)) return { path, kind: "dir" };
  return (await onDisk([path], index, revision))[0];
}

/**
 * ⌘-click (Ctrl off macOS) in terminal output: URLs, and repo files and folders by paths from the
 * shell's folder as last known (`cwd()`: where it started, or where a split or a save found it)
 * or the repo root, with a line when one follows. A plain click stays the terminal's: it selects,
 * or goes to a program using the mouse.
 */
export function terminalLinks(term: Terminal, cwd: () => string): IDisposable {
  const title = (text: string | null) => (text ? term.element?.setAttribute("title", text) : term.element?.removeAttribute("title"));
  // OSC 8 hyperlinks, which xterm finds itself: by the same rule, but their text needn't be where
  // they go, so that shows on hover. Other schemes than http(s) come through for file://, and
  // hyperlinkTarget drops the rest (javascript:, custom ones).
  const hyperlink = (uri: string) => (host ? hyperlinkTarget(uri, host.root) : null);
  /** A file:// link's file or folder; the repo's own folder has no row in the explorer, so it's none. */
  const hyperlinkFile = async (target: Target): Promise<Target | null> => {
    if ("url" in target || !target.path || !host) return null;
    const found = await kindOf(target.path, host.revision);
    return found && (found.kind === "file" ? { ...target, path: found.path } : { path: found.path, dir: true });
  };
  let hovered: string | null = null;
  term.options.linkHandler = {
    allowNonHttpProtocols: true,
    hover: (_, uri) => {
      const target = hyperlink(uri);
      if (!target) return;
      hovered = uri;
      title(`${uri}\n${hint(target)}`);
      // A file's or a folder's wording once the list or the disk says which.
      void hyperlinkFile(target).then((t) => t && hovered === uri && title(`${uri}\n${hint(t)}`));
    },
    leave: () => {
      hovered = null;
      title(null);
    },
    activate: (e, uri) => {
      const target = hyperlink(uri);
      if (!primaryKey(e) || !target) return;
      if ("url" in target) return followTerminalLink(target);
      if (!target.path) return void revealPath("");
      void hyperlinkFile(target).then((t) => t && followTerminalLink(t));
    },
  };
  // xterm keeps one line's links at a time and files a late answer under the line asked last:
  // only the newest ask answers.
  let asked = 0;
  return term.registerLinkProvider({
    provideLinks(y, callback) {
      const ask = ++asked;
      const line = logicalLine(term, y - 1);
      const row = y - 1 - line.first;
      const found = findTerminalLinks(line.text, { start: line.starts[row], end: line.starts[row + 1] }).filter((l) => {
        const { start, end } = line.range(l);
        return start.y <= y && end.y >= y && line.whole(l);
      });
      if (!found.length || !host) return callback(undefined);
      void targetsOf(found, host, cwd()).then((targets) => {
        if (ask !== asked) return;
        const links = found.flatMap((l, i): ILink[] => {
          const target = targets[i];
          if (!target) return [];
          return [{ range: line.range(l), text: l.spec, hover: () => title(hint(target)), leave: () => title(null), activate: (e) => primaryKey(e) && followTerminalLink(target) }];
        });
        callback(links.length ? links : undefined);
      });
    },
  });
}

/**
 * The line of output buffer row `y` is in: a long one wraps over several rows, read around it as far
 * as links are looked for (LINK_WINDOW). `range`: a link's cells, from its first to its last (the
 * second half of a wide character included), 1-based as xterm's links; `whole`: no row cut off
 * either side may carry the link on past what was read.
 */
function logicalLine(term: Terminal, y: number) {
  const buf = term.buffer.active;
  const reach = Math.ceil(LINK_WINDOW / term.cols) + 1;
  let first = y;
  while (first > y - reach && first > 0 && buf.getLine(first)?.isWrapped) first--;
  let last = y;
  while (last < y + reach && buf.getLine(last + 1)?.isWrapped) last++;
  const [cutBefore, cutAfter] = [!!buf.getLine(first)?.isWrapped, !!buf.getLine(last + 1)?.isWrapped];
  const rows = Array.from({ length: last - first + 1 }, (_, i) => buf.getLine(first + i));
  const { text, cells, starts } = cellText(rows, term.cols, buf.getNullCell());
  const cellAt = (c: number) => ({ x: (c % term.cols) + 1, y: first + Math.floor(c / term.cols) + 1 });
  const range = (l: Link): IBufferRange => ({ start: cellAt(cells[l.start]), end: cellAt(cells[l.end] - 1) });
  const whole = (l: Link) => (!cutBefore || l.start > 0) && (!cutAfter || l.end < text.length);
  return { first, last, text, starts, range, whole };
}

/**
 * What terminal links go to, null for one that goes nowhere: paths by the shell's folder `cwd`, from
 * the file list or else the disk; SHAs the repo knows; #123 when origin is on GitHub. Commits and
 * issues only from a shell in this checkout, whose repo they'd be.
 */
async function targetsOf(found: Link[], h: LinkHost, cwd: string): Promise<(TerminalTarget | null)[]> {
  // Windows paths come with backslashes; the index and the links have forward ones.
  const [root, from] = [slashes(h.root), slashes(cwd)];
  const dir = from === root ? "" : from.startsWith(`${root}/`) ? from.slice(root.length + 1) : null;
  // A shell in a worktree inside this one (an agent's): its paths are that checkout's files,
  // never the same names in this one's.
  const nested =
    dir !== null &&
    h.worktrees.some((w) => {
      const at = slashes(w);
      return at.startsWith(`${root}/`) && (from === at || from.startsWith(`${at}/`));
    });
  // Only a shell in this checkout may fall back to its root (a …/ path, a root-relative one).
  const rootToo = dir !== null && !nested;
  const repos = githubRepos(h);
  const files = found.some((l) => l.kind === "file");
  const shas = dir === null ? [] : found.filter((l) => l.kind === "commit").map((l) => l.spec);
  const [index, ids] = await Promise.all([files ? indexOf({ rev: null, revision: h.revision }) : NO_FILES, shas.length ? commitIds(shas, h.revision) : []]);
  const known = new Map(shas.map((s, i) => [s, ids[i]]));
  const targets = found.map((l): TerminalTarget | null => {
    if (l.kind === "commit") return known.get(l.spec) ? { commit: known.get(l.spec)! } : null;
    const item = l.kind === "issue" && dir === null ? null : githubItem(l, repos);
    return item ?? resolveTerminalLink(l, dir, index, root, rootToo);
  });
  // What the list lacks may be on disk, ignored: one call for the line's paths, only when hovered.
  const asks = found.map((l, i) => (targets[i] ? [] : diskCandidates(l, dir, root, rootToo)));
  const hits = await onDisk(asks.flat(), index, h.revision);
  let at = 0;
  asks.forEach((paths, i) => {
    const hit = hits.slice(at, at + paths.length).find(Boolean);
    if (hit) targets[i] = diskTarget(found[i].spec, hit.path, hit.kind);
    at += paths.length;
  });
  return targets;
}

/** A link on screen in a terminal: its cells (1-based, as xterm's links), its text and where it goes. */
export interface ShownLink {
  range: IBufferRange;
  text: string;
  target: TerminalTarget;
}

/**
 * Every link that goes somewhere on `term`'s screen, top to bottom, in whichever buffer shows (a
 * full-screen program's too): the hints' (lib/terminal/hints). Asked once per call, all together.
 */
export async function shownLinks(term: Terminal, cwd: string): Promise<ShownLink[]> {
  const h = host;
  if (!h) return [];
  const top = term.buffer.active.viewportY;
  const bottom = top + term.rows - 1;
  const found: { link: Link; range: IBufferRange }[] = [];
  for (let y = top; y <= bottom; ) {
    const line = logicalLine(term, y);
    for (const l of findTerminalLinks(line.text)) {
      const range = line.range(l);
      // Labelled at its first cell, which must be on screen.
      if (line.whole(l) && range.start.y - 1 >= top && range.start.y - 1 <= bottom) found.push({ link: l, range });
    }
    y = Math.max(line.last, y) + 1;
  }
  if (!found.length) return [];
  const targets = await targetsOf(
    found.map((f) => f.link),
    h,
    cwd,
  );
  return found.flatMap((f, i) => {
    const target = targets[i];
    return target ? [{ range: f.range, text: f.link.spec, target }] : [];
  });
}
