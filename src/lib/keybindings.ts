import { useEffect, useRef } from "react";
import { getSettings, type Settings, useSettings } from "./settings";

/**
 * Every keyboard command in one place. A binding is a canonical chord string such as
 * "shift+cmd+e" or "alt+down" (modifiers in ⌃⌥⇧⌘ order, then the key). Defaults follow
 * VS Code where it has an equivalent; users override per command in settings.
 */
export const COMMANDS = [
  { id: "workbench.openSettings", title: "Open Settings", category: "General", keys: ["cmd+,"] },
  { id: "file.openRepo", title: "Open Repository", category: "General", keys: ["cmd+o"] },
  { id: "repo.refresh", title: "Refresh", category: "General", keys: ["cmd+r"] },
  { id: "tab.close", title: "Close Tab", category: "General", keys: ["cmd+w"] },
  { id: "view.changes", title: "Show Changes", category: "View", keys: ["cmd+1"] },
  { id: "view.history", title: "Show History", category: "View", keys: ["cmd+2"] },
  { id: "view.pulls", title: "Show Pull Requests", category: "View", keys: ["cmd+3"] },
  { id: "view.toggleGitPanel", title: "Toggle Git Panel", category: "View", keys: ["cmd+b"] },
  { id: "view.toggleExplorer", title: "Toggle Explorer", category: "View", keys: ["alt+cmd+b"] },
  { id: "view.showExplorer", title: "Show Explorer", category: "View", keys: ["shift+cmd+e"] },
  { id: "view.zoomIn", title: "Zoom In", category: "View", keys: ["cmd+=", "shift+cmd+="] },
  { id: "view.zoomOut", title: "Zoom Out", category: "View", keys: ["cmd+-"] },
  { id: "view.zoomReset", title: "Reset Zoom", category: "View", keys: ["cmd+0"] },
  { id: "review.nextFile", title: "Next Changed File", category: "Review", keys: ["j"] },
  { id: "review.prevFile", title: "Previous Changed File", category: "Review", keys: ["k"] },
  { id: "review.toggleViewed", title: "Toggle File Viewed", category: "Review", keys: ["v"] },
  { id: "diff.nextChange", title: "Next Change", category: "Diff", keys: ["f7", "alt+down"] },
  { id: "diff.prevChange", title: "Previous Change", category: "Diff", keys: ["shift+f7", "alt+up"] },
  { id: "diff.toggleSplit", title: "Toggle Unified / Split", category: "Diff", keys: ["alt+s"] },
  { id: "diff.toggleCollapse", title: "Toggle Collapse Unchanged", category: "Diff", keys: ["alt+c"] },
  { id: "editor.toggleWrap", title: "Toggle Word Wrap", category: "Editor", keys: ["alt+z"] },
  { id: "editor.fontZoomIn", title: "Increase Code Font Size", category: "Editor", keys: ["alt+cmd+="] },
  { id: "editor.fontZoomOut", title: "Decrease Code Font Size", category: "Editor", keys: ["alt+cmd+-"] },
  { id: "editor.fontZoomReset", title: "Reset Code Font Size", category: "Editor", keys: ["alt+cmd+0"] },
  // Local: only the commit message fields listen for it.
  { id: "git.commit", title: "Commit", category: "Git", keys: ["cmd+enter", "ctrl+enter"], local: true },
] as const satisfies readonly { id: string; title: string; category: string; keys: readonly string[]; local?: boolean }[];

export type CommandId = (typeof COMMANDS)[number]["id"];
export type Command = (typeof COMMANDS)[number];

const MODS = ["ctrl", "alt", "shift", "cmd"] as const;

const CODE_KEYS: Record<string, string> = {
  Equal: "=",
  Minus: "-",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  BracketLeft: "[",
  BracketRight: "]",
  Backquote: "`",
};

const NAMED_KEYS: Record<string, string> = {
  ArrowUp: "up",
  ArrowDown: "down",
  ArrowLeft: "left",
  ArrowRight: "right",
  Enter: "enter",
  Escape: "escape",
  Tab: "tab",
  " ": "space",
  Backspace: "backspace",
  Delete: "delete",
  Home: "home",
  End: "end",
  PageUp: "pageup",
  PageDown: "pagedown",
  // "+" can't be a token (it's the separator); every layout's plus key means zoom in anyway.
  "+": "=",
};

function keyToken(e: KeyboardEvent): string | null {
  if (["Meta", "Control", "Alt", "Shift", "CapsLock", "Fn"].includes(e.key)) return null;
  if (NAMED_KEYS[e.key]) return NAMED_KEYS[e.key];
  if (/^F\d{1,2}$/.test(e.key)) return e.key.toLowerCase();
  // ⌥ turns letters into symbols on macOS (⌥B → ∫) and ⇧ shifts digits, so read the
  // physical key then; otherwise the character, which follows the user's layout.
  if (e.altKey || e.shiftKey) {
    if (/^Key[A-Z]$/.test(e.code)) return e.code.slice(3).toLowerCase();
    if (/^Digit\d$/.test(e.code)) return e.code.slice(5);
    if (CODE_KEYS[e.code]) return CODE_KEYS[e.code];
  }
  return e.key.length === 1 ? e.key.toLowerCase() : null;
}

/** The canonical chord for a key event, or null for a lone modifier. */
export function eventChord(e: KeyboardEvent): string | null {
  const key = keyToken(e);
  if (!key) return null;
  const mods = [e.ctrlKey && "ctrl", e.altKey && "alt", e.shiftKey && "shift", e.metaKey && "cmd"].filter(Boolean);
  return [...mods, key].join("+");
}

/** Canonical order, so "cmd+shift+e" typed by hand still matches. */
function canonical(chord: string): string {
  const parts = chord.toLowerCase().split("+");
  const key = parts.pop() ?? "";
  return [...MODS.filter((m) => parts.includes(m)), key].join("+");
}

export function bindingsFor(id: CommandId, overrides: Settings["keybindings"]): readonly string[] {
  const own = overrides[id];
  return own ? own.map(canonical) : COMMANDS.find((c) => c.id === id)!.keys;
}

const GLYPHS: Record<string, string> = {
  ctrl: "⌃",
  alt: "⌥",
  shift: "⇧",
  cmd: "⌘",
  up: "↑",
  down: "↓",
  left: "←",
  right: "→",
  enter: "↵",
  escape: "⎋",
  tab: "⇥",
  backspace: "⌫",
  delete: "⌦",
  space: "Space",
  pageup: "PgUp",
  pagedown: "PgDn",
  home: "Home",
  end: "End",
  "-": "−",
};

export function formatChord(chord: string): string {
  return chord
    .split("+")
    .map((p) => GLYPHS[p] ?? p.toUpperCase())
    .join("");
}

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

// Last registered wins, so a nested view can take a command over while it's mounted.
const handlers = new Map<CommandId, (() => void)[]>();
window.addEventListener("keydown", (e) => {
  // A focused widget that handled the key (e.g. the shortcut recorder) prevents default.
  if (e.defaultPrevented || e.isComposing) return;
  const chord = eventChord(e);
  if (!chord) return;
  // ⌘ shortcuts work everywhere; the rest only when not typing, since ⌥+letter types
  // characters (ç, ß), plain letters are text, and menus/dialogs own their own keys.
  if (!chord.includes("cmd") && isTyping(e)) return;
  for (const c of COMMANDS) {
    if ("local" in c || !bindingsFor(c.id, getSettings().keybindings).includes(chord)) continue;
    const stack = handlers.get(c.id);
    if (!stack?.length) continue;
    e.preventDefault();
    stack[stack.length - 1]();
    return;
  }
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
