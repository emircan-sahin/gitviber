import type { FileChange, RepoStatus } from "../../lib/api/types.ts";
import { createStore } from "../../lib/store.ts";
import { isRecord, readJson, writeJson } from "../../lib/storage.ts";
import { treeOrder } from "../../lib/ui/pathTree.ts";

export type ChangeSort = "name" | "recent";

/** How Changes lists files: as a list or a folder tree, by name (git's order) or newest first. Kept across restarts. */
export interface ChangesView {
  tree: boolean;
  sort: ChangeSort;
}

const KEY = "gitviber.changes-view";
const DEFAULT: ChangesView = { tree: false, sort: "name" };
const stored = readJson<unknown>(KEY, null);
export const changesView = createStore<ChangesView>(
  isRecord(stored) ? { tree: stored.tree === true, sort: stored.sort === "recent" ? "recent" : "name" } : DEFAULT,
);

export function setChangesView(patch: Partial<ChangesView>) {
  const next = { ...changesView.get(), ...patch };
  changesView.set(next);
  writeJson(KEY, next);
}

/** Modified times (ms) of the listed files on disk, by path; a deleted file has none. */
export type Mtimes = ReadonlyMap<string, number>;

/** What sorts files newest first, when they're sorted so; files with no time (deleted) go last. */
export const rankFor = (view: ChangesView, mtimes: Mtimes | null) =>
  view.sort === "recent" && mtimes ? (f: FileChange) => mtimes.get(f.path) ?? -Infinity : undefined;

/** One list's files in the order Changes shows them. */
export function orderFiles(files: FileChange[], view: ChangesView, mtimes: Mtimes | null): FileChange[] {
  const rank = rankFor(view, mtimes);
  if (view.tree) return treeOrder(files, (f) => f.path, rank);
  if (!rank) return files;
  // Stable: ties keep git's order.
  return files
    .map((f, i) => ({ f, i, r: rank(f) }))
    .sort((a, b) => (a.r === b.r ? a.i - b.i : b.r - a.r))
    .map(({ f }) => f);
}

/** `status` with each list in the order Changes shows it, which J/K follow too. */
export function ordered(status: RepoStatus, view: ChangesView, mtimes: Mtimes | null): RepoStatus {
  if (!view.tree && view.sort === "name") return status;
  return {
    ...status,
    conflicted: orderFiles(status.conflicted, view, mtimes),
    staged: orderFiles(status.staged, view, mtimes),
    unstaged: orderFiles(status.unstaged, view, mtimes),
  };
}


/** Folders closed in the tree, as "<list>:<path>" (the deepest folder of a compacted row); for the session only. */
export const closedFolders = createStore<ReadonlySet<string>>(new Set());

export function toggleFolder(key: string, open?: boolean) {
  const now = closedFolders.get();
  if (open ?? now.has(key)) {
    if (!now.has(key)) return;
    const next = new Set(now);
    next.delete(key);
    closedFolders.set(next);
  } else if (!now.has(key)) closedFolders.set(new Set(now).add(key));
}
