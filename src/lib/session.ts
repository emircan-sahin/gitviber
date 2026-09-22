import { type Selection, selectionKey } from "./selection";

/** What a worktree's window looked like, so reopening the app picks up where it was. */
export interface WorkspaceSnapshot {
  tabs: { key: string; sel: Selection; preview: boolean }[];
  active: string | null;
  listTab: string;
  /** Viewed marks: `kind:path` → the file's content signature when it was marked. */
  viewed: [string, string][];
}

const KEY = "gitviber.workspaces";
// Agent worktrees come and go; keep only the most recently used.
const MAX = 30;

function all(): Record<string, WorkspaceSnapshot> {
  try {
    const v: unknown = JSON.parse(localStorage.getItem(KEY) ?? "{}");
    return v && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, WorkspaceSnapshot>) : {};
  } catch {
    return {};
  }
}

export function loadWorkspace(root: string): WorkspaceSnapshot | null {
  const s = all()[root];
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
  const { [root]: _, ...rest } = all();
  // Insertion order is recency: the one saved now goes last, the oldest drop off the front.
  const entries = [...Object.entries(rest), [root, snapshot] as const].slice(-MAX);
  try {
    localStorage.setItem(KEY, JSON.stringify(Object.fromEntries(entries)));
  } catch {
    // Not critical.
  }
}
