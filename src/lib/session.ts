import type { Selection } from "./selection";

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
  return { ...s, tabs: s.tabs.filter((t) => typeof t?.key === "string" && typeof t.sel?.kind === "string") };
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
