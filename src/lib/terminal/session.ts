import { readJson } from "../storage";
import { dueForSave, SAVE_MS } from "./saveRound";
import { type Layout, mapPanes, savedLayout } from "./layout";
import { createPane, focusActive, newId, panes, type Pane, set, shellDir, state } from "./terminals";

// The session save: where each shell was and what it printed, for the next run to restore.
// Imported through terminals.ts only: the two import each other, and it reads the save as it loads.

/** The terminals of a previous run: where each shell was, and what it had printed. */
export interface SavedSession {
  savedAt: number;
  active: number;
  /** `layout`'s leaves index `panes`; a save from before splits went down has none. */
  groups: { name?: string; focused: number; layout?: Layout; panes: { cwd: string; dir?: string; history: string }[] }[];
}

const SESSION_KEY = "gitviber.terminals";
// Serialized with colors, 1000 lines is ~100 KB a pane; localStorage holds a few MB.
const HISTORY_LINES = 1000;

export function loadSession(): SavedSession | null {
  const s = readJson<SavedSession | null>(SESSION_KEY, null);
  return s && Array.isArray(s.groups) && s.groups.length ? s : null;
}

let saveTimer: number | undefined;
/** Throttled rather than debounced, so a shell that never stops printing still gets saved. */
export function scheduleSave() {
  saveTimer ??= window.setTimeout(async () => {
    const due = dueForSave(panes.values(), Date.now(), false);
    // Where their shells are now, asked before they're saved (a reload's save keeps the last answer).
    await Promise.all(due.map(shellDir));
    saveTimer = undefined;
    saveSession(false, due.filter((p) => panes.has(p.id)));
  }, SAVE_MS);
}

// Out of sight (a reload, a quit (lib/app/quit), the window hidden) a stall goes unseen: every changed pane is saved.
window.addEventListener("pagehide", () => saveSession(true));
document.addEventListener("visibilitychange", () => document.hidden && saveSession(true));

/** The layout last written, to skip a write that changes nothing (a title changing, a pane still printing). */
let written = "";

/** `due`: the panes whose history this round saves (dueForSave), when already picked. */
function saveSession(all = false, due?: Pane[]) {
  // Nothing opened yet: keep the last run's terminals for the restore offer.
  if (!state.groups.length && state.restorable) return;
  try {
    if (!state.groups.length) {
      written = "";
      return localStorage.removeItem(SESSION_KEY);
    }
    const now = Date.now();
    const saving = due ?? dueForSave(panes.values(), now, all);
    for (const p of saving) {
      // Alt-screen apps and terminal modes (mouse, bracketed paste) would leak into the new shell.
      p.saved = p.serialize.serialize({ scrollback: HISTORY_LINES, excludeAltBuffer: true, excludeModes: true });
      [p.dirty, p.serializedAt] = [false, now];
    }
    const snapshot = (history: boolean): SavedSession => ({
      savedAt: now,
      active: Math.max(0, state.groups.findIndex((g) => g.id === state.active)),
      groups: state.groups.map((g) => ({
        name: g.name,
        focused: Math.max(0, g.panes.findIndex((p) => p.id === g.focused)),
        layout: mapPanes(g.layout, (id) => g.panes.findIndex((p) => p.id === id)),
        panes: g.panes.map(({ id, cwd }) => {
          const dir = panes.get(id)?.dir;
          return { cwd, dir: dir !== cwd ? dir : undefined, history: (history && panes.get(id)?.saved) || "" };
        }),
      })),
    });
    const layout = JSON.stringify({ ...snapshot(false), savedAt: 0 });
    if (saving.length || layout !== written) {
      try {
        localStorage.setItem(SESSION_KEY, JSON.stringify(snapshot(true)));
      } catch {
        // Over quota: the layout alone is still worth keeping.
        localStorage.setItem(SESSION_KEY, JSON.stringify(snapshot(false)));
      }
      written = layout;
    }
    // Another round for panes left for later, until each is saved.
    if ([...panes.values()].some((p) => p.dirty)) scheduleSave();
  } catch {
    // Not critical.
  }
}

/** Reopens last run's terminals beside any opened since: same folders, their output, new shells. */
export function restoreSession() {
  const saved = state.restorable;
  if (!saved) return;
  // A tab saved with no panes (by hand, or a bug) has nothing to reopen, and left the restore with no tab to show.
  const restored = saved.groups.map((g) => {
    if (!g.panes?.length) return null;
    const infos = g.panes.map((p) => createPane(p.cwd, { history: p.history, savedAt: saved.savedAt }, typeof p.dir === "string" ? p.dir : undefined));
    const layout = mapPanes(savedLayout(g.layout, infos.length), (i) => infos[i].id);
    return { id: newId(), name: typeof g.name === "string" ? g.name : undefined, panes: infos, layout, focused: (infos[g.focused] ?? infos[0]).id };
  });
  const groups = restored.filter((g) => g !== null);
  if (!groups.length) return set({ restorable: null });
  set({ open: true, groups: [...state.groups, ...groups], active: (restored[saved.active] ?? groups[0]).id, restorable: null });
  focusActive();
}

export function dismissRestore() {
  set({ restorable: null });
}
