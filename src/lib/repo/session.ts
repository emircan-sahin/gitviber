import { type Selection, selectionKey } from "./selection";
import { getSettings } from "../settings";
import { readJson, writeJson } from "../storage";
import { folderName, joinPath } from "../path";

/** What a worktree's window looked like, so reopening the app picks up where it was. */
interface WorkspaceSnapshot {
  tabs: { key: string; sel: Selection; preview: boolean }[];
  active: string | null;
  listTab: string;
  /** Viewed marks: `kind:path` → the file's content signature when it was marked. */
  viewed: [string, string][];
  /** The full ref the branch is reviewed against, while Changes shows that review ("" before one is picked). */
  review?: string | null;
}

/** A commit message being written in a worktree, kept until it's committed. */
export interface CommitDraft {
  summary: string;
  body: string;
  /** "Name <email>", added as Co-authored-by trailers. */
  coAuthors: string[];
}

const KEY = "gitviber.workspaces";
const DRAFTS_KEY = "gitviber.drafts";
const EDITS_KEY = "gitviber.fileEdits";
const WORKTREE_DIRS_KEY = "gitviber.worktreeDirs";
// Agent worktrees come and go; keep only the most recently used.
const MAX = 30;

const isRecord = (v: unknown): v is Record<string, unknown> => !!v && typeof v === "object" && !Array.isArray(v);
const all = (key: string) => readJson(key, {}, isRecord);

/** Stores `value` under `root` (null removes it), keeping the MAX most recently saved roots. */
function put(key: string, root: string, value: unknown) {
  const { [root]: _, ...rest } = all(key);
  // Insertion order is recency: the one saved now goes last, the oldest drop off the front.
  const entries = [...Object.entries(rest), ...(value === null ? [] : [[root, value] as const])].slice(-MAX);
  return writeJson(key, Object.fromEntries(entries));
}

export function loadWorkspace(root: string): WorkspaceSnapshot | null {
  const s = all(KEY)[root] as WorkspaceSnapshot | undefined;
  if (!s || !Array.isArray(s.tabs) || !Array.isArray(s.viewed)) return null;
  // Keys are re-derived: their format changes (PRs went from number to url), and a stale key
  // would stop the list row matching its tab. A tab too malformed to key is dropped.
  const rekey = (sel: Selection) => {
    try {
      return selectionKey(sel);
    } catch {
      return null;
    }
  };
  const tabs = s.tabs.flatMap((t) => {
    const key = typeof t?.key === "string" && typeof t.sel?.kind === "string" ? rekey(t.sel) : null;
    return key ? [{ ...t, old: t.key, key }] : [];
  });
  const active = tabs.find((t) => t.old === s.active)?.key ?? null;
  return { ...s, tabs: tabs.map(({ old: _, ...t }) => t), active };
}

export function saveWorkspace(root: string, snapshot: WorkspaceSnapshot) {
  put(KEY, root, snapshot);
}

export function loadDraft(root: string): CommitDraft | null {
  const d = all(DRAFTS_KEY)[root] as Partial<CommitDraft> | undefined;
  if (!d || typeof d.summary !== "string" || typeof d.body !== "string") return null;
  const coAuthors = Array.isArray(d.coAuthors) ? d.coAuthors.filter((a) => typeof a === "string") : [];
  return { summary: d.summary, body: d.body, coAuthors };
}

/** An empty draft is dropped rather than stored. */
export function saveDraft(root: string, draft: CommitDraft) {
  put(DRAFTS_KEY, root, draft.summary || draft.body || draft.coAuthors.length ? draft : null);
}

/** A file edited in the file view and not saved: its text, and the file's when the edit began. */
export interface FileEdit {
  text: string;
  base: string;
}

export function loadEdits(root: string): Record<string, FileEdit> {
  const saved = all(EDITS_KEY)[root];
  if (!isRecord(saved)) return {};
  const ok = (e: unknown): e is FileEdit => isRecord(e) && typeof e.text === "string" && typeof e.base === "string";
  return Object.fromEntries(Object.entries(saved).filter((entry): entry is [string, FileEdit] => ok(entry[1])));
}

/** False when storage is full: the edits then live only until the app quits. */
export function saveEdits(root: string, edits: Record<string, FileEdit>) {
  return put(EDITS_KEY, root, Object.keys(edits).length ? edits : null);
}

/** A worktree's folder moved: its layout, unsent commit message and unsaved files, kept by path, go along. */
export function moveRoot(from: string, to: string) {
  for (const key of [KEY, DRAFTS_KEY, EDITS_KEY]) {
    const saved = all(key)[from];
    if (saved === undefined) continue;
    put(key, from, null);
    put(key, to, saved);
  }
}

/** The folder the project `main` puts new worktrees in, when it isn't the default one beside it. */
export function loadWorktreeDir(main: string): string | null {
  const d = all(WORKTREE_DIRS_KEY)[main];
  return typeof d === "string" ? d : null;
}

/** null goes back to the default folder. */
export function saveWorktreeDir(main: string, dir: string | null) {
  put(WORKTREE_DIRS_KEY, main, dir);
}

/** The project's own subfolder of the worktree folder set in Settings, or null while that's off. */
export function sharedWorktreeDir(main: string): string | null {
  const root = getSettings().worktreeRoot;
  return root ? joinPath(root, folderName(main)) : null;
}

/** Where `main`'s next worktree goes; null is `<project>.worktrees` beside it. */
export const worktreeDir = (main: string) => loadWorktreeDir(main) ?? sharedWorktreeDir(main);
