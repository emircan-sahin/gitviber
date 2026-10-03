import { listen } from "@tauri-apps/api/event";
import { useMemo, useSyncExternalStore } from "react";
import { notifyIfAway } from "../app/notify";
import { getSettings, type NotifyEvent } from "../settings";
import { folderName } from "../path";
import { kittyNotes, type Note, osc777Note, osc9Note } from "./attention";
import { type CommandEnd, endText } from "./commandMarks";
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
// turned off, a terminal's escape could mark the pane first and keep the agent's news quiet.
const told = new Set<number>();
// Pane ids start over each run: a click on an older run's notification, still in Notification
// Center, would show some other pane.
const RUN = Math.random().toString(36).slice(2, 10);

/**
 * Marks `p` (and tells the OS, when away and `event` is on) unless it's being looked at: once until
 * it is, so an agent's own escape and its state file saying the same thing are one mark, one
 * notification. Without `event` it's the terminal's bell or escape, which only marks a pane whose
 * agent reports its state: agents.ts tells that one as finished or waiting, under its own switch.
 */
export function needsYou(p: Pane, note?: Note, event?: NotifyEvent) {
  const g = state.groups.find((x) => x.panes.some((i) => i.id === p.id));
  const info = g?.panes.find((i) => i.id === p.id);
  if (!g || !info || told.has(p.id) || (document.hasFocus() && document.activeElement === p.term.textarea)) return;
  if (!info.needsYou) update(p.id, (i) => ({ ...i, needsYou: true }));
  if (!event && info.agent?.state != null) return;
  const text = [note?.title, note?.body].filter(Boolean).join(": ");
  const title = g.name ?? (folderName(p.cwd) || p.cwd);
  if (notifyIfAway(event ?? "notifyTerminal", title, text || info.title || "Needs you", `pane:${RUN}:${p.id}`)) told.add(p.id);
}

/**
 * A command that ran past the setting's threshold and ended out of sight marks its pane, and tells
 * the OS when the app is in the background: Ghostty's notify-on-command-finish. A pane on screen in
 * the app in front is seen, focused or not. An agent's pane has its own news (agents.ts).
 */
export function commandEnded(p: Pane, end: CommandEnd) {
  const s = getSettings();
  if (!s.notifyLongCommand || end.ms <= s.longCommandSeconds * 1000) return;
  if (state.groups.some((g) => g.panes.some((i) => i.id === p.id && i.agent))) return;
  const onScreen = p.host.isConnected && p.host.clientWidth > 0;
  if (document.hasFocus() && onScreen) return;
  needsYou(p, { body: endText(end) }, "notifyLongCommand");
}

export function lookedAt(id: number) {
  told.delete(id);
  if (state.groups.some((g) => g.panes.some((p) => p.id === id && p.needsYou))) update(id, (p) => ({ ...p, needsYou: false }));
}

// A click on a pane's notification (notifications.rs has brought the app to the front) shows it.
try {
  listen<string | null>("notification-click", ({ payload }) => {
    const pane = /^pane:(\w+):(\d+)$/.exec(payload ?? "");
    if (pane?.[1] === RUN) revealPane(Number(pane[2]));
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
