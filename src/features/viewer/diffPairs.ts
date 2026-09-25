import { useEffect, useMemo, useRef, useState } from "react";
import { api, type Blame, type DiffKind, type DiffPair, type DiffRow, errorMessage, type Whitespace } from "@/lib/api";
import { toast } from "@/lib/app/toast";
import { resetDefinitions } from "@/lib/editor/definitions";
import { resetModels } from "@/lib/editor/monaco";
import { type LinkSide, resetLinks } from "@/lib/links/linkHost";
import { type Selection, selectionPath } from "@/lib/repo/selection";
import { diffWhitespace, getSettings } from "@/lib/settings";

/** Tabs whose content is a diff of one file (everything except PR and issue overviews). */
export type FileSelection = Exclude<Selection, { kind: "pull" | "issue" }>;

export function pairArgs(sel: FileSelection, revision: number, whitespace: Whitespace | null = null) {
  const kind: DiffKind =
    sel.kind === "file" ? "worktree" : sel.kind === "conflict" ? "unstaged" : sel.kind === "pr-file" ? "range" : sel.kind;
  const path = selectionPath(sel);
  const oldPath = sel.kind === "file" ? null : sel.file.oldPath;
  const sha = sel.kind === "commit" ? sel.commit.sha : sel.kind === "pr-file" ? sel.range.head : null;
  const base = sel.kind === "pr-file" ? sel.range.base : null;
  // Commits and PR ranges never change, so only working-tree views follow the revision counter.
  const rev = sel.kind === "commit" || sel.kind === "pr-file" ? 0 : revision;
  const id = `${kind}\0${path}\0${oldPath}\0${sha}\0${base}\0${whitespace}`;
  return { kind, path, oldPath, sha, base, whitespace, id, rev, key: `${id}\0${rev}` };
}

/**
 * Where the code view's Go to Definition looks: a commit's sides in the commit and its parent, a
 * PR's in its base and head. Working-tree diffs look in the working tree for both sides (their old
 * side is the index or HEAD, close enough). A place found opens in the working-tree file.
 */
export function linkSides(sel: FileSelection, revision: number): { original: LinkSide | null; modified: LinkSide } {
  const path = selectionPath(sel);
  if (sel.kind === "file") return { original: null, modified: { path, tree: { rev: null, revision } } };
  const [before, after] = sel.kind === "commit" ? [`${sel.commit.sha}^`, sel.commit.sha] : sel.kind === "pr-file" ? [sel.range.base, sel.range.head] : [null, null];
  return { original: { path: sel.file.oldPath ?? path, tree: { rev: before, revision } }, modified: { path, tree: { rev: after, revision } } };
}

// Recent diffs, one per view of a file (`id`) at the revision it was read at: keyed by revision,
// every change an agent made kept another copy. Per repo: `generation` changes on a switch, and
// replies still in flight from the previous repo are dropped.
const pairCache = new Map<string, { rev: number; pair: DiffPair }>();
const cachedPair = (id: string, rev: number) => {
  const hit = pairCache.get(id);
  return hit?.rev === rev ? hit.pair : undefined;
};
let generation = 0;
export function resetPairCache() {
  pairCache.clear();
  blames.clear();
  resetLinks();
  resetDefinitions();
  resetModels();
  generation++;
}
function remember(id: string, rev: number, pair: DiffPair, gen: number) {
  const had = pairCache.get(id);
  // A late reply for an older revision doesn't replace a newer one.
  if (gen !== generation || (had && had.rev > rev)) return pair;
  // The same diff read again keeps the copy already here: the code view's kept models match it by identity.
  const same = had && samePair(had.pair, pair) ? had.pair : pair;
  pairCache.delete(id);
  pairCache.set(id, { rev, pair: same });
  if (pairCache.size > 32) pairCache.delete(pairCache.keys().next().value!);
  return same;
}

