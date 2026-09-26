import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";
import { bindingsFor, COMMANDS, eventChord, isCommandId, menuAccelerator } from "./commands";
import { type Action, hasHandler, MENU_ACTIONS, onHandlersChange, runCommand } from "./keybindings";
import { lastOpenApp, subscribeOpenApps } from "../app/openIn";
import { getSettings, type Settings, subscribeSettings } from "../settings";
import { folderName } from "../path";
import { api } from "../api";
import { IS_MAC } from "../platform";

/**
 * The menu bar (menu.rs) runs the same commands as the keyboard. It learns from here which
 * ones apply right now and each one's shortcut, following the user's bindings.
 */

const ITEMS: Action[] = [...COMMANDS.map((c) => c.id), ...MENU_ACTIONS];

const CHECKED: Partial<Record<Action, (s: Settings) => boolean>> = {
  "diff.toggleSplit": (s) => s.sideBySide,
  "diff.toggleCollapse": (s) => s.hideUnchanged,
  "diff.toggleWhitespace": (s) => s.ignoreWhitespace,
  "editor.toggleWrap": (s) => s.wordWrap,
  "editor.toggleBlame": (s) => s.blame,
};

const TEXT: Partial<Record<Action, (s: Settings) => string>> = {
  "file.openIn": (s) => {
    const app = lastOpenApp(s);
    return app ? `Open in ${app.name}` : "Open in…";
  },
};

let recent: { list: string[]; open: (path: string) => void; clear: () => void } | null = null;

// Only what changed is sent: each item update is a call into AppKit.
const sent = new Map<Action, string>();
let sentRecent = "";
let queued = false;

function send() {
  queued = false;
  const s = getSettings();
  const items: Record<string, unknown> = {};
  for (const id of ITEMS) {
    const chord = isCommandId(id) ? bindingsFor(id, s.keybindings)[0] : undefined;
    const state = JSON.stringify({
      enabled: hasHandler(id),
      // Off macOS a menu's key fires before the page sees it (GTK runs accelerators ahead of the
      // focused widget): Ctrl+R in a terminal reloaded the window, "s" in a text field staged.
      // The page's own keydown runs every command anyway, so only macOS's menu gets the keys.
      accelerator: IS_MAC && chord ? menuAccelerator(chord) : null,
      checked: CHECKED[id]?.(s),
      text: TEXT[id]?.(s),
    });
    if (sent.get(id) === state) continue;
    sent.set(id, state);
    items[id] = JSON.parse(state);
  }
  // Two folders of the same name show their paths instead.
  const list = recent?.list ?? [];
  const names = list.map(folderName);
  const shown = JSON.stringify(list.map((path, i) => ({ path, title: names.indexOf(names[i]) === names.lastIndexOf(names[i]) ? names[i] : path })));
  const changedRecent = shown !== sentRecent;
  sentRecent = shown;
  if (!Object.keys(items).length && !changedRecent) return;
  api.setMenu(items, changedRecent ? JSON.parse(shown) : null).catch(() => {});
}

function update() {
  if (queued) return;
  queued = true;
  // A render mounts and unmounts many handlers at once; send the result once.
  setTimeout(send);
}

// A key the page lets through reaches the menu as that item's key equivalent. The page has
// already decided the key does nothing here (typing, a local shortcut like ⌘↵ outside the
// commit message), so the menu mustn't run it anyway.
let passed: { e: KeyboardEvent; at: number } | null = null;
window.addEventListener("keydown", (e) => eventChord(e) && (passed = { e, at: performance.now() }), true);
const fromPassedKey = () => !!passed && !passed.e.defaultPrevented && performance.now() - passed.at < 500;

function onMenu(id: string) {
  if (fromPassedKey()) return;
  if (id.startsWith("recent:")) recent?.open(id.slice("recent:".length));
  else if (id === "recent.clear") recent?.clear();
  else if ((ITEMS as string[]).includes(id)) runCommand(id as Action);
}

try {
  listen<string>("menu", (e) => onMenu(e.payload)).catch(() => {});
} catch {
  // Not in Tauri (the browser-only dev fixture).
}
onHandlersChange(update);
subscribeSettings(update);
subscribeOpenApps(update);
update();

/** File → Open Recent: the projects list. */
export function useRecentMenu(list: string[], open: (path: string) => void, clear: () => void) {
  useEffect(() => {
    recent = { list, open, clear };
    update();
  }, [list, open, clear]);
}
