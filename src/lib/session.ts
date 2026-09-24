import { type Selection, selectionKey } from "./selection";

/** What a worktree's window looked like, so reopening the app picks up where it was. */
export interface WorkspaceSnapshot {
  tabs: { key: string; sel: Selection; preview: boolean }[];
  active: string | null;
  listTab: string;
  /** Viewed marks: `kind:path` → the file's content signature when it was marked. */
  viewed: [string, string][];
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
const WORKTREE_DIRS_KEY = "gitviber.worktreeDirs";
// Agent worktrees come and go; keep only the most recently used.
const MAX = 30;

function all(key: string): Record<string, unknown> {
  try {
    const v: unknown = JSON.parse(localStorage.getItem(key) ?? "{}");
    return v && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, unknown>) : {};
  } catch {
    return {};
  }
}

/** Stores `value` under `root` (null removes it), keeping the MAX most recently saved roots. */
function put(key: string, root: string, value: unknown) {
  const { [root]: _, ...rest } = all(key);
  // Insertion order is recency: the one saved now goes last, the oldest drop off the front.
  const entries = [...Object.entries(rest), ...(value === null ? [] : [[root, value] as const])].slice(-MAX);
  try {
    localStorage.setItem(key, JSON.stringify(Object.fromEntries(entries)));
  } catch {
    // Not critical.
  }
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

/** The folder the project `main` puts new worktrees in, when it isn't the default one beside it. */
export function loadWorktreeDir(main: string): string | null {
  const d = all(WORKTREE_DIRS_KEY)[main];
  return typeof d === "string" ? d : null;
}

/** null goes back to the default folder. */
export function saveWorktreeDir(main: string, dir: string | null) {
  put(WORKTREE_DIRS_KEY, main, dir);
}