/** Loads a diff in the background, so opening it next is instant. */
export function prefetchSelection(sel: Selection, revision: number) {
  if (sel.kind === "pull" || sel.kind === "issue") return;
  const { kind, path, oldPath, sha, base, whitespace, id, rev } = pairArgs(sel, revision, diffWhitespace(getSettings()));
  if (cachedPair(id, rev)) return;
  const gen = generation;
  api
    .diffPair(kind, path, oldPath, sha, base, whitespace)
    .then((p) => remember(id, rev, p, gen))
    .catch(() => {});
}

// Every change anywhere in the repo bumps the revision; a file that didn't change keeps its
// pair object, so the code view doesn't rebuild and re-render every row for nothing. The rows
// differ on the same texts when whitespace is ignored or no longer is.
const samePair = (a: DiffPair | null, b: DiffPair) =>
  !!a && sameText(a.original, b.original) && sameText(a.modified, b.modified) && sameRows(a.rows, b.rows);
const sameText = (a: DiffPair["original"], b: DiffPair["original"]) =>
  a.text === b.text && a.exists === b.exists && a.binary === b.binary && a.tooLarge === b.tooLarge && a.lossy === b.lossy && a.lfsMissing === b.lfsMissing;
const sameRows = (a: DiffRow[], b: DiffRow[]) =>
  a.length === b.length && a.every((r, i) => r.k === b[i].k && r.o === b[i].o && r.n === b[i].n && String(r.e) === String(b[i].e));

export function usePair(sel: FileSelection, revision: number, ws: Whitespace | null) {
  const { kind, path, oldPath, sha, base, whitespace, id, rev, key } = pairArgs(sel, revision, ws);
  const [pair, setPair] = useState<DiffPair | null>(() => cachedPair(id, rev) ?? null);
  const [error, setError] = useState<string | null>(null);
  const latest = useRef(0);
  const applied = useRef(0);

  useEffect(() => {
    const hit = cachedPair(id, rev);
    if (hit) {
      // Counts as the newest reply: one still in flight for another key (the other whitespace
      // setting, say) must not replace it when it lands.
      applied.current = ++latest.current;
      setPair((prev) => (samePair(prev, hit) ? prev : hit));
      setError(null);
      return;
    }
    // Apply any response newer than the last applied one, instead of dropping superseded
    // requests: a file rewritten faster than it loads would otherwise never update.
    const seq = ++latest.current;
    const gen = generation;
    api
      .diffPair(kind, path, oldPath, sha, base, whitespace)
      .then((p) => {
        const q = remember(id, rev, p, gen);
        if (seq > applied.current) {
          applied.current = seq;
          setPair((prev) => (samePair(prev, q) ? prev : q));
          setError(null);
        }
      })
      .catch((e) => seq >= latest.current && setError(errorMessage(e)));
  }, [key, kind, path, oldPath, sha, base, whitespace]);
  return { pair, error };
}

// Blame by file content and HEAD: the same text at the same HEAD blames the same, so a file is
// blamed once per version, not on every refresh.
const blames = new Map<string, Promise<Blame>>();

export function useBlame(path: string | null, pair: DiffPair | null, head: string | null) {
  const [result, setResult] = useState<{ key: string; blame: Blame } | null>(null);
  const text = path ? pair?.modified.text : undefined;
  const version = useMemo(() => text !== undefined && `${text.length}:${hash(text)}`, [text]);
  const key = path && version ? `${path}\0${head}\0${version}` : null;
  useEffect(() => {
    if (!key || !path) return;
    let p = blames.get(key);
    if (!p) {
      p = api.blame(path);
      blames.set(key, p);
      p.catch(() => blames.delete(key));
      if (blames.size > 32) blames.delete(blames.keys().next().value!);
    }
    let alive = true;
    p.then(
      (blame) => alive && setResult({ key, blame }),
      (e) => alive && toast("error", "Could not blame this file", errorMessage(e)),
    );
    return () => {
      alive = false;
    };
  }, [key, path]);
  return result && result.key === key ? result.blame : null;
}

/** FNV-1a: tells file versions apart for the blame cache. */
function hash(text: string) {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) h = Math.imul(h ^ text.charCodeAt(i), 0x01000193);
  return (h >>> 0).toString(36);
}
