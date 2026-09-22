import { useEffect, useRef } from "react";
import { bindingsFor, type CommandId, commandFor, eventChord, formatChord } from "./commands";
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

/** Focus is somewhere that owns its keystrokes: text fields, menus, dialogs, pickers. */
export function isTyping(e: KeyboardEvent) {
  const el = e.target instanceof HTMLElement ? e.target : null;
  return !!el && (el.isContentEditable || !!el.closest("input,textarea,select,[role=menu],[role=listbox],[role=dialog]"));
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

window.addEventListener("keydown", (e) => {
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
  // ⌘ shortcuts work everywhere but where text owns them (⌘Z); the rest only when not
  // typing, since ⌥+letter types characters (ç, ß), plain letters are text, and
  // menus/dialogs own their own keys.
  if ((!chord.includes("cmd") || "outsideText" in command) && isTyping(e)) return;
  const run = handlerFor(command.id);
  if (!run) return;
  e.preventDefault();
  run();
});

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
