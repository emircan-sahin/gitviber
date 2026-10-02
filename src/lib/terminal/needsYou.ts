import { listen } from "@tauri-apps/api/event";
import { useMemo, useSyncExternalStore } from "react";
import { notifyIfAway } from "../app/notify";
import type { NotifyEvent } from "../settings";
import { folderName } from "../path";
import { kittyNotes, type Note, osc777Note, osc9Note } from "./attention";
import { panes, type Pane, revealPane, state, subscribe, update } from "./terminals";

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

// Panes whose mark has been told to the OS. Apart from the mark: with one kind of notification
// turned off, an agent's own escape could mark the pane first and keep its state file's news quiet.
const told = new Set<number>();

/**
 * Marks `p` (and tells the OS, when away and `event` is on) unless it's being looked at: once until
 * it is, so an agent's own escape and its state file saying the same thing are one mark, one notification.
 */
export function needsYou(p: Pane, note?: Note, event: NotifyEvent = "notifyTerminal") {
  const g = state.groups.find((x) => x.panes.some((i) => i.id === p.id));
  const info = g?.panes.find((i) => i.id === p.id);
  if (!g || !info || told.has(p.id) || (document.hasFocus() && document.activeElement === p.term.textarea)) return;
  if (!info.needsYou) update(p.id, (i) => ({ ...i, needsYou: true }));
  const text = [note?.title, note?.body].filter(Boolean).join(": ");
  if (notifyIfAway(event, g.name ?? (folderName(p.cwd) || p.cwd), text || info.title || "Needs you", `pane:${p.id}`)) told.add(p.id);
}

export function lookedAt(id: number) {
  told.delete(id);
  if (state.groups.some((g) => g.panes.some((p) => p.id === id && p.needsYou))) update(id, (p) => ({ ...p, needsYou: false }));
}

// A click on a pane's notification (notifications.rs has brought the app to the front) shows it.
try {
  listen<string | null>("notification-click", ({ payload }) => {
    const pane = /^pane:(\d+)$/.exec(payload ?? "");
    if (pane) revealPane(Number(pane[1]));
  }).catch(() => {});
} catch {
  // Not in Tauri (the browser-only dev fixture).
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
