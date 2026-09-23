import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";
import { bindingsFor, COMMANDS, eventChord, isCommandId } from "./commands";
import { type Action, hasHandler, MENU_ACTIONS, onHandlersChange, runCommand } from "./keybindings";
import { getSettings, type Settings, subscribeSettings } from "./settings";
import { folderName } from "./worktrees";

/**
 * The menu bar (menu.rs) runs the same commands as the keyboard. It learns from here which
 * ones apply right now and each one's shortcut, following the user's bindings.
 */

const ITEMS: Action[] = [...COMMANDS.map((c) => c.id), ...MENU_ACTIONS];

// TerminalPanel's own keys, which aren't rebindable.
const FIXED_KEYS: Partial<Record<Action, string>> = { "terminal.toggle": "cmd+j" };

const CHECKED: Partial<Record<Action, (s: Settings) => boolean>> = {
  "diff.toggleSplit": (s) => s.sideBySide,
  "diff.toggleCollapse": (s) => s.hideUnchanged,
  "diff.toggleWhitespace": (s) => s.ignoreWhitespace,
  "editor.toggleWrap": (s) => s.wordWrap,
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
    const state = JSON.stringify({
      enabled: hasHandler(id),
      accelerator: (isCommandId(id) ? bindingsFor(id, s.keybindings)[0] : FIXED_KEYS[id]) ?? null,
      checked: CHECKED[id]?.(s),
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
  invoke("set_menu", { items, recent: changedRecent ? JSON.parse(shown) : null }).catch(() => {});
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
update();

/** File → Open Recent: the projects list. */
export function useRecentMenu(list: string[], open: (path: string) => void, clear: () => void) {
  useEffect(() => {
    recent = { list, open, clear };
    update();
  }, [list, open, clear]);
}
