import { useMemo, useSyncExternalStore } from "react";
import { notifyIfAway } from "../app/notify";
import { folderName } from "../path";
import { kittyNotes, type Note, osc777Note, osc9Note } from "./attention";
import { panes, type Pane, state, subscribe, update } from "./terminals";

/**
 * A bell, or a notification escape (OSC 9, 777, 99: Claude Code, Codex), from a pane not being
 * looked at marks it, its tab and its worktree, and tells the OS when the app is in the background
 * and the user turned that on. Once until it's looked at: a program ringing on and on is one mark.
 */
export function watchAttention(p: Pane) {
  p.term.onBell(() => needsYou(p));
  const readers: [number, (data: string) => Note | null][] = [[9, osc9Note], [777, osc777Note], [99, kittyNotes()]];
  for (const [code, read] of readers)
    p.term.parser.registerOscHandler(code, (data) => {
      const note = read(data);
      if (note) needsYou(p, note);
      // Not a notification (OSC 9;4 is a progress bar): left to any other handler.
      return !!note;
    });
}

/** Marks `p` (and tells the OS, when away) unless it's being looked at or already marked: an agent's own escape and its state file saying the same thing are one mark. */
export function needsYou(p: Pane, note?: Note) {
  const g = state.groups.find((x) => x.panes.some((i) => i.id === p.id));
  const info = g?.panes.find((i) => i.id === p.id);
  if (!g || !info || info.needsYou || (document.hasFocus() && document.activeElement === p.term.textarea)) return;
  update(p.id, (i) => ({ ...i, needsYou: true }));
  const text = [note?.title, note?.body].filter(Boolean).join(": ");
  notifyIfAway(g.name ?? (folderName(p.cwd) || p.cwd), text || info.title || "Needs you");
}

export function lookedAt(id: number) {
  if (state.groups.some((g) => g.panes.some((p) => p.id === id && p.needsYou))) update(id, (p) => ({ ...p, needsYou: false }));
}

// Back in the window, the pane that has the keys is looked at again.
window.addEventListener("focus", () => {
  for (const p of panes.values()) if (document.activeElement === p.term.textarea) lookedAt(p.id);
});

/** Folders of the panes that need the user, "\0"-joined: a string, so a hook re-renders only when it changes. */
const needing = () =>
  state.groups
    .flatMap((g) => g.panes.filter((p) => p.needsYou).map((p) => p.cwd))
    .sort()
    .join("\0");

/** The folders of panes that need the user (needsYou), for the worktree picker's marks. */
export function useNeedsYou() {
  const key = useSyncExternalStore(subscribe, needing);
  return useMemo(() => (key ? key.split("\0") : []), [key]);
}
