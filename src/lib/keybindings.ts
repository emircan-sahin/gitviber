import { useEffect, useRef } from "react";
import { bindingsFor, type CommandId, commandFor, eventChord, formatChord, runsInTerminal, runsWhileTyping, takenFromTerminal } from "./commands";
import { getSettings, useSettings } from "./settings";

export { bindingsFor, COMMANDS, type Command, type CommandId, eventChord, formatChord, RESERVED } from "./commands";

/** The first binding of a command, formatted for a tooltip; follows the user's overrides. */
export function useShortcut(id: CommandId): string | undefined {
  const { keybindings } = useSettings();
  const first = bindingsFor(id, keybindings)[0];
  return first ? formatChord(first) : undefined;
}

/** For local handlers (e.g. ⌘↵ in the commit box) that still honour the user's binding. */
export function matchesCommand(id: CommandId, e: KeyboardEvent): boolean {
  const chord = eventChord(e);
  return !!chord && bindingsFor(id, getSettings().keybindings).includes(chord);
}

/** Focus is in the code view: Monaco's text area, read-only, so not typing (its find box is). */
const inCodeView = (e: KeyboardEvent) => e.target instanceof HTMLElement && e.target.matches(".monaco-editor textarea.inputarea");

/** Focus is somewhere that owns its keystrokes: text fields, menus, dialogs, pickers (not the sidebar lists, see useListNav). */
export function isTyping(e: KeyboardEvent) {
  const el = e.target instanceof HTMLElement ? e.target : null;
  return !!el && !inCodeView(e) && (el.isContentEditable || !!el.closest("input,textarea,select,[role=menu],[role=listbox]:not([data-list-nav]),[role=dialog]"));
}

/** Menu bar items that aren't key commands (lib.rs `menu`); they run through the same handlers. */
export const MENU_ACTIONS = [
  "app.about",
  "terminal.new",
  "terminal.toggle",
  "help.readme",
  "help.shortcuts",
  "help.reportBug",
  "help.releaseNotes",
  "help.license",
] as const;
export type Action = CommandId | (typeof MENU_ACTIONS)[number];

/** How the command palette lists them; `key` is fixed (TerminalPanel's own ⌘J), not rebindable. */
export const MENU_ACTION_INFO: Record<(typeof MENU_ACTIONS)[number], { title: string; category: string; key?: string }> = {
  "app.about": { title: "About GitViber", category: "Help" },
  "terminal.new": { title: "New Terminal", category: "Terminal" },
  "terminal.toggle": { title: "Toggle Terminal", category: "Terminal", key: "cmd+j" },
  "help.readme": { title: "GitViber Help", category: "Help" },
  "help.shortcuts": { title: "Keyboard Shortcuts", category: "Help" },
  "help.reportBug": { title: "Report a Bug", category: "Help" },
  "help.releaseNotes": { title: "Release Notes", category: "Help" },
  "help.license": { title: "View License", category: "Help" },
};

const MODAL_SAFE: Action[] = ["workbench.openSettings", "window.reload", "view.zoomIn", "view.zoomOut", "view.zoomReset", "app.about", "help.readme", "help.shortcuts", "help.reportBug", "help.releaseNotes", "help.license"];

// Last registered wins, so a nested view can take a command over while it's mounted.
const handlers = new Map<Action, (() => void)[]>();
const changeListeners = new Set<() => void>();

/** Called when a command gains or loses its handler, which is what greys it out in the menu. */
export function onHandlersChange(l: () => void) {
  changeListeners.add(l);
}

export const hasHandler = (id: Action) => !!handlers.get(id)?.length;

/** The handler a command runs now, if any; none while a modal dialog hides the workspace it acts on. */
function handlerFor(id: Action) {
  // Nothing may act on the workspace hidden behind a modal dialog; zoom only rescales it.
  if (document.querySelector("[data-modal]") && !MODAL_SAFE.includes(id)) return null;
  return handlers.get(id)?.at(-1) ?? null;
}

/** For the menu bar: runs a command as its key would, minus the checks on where the key was typed. */
export function runCommand(id: Action) {
  handlerFor(id)?.();
}

function dispatch(e: KeyboardEvent) {
  if (e.isComposing) return;
  const chord = eventChord(e);
  if (!chord) return;
  // A focused widget that handled the key (e.g. the shortcut recorder) prevents default.
  const handled = e.defaultPrevented;
  // Whatever they're bound to, ⌘W would close the window and ⌘R reload the webview.
  if (chord === "cmd+w" || chord === "cmd+r") e.preventDefault();
  if (handled) return;
  const command = commandFor(chord, getSettings().keybindings);
  if (!command) return;
  // Text owns most keys (see runsWhileTyping), and menus and dialogs theirs, the same way. The
  // terminal passes on what it doesn't read without saying so (xterm leaves the event alone).
  const inTerminal = e.target instanceof HTMLElement && !!e.target.closest(".xterm");
  if (inTerminal ? !runsInTerminal(chord, command) : isTyping(e) && !runsWhileTyping(chord, command)) return;
  const run = handlerFor(command.id);
  if (!run) return;
  e.preventDefault();
  // Keep it from Monaco too, which would take F7 for its own diff navigation and ⌘Z as undo.
  e.stopPropagation();
  run();
}

// Clicking into the code view focuses Monaco's text area, which sees keys before the window does:
// there the app's commands go first (on the way down), and Monaco keeps the rest (copy, find, arrows).
window.addEventListener("keydown", (e) => inCodeView(e) && dispatch(e), { capture: true });
window.addEventListener("keydown", (e) => !inCodeView(e) && dispatch(e));

/** A Ctrl chord of the app's (⌃Tab, ⌃1) that the terminal must not turn into a control code for the shell. */
export function appTakesFromTerminal(e: KeyboardEvent) {
  const chord = e.ctrlKey ? eventChord(e) : null;
  const command = chord && commandFor(chord, getSettings().keybindings);
  return !!command && takenFromTerminal(chord, command) && hasHandler(command.id);
}

/** Registers handlers for commands while the component is mounted; always calls the latest closures. A command left undefined is unavailable (greyed out in the menu). */
export function useCommands(map: Partial<Record<Action, () => void>>) {
  const ref = useRef(map);
  useEffect(() => {
    ref.current = map;
  });
  const ids = Object.keys(map)
    .filter((id) => map[id as Action])
    .sort()
    .join(" ");
  useEffect(() => {
    if (!ids) return;
    const entries = ids.split(" ").map((id) => {
      const fn = () => ref.current[id as Action]?.();
      const stack = handlers.get(id as Action) ?? [];
      handlers.set(id as Action, [...stack, fn]);
      return [id as Action, fn] as const;
    });
    changeListeners.forEach((l) => l());
    return () => {
      for (const [id, fn] of entries) handlers.set(id, (handlers.get(id) ?? []).filter((f) => f !== fn));
      changeListeners.forEach((l) => l());
    };
  }, [ids]);
}
