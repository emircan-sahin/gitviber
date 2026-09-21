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

const MODAL_SAFE: CommandId[] = ["workbench.openSettings", "view.zoomIn", "view.zoomOut", "view.zoomReset"];

// Last registered wins, so a nested view can take a command over while it's mounted.
const handlers = new Map<CommandId, (() => void)[]>();

window.addEventListener("keydown", (e) => {
  if (e.isComposing) return;
  const chord = eventChord(e);
  if (!chord) return;
  // A focused widget that handled the key (e.g. the shortcut recorder) prevents default.
  const handled = e.defaultPrevented;
  // Whatever they're bound to, ⌘W would close the window and ⌘R reload the webview.
  if (chord === "cmd+w" || chord === "cmd+r") e.preventDefault();
  if (handled) return;
  // ⌘ shortcuts work everywhere; the rest only when not typing, since ⌥+letter types
  // characters (ç, ß), plain letters are text, and menus/dialogs own their own keys.
  if (!chord.includes("cmd") && isTyping(e)) return;
  const command = commandFor(chord, getSettings().keybindings);
  if (!command) return;
  // Nothing may act on the workspace hidden behind a modal dialog; zoom only rescales it.
  if (document.querySelector("[data-modal]") && !MODAL_SAFE.includes(command.id)) return;
  const stack = handlers.get(command.id);
  if (!stack?.length) return;
  e.preventDefault();
  stack[stack.length - 1]();
});

/** Registers handlers for commands while the component is mounted; always calls the latest closures. */
export function useCommands(map: Partial<Record<CommandId, () => void>>) {
  const ref = useRef(map);
  useEffect(() => {
    ref.current = map;
  });
  const ids = Object.keys(map).sort().join(" ");
  useEffect(() => {
    const entries = ids.split(" ").map((id) => {
      const fn = () => ref.current[id as CommandId]?.();
      const stack = handlers.get(id as CommandId) ?? [];
      handlers.set(id as CommandId, [...stack, fn]);
      return [id as CommandId, fn] as const;
    });
    return () => {
      for (const [id, fn] of entries) handlers.set(id, (handlers.get(id) ?? []).filter((f) => f !== fn));
    };
  }, [ids]);
}
