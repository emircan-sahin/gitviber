/**
 * Every keyboard command in one place, plus the pure chord logic (no DOM or React, so it
 * runs under node:test). A binding is a canonical chord string such as "shift+cmd+e" or
 * "alt+down": modifiers in ⌃⌥⇧⌘ order, then the key. Defaults follow VS Code where it has
 * an equivalent. When two commands share a chord, the one listed first here wins.
 */
export const COMMANDS = [
  { id: "workbench.openSettings", title: "Open Settings", category: "General", keys: ["cmd+,"] },
  { id: "file.openRepo", title: "Open Repository", category: "General", keys: ["cmd+o"] },
  { id: "file.cloneRepo", title: "Clone Repository", category: "General", keys: [] },
  { id: "window.reload", title: "Reload Window", category: "General", keys: ["cmd+r"] },
  // The file watcher refreshes on its own; this is for changes it can't see.
  { id: "repo.refresh", title: "Refresh", category: "General", keys: [] },
  { id: "tab.close", title: "Close Tab", category: "General", keys: ["cmd+w"] },
  { id: "file.reveal", title: "Reveal in Finder", category: "General", keys: [] },
  { id: "view.changes", title: "Show Changes", category: "View", keys: ["cmd+1"] },
  { id: "view.history", title: "Show History", category: "View", keys: ["cmd+2"] },
  { id: "view.pulls", title: "Show Pull Requests", category: "View", keys: ["cmd+3"] },
  { id: "view.issues", title: "Show Issues", category: "View", keys: ["cmd+4"] },
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
  { id: "git.fetch", title: "Fetch", category: "Git", keys: [] },
  { id: "git.pull", title: "Pull", category: "Git", keys: [] },
  { id: "git.push", title: "Push", category: "Git", keys: [] },
  // Not while typing: text fields keep ⌘Z for their own undo.
  { id: "git.undo", title: "Undo Git Action", category: "Git", keys: ["cmd+z"], outsideText: true },
  { id: "git.redo", title: "Redo Git Action", category: "Git", keys: ["shift+cmd+z"], outsideText: true },
  // These act on the file open in the viewer. Discard asks first; ⌘⌫ stays line delete in text.
  { id: "git.toggleStage", title: "Stage / Unstage Changes", category: "Git", keys: ["s"] },
  { id: "git.discard", title: "Discard Changes", category: "Git", keys: ["cmd+backspace"], outsideText: true },
  // Local: only the commit message fields listen for it.
  { id: "git.commit", title: "Commit", category: "Git", keys: ["cmd+enter", "ctrl+enter"], local: true },
] as const satisfies readonly { id: string; title: string; category: string; keys: readonly string[]; local?: boolean; outsideText?: boolean }[];

export type CommandId = (typeof COMMANDS)[number]["id"];
export type Command = (typeof COMMANDS)[number];
export type Overrides = Record<string, string[]>;

export const isCommandId = (id: string): id is CommandId => COMMANDS.some((c) => c.id === id);

/** macOS handles these before the page sees them (quit, hide, minimize, cycle windows). */
export const RESERVED = ["cmd+q", "cmd+h", "alt+cmd+h", "cmd+m", "cmd+`"];

const MODS = ["ctrl", "alt", "shift", "cmd"] as const;

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

/** US names for physical keys, the last resort when ⌥ hid the character. */
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
  IntlBackslash: "§",
};

/** The parts of a KeyboardEvent used here (spelled out so tests need no DOM). */
export type KeyLike = { key: string; code: string; altKey: boolean; shiftKey: boolean; metaKey: boolean; ctrlKey: boolean };

// What each physical key types on the user's layout, learned from plain key presses. macOS
// turns ⌥+key into a symbol (⌥S → ß, German ⌥Z → Ω), so ⌥ chords map back through this.
const layout = new Map<string, string>();

const printable = (k: string) => [...k].length === 1 && k !== " ";
// ⌥ output that is still plain ASCII (German ⌥L → @) is taken as typed.
const plainAscii = (k: string) => /^[\x21-\x7e]$/.test(k);

function keyToken(e: KeyLike): string | null {
  if (NAMED_KEYS[e.key]) return NAMED_KEYS[e.key];
  if (/^F\d{1,2}$/.test(e.key)) return e.key.toLowerCase();
  if (!e.altKey && printable(e.key)) {
    const k = e.key.toLowerCase();
    if (!e.shiftKey) layout.set(e.code, k);
    return k;
  }
  if (plainAscii(e.key)) return e.key.toLowerCase();
  // ⌥ produced a symbol or a dead key: fall back to what that key types without ⌥.
  if (layout.has(e.code)) return layout.get(e.code)!;
  if (/^Key[A-Z]$/.test(e.code)) return e.code.slice(3).toLowerCase();
  if (/^Digit\d$/.test(e.code)) return e.code.slice(5);
  return CODE_KEYS[e.code] ?? null;
}

/** The canonical chord for a key event, or null for a lone modifier or an unknown key. */
export function eventChord(e: KeyLike): string | null {
  if (["Meta", "Control", "Alt", "Shift", "CapsLock", "Fn"].includes(e.key)) return null;
  const key = keyToken(e);
  if (!key) return null;
  const mods = MODS.filter((m) => (m === "cmd" ? e.metaKey : m === "ctrl" ? e.ctrlKey : m === "alt" ? e.altKey : e.shiftKey));
  return [...mods, key].join("+");
}

/** Canonical form of a stored chord, or null when it has unknown or repeated modifiers. */
export function canonical(chord: string): string | null {
  const parts = chord.toLowerCase().split("+");
  const key = parts.pop();
  if (!key || parts.some((p) => !(MODS as readonly string[]).includes(p)) || new Set(parts).size !== parts.length) return null;
  return [...MODS.filter((m) => parts.includes(m)), key].join("+");
}

/** Keeps only overrides of known commands whose chords all parse; the rest fall back to defaults. */
export function cleanOverrides(raw: unknown): Overrides {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return {};
  const out: Overrides = {};
  for (const [id, keys] of Object.entries(raw)) {
    if (!isCommandId(id) || !Array.isArray(keys)) continue;
    const parsed = keys.map((k) => (typeof k === "string" ? canonical(k) : null));
    if (parsed.every((k) => k !== null)) out[id] = parsed as string[];
  }
  return out;
}

export function bindingsFor(id: CommandId, overrides: Overrides): readonly string[] {
  return overrides[id] ?? COMMANDS.find((c) => c.id === id)!.keys;
}

/** The global command a chord runs: the first listed one bound to it. */
export function commandFor(chord: string, overrides: Overrides): Command | undefined {
  return COMMANDS.find((c) => !("local" in c) && bindingsFor(c.id, overrides).includes(chord));
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
