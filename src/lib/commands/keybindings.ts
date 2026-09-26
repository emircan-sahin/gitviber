import { useEffect, useRef } from "react";
import { bindingsFor, type Command, type CommandId, commandFor, eventChords, formatChord, runsInTerminal, runsWhileTyping, takenFromTerminal } from "./commands";
import { getSettings, useSettings } from "../settings";

export { bindingsFor, COMMANDS, type Command, type CommandId, eventChord, formatChord, isReserved } from "./commands";

/** The first binding of a command, formatted for a tooltip; follows the user's overrides. */
export function useShortcut(id: CommandId): string | undefined {
  const { keybindings } = useSettings();
  const first = bindingsFor(id, keybindings)[0];
  return first ? formatChord(first) : undefined;
}

/** For local handlers (e.g. ⌘↵ in the commit box) that still honour the user's binding. */
export function matchesCommand(id: CommandId, e: KeyboardEvent): boolean {
  const keys = bindingsFor(id, getSettings().keybindings);
  return eventChords(e).some((c) => keys.includes(c));
}

/** Which of these commands a key runs, reading the key once: the terminal asks on every keystroke. */
export function commandIn<T extends CommandId>(ids: readonly T[], e: KeyboardEvent): T | undefined {
  const chords = eventChords(e);
  if (!chords.length) return undefined;
  const overrides = getSettings().keybindings;
  return ids.find((id) => bindingsFor(id, overrides).some((k) => chords.includes(k)));
}

/** The global command a key runs, and the chord it's bound under (see eventChords). */
function commandOf(e: KeyboardEvent) {
  const overrides = getSettings().keybindings;
  for (const chord of eventChords(e)) {
    const command = commandFor(chord, overrides);
    if (command) return { chord, command };
  }
  return null;
}

/** Where a key lands: a key event, or `{ target: document.activeElement }` for the next one. */
type At = { target: EventTarget | null };

/** Focus is in the code view: Monaco's text area, read-only, so not typing (its find box is, and so is a file open for editing). */
const inCodeView = (e: At) => e.target instanceof HTMLElement && e.target.matches(".monaco-editor textarea.inputarea") && !e.target.closest("[data-editable]");

/** Focus is somewhere that owns its keystrokes: text fields, menus, dialogs, pickers (not the sidebar lists, see useListNav). */
export function isTyping(e: At) {
  const el = e.target instanceof HTMLElement ? e.target : null;
  return !!el && !inCodeView(e) && (el.isContentEditable || !!el.closest("input,textarea,select,[role=menu],[role=listbox]:not([data-list-nav]),[role=dialog]"));
}

/** Menu bar items that aren't key commands (lib.rs `menu`); they run through the same handlers. */
export const MENU_ACTIONS = [
  "app.about",
  "app.checkForUpdates",
  "app.installCli",
  "help.readme",
  "help.shortcuts",
  "help.reportBug",
  "help.copyDiagnostics",
  "help.showLogs",
  "help.releaseNotes",
  "help.license",
] as const;
export type Action = CommandId | (typeof MENU_ACTIONS)[number];

/** How the command palette lists them. */
export const MENU_ACTION_INFO: Record<(typeof MENU_ACTIONS)[number], { title: string; category: string }> = {
  "app.about": { title: "About GitViber", category: "Help" },
  "app.checkForUpdates": { title: "Check for Updates", category: "Help" },
  "app.installCli": { title: "Install 'gitviber' Command in PATH", category: "Help" },
  "help.readme": { title: "GitViber Help", category: "Help" },
  "help.shortcuts": { title: "Keyboard Shortcuts", category: "Help" },
  "help.reportBug": { title: "Report a Bug", category: "Help" },
  "help.copyDiagnostics": { title: "Copy Diagnostics", category: "Help" },
  "help.showLogs": { title: "Show Logs", category: "Help" },
  "help.releaseNotes": { title: "Release Notes", category: "Help" },
  "help.license": { title: "View License", category: "Help" },
};

const MODAL_SAFE: Action[] = ["workbench.openSettings", "workbench.shortcutOverlay", "window.reload", "view.zoomIn", "view.zoomOut", "view.zoomReset", "app.about", "app.checkForUpdates", "app.installCli", "help.readme", "help.shortcuts", "help.reportBug", "help.copyDiagnostics", "help.showLogs", "help.releaseNotes", "help.license"];

// Last registered wins, so a nested view can take a command over while it's mounted.
const handlers = new Map<Action, (() => void)[]>();
const changeListeners = new Set<() => void>();

/** Called when a command gains or loses its handler, which is what greys it out in the menu. */
export function onHandlersChange(l: () => void) {
  changeListeners.add(l);
}

export const hasHandler = (id: Action) => !!handlers.get(id)?.length;

/** Whether a command's key would run it now: it has a handler, and no modal dialog stands in the way. */
export const canRun = (id: Action) => !!handlerFor(id);

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

/**
 * Whether a chord runs its command where it's typed. Text owns most keys (see runsWhileTyping), and
 * menus and dialogs theirs, the same way. The terminal passes on what it doesn't read without saying
 * so (xterm leaves the event alone).
 */
export function runsAt(e: At, chord: string, command: Command) {
  const inTerminal = e.target instanceof HTMLElement && !!e.target.closest(".xterm");
  return inTerminal ? runsInTerminal(chord, command) : !isTyping(e) || runsWhileTyping(chord, command);
}

function dispatch(e: KeyboardEvent) {
  if (e.isComposing) return;
  const chords = eventChords(e);
  // A focused widget that handled the key (e.g. the shortcut recorder) prevents default.
  const handled = e.defaultPrevented;
  // Whatever they're bound to, ⌘W would close the window and ⌘R reload the webview.
  if (chords[0] === "cmd+w" || chords[0] === "cmd+r") e.preventDefault();
  if (handled) return;
  const found = commandOf(e);
  if (!found || !runsAt(e, found.chord, found.command)) return;
  const run = handlerFor(found.command.id);
  if (!run) return;
  e.preventDefault();
  // Keep it from Monaco too, which would take F7 for its own diff navigation and ⌘Z as undo.
  e.stopPropagation();
  if (!(e.repeat && "noRepeat" in found.command)) run();
}

// Clicking into the code view focuses Monaco's text area, which sees keys before the window does:
// there the app's commands go first (on the way down), and Monaco keeps the rest (copy, find, arrows).
window.addEventListener("keydown", (e) => inCodeView(e) && dispatch(e), { capture: true });
window.addEventListener("keydown", (e) => !inCodeView(e) && dispatch(e));

/** A Ctrl chord of the app's (⌃Tab, ⌃1) that the terminal must not turn into a control code for the shell. */
export function appTakesFromTerminal(e: KeyboardEvent) {
  const found = e.ctrlKey ? commandOf(e) : null;
  return !!found && takenFromTerminal(found.chord, found.command) && hasHandler(found.command.id);
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
