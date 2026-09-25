/**
 * Every keyboard command in one place, plus the pure chord logic (no DOM or React, so it
 * runs under node:test). A binding is a canonical chord string such as "shift+cmd+e" or
 * "alt+down": modifiers in ⌃⌥⇧⌘ order, then the key. Defaults follow VS Code where it has
 * an equivalent. When two commands share a chord, the one listed first here wins.
 * Off macOS "cmd" is Ctrl, as VS Code's CtrlCmd, and "ctrl" is the Windows / Super key.
 */

import { IS_MAC, IS_WINDOWS, REVEAL_LABEL } from "../platform.ts";

export const COMMANDS = [
  { id: "workbench.showCommands", title: "Command Palette", category: "General", keys: ["shift+cmd+p"] },
  { id: "workbench.quickOpen", title: "Open File", category: "General", keys: ["cmd+p"] },
  { id: "workbench.openChange", title: "Open Changed File", category: "General", keys: [] },
  { id: "workbench.openSettings", title: "Open Settings", category: "General", keys: ["cmd+,"] },
  // Holding ⌘ alone shows it too (ShortcutOverlay).
  { id: "workbench.shortcutOverlay", title: "Show Shortcut Overlay", category: "General", keys: ["cmd+/"] },
  { id: "file.openRepo", title: "Open Repository", category: "General", keys: ["cmd+o"] },
  { id: "file.cloneRepo", title: "Clone Repository", category: "General", keys: [] },
  { id: "file.switchProject", title: "Switch Project", category: "General", keys: [] },
  { id: "window.reload", title: "Reload Window", category: "General", keys: ["cmd+r"] },
  // The file watcher refreshes on its own; this is for changes it can't see.
  { id: "repo.refresh", title: "Refresh", category: "General", keys: [] },
  { id: "tab.close", title: "Close Tab", category: "General", keys: ["cmd+w"] },
  { id: "file.reveal", title: REVEAL_LABEL, category: "General", keys: [] },
  // The app last picked under "Open in"; the list of apps until there is one.
  { id: "file.openIn", title: "Open in External App", category: "General", keys: ["shift+cmd+o"] },
  // ⌘1–⌘9 pick tabs, as in browsers. Listed before the tabs, so a user who bound ⌘1 here keeps it.
  // Elsewhere "ctrl" is the Super key, which the OS takes; Alt+1–4 is the usual there.
  { id: "view.changes", title: "Focus Changes", category: "View", keys: ["ctrl+1"], keysOther: ["alt+1"] },
  { id: "view.history", title: "Focus History", category: "View", keys: ["ctrl+2"], keysOther: ["alt+2"] },
  { id: "view.pulls", title: "Focus Pull Requests", category: "View", keys: ["ctrl+3"], keysOther: ["alt+3"] },
  { id: "view.issues", title: "Focus Issues", category: "View", keys: ["ctrl+4"], keysOther: ["alt+4"] },
  // ⌘F is Find, which searches the history list too while focus is there.
  { id: "history.search", title: "Search History", category: "View", keys: ["alt+cmd+f"] },
  { id: "view.toggleGitPanel", title: "Toggle Git Panel", category: "View", keys: ["cmd+b"] },
  { id: "view.toggleExplorer", title: "Toggle Explorer", category: "View", keys: ["alt+cmd+b"] },
  { id: "view.focusGitPanel", title: "Focus Git Panel", category: "View", keys: ["shift+cmd+g"] },
  { id: "view.focusCode", title: "Focus Code View", category: "View", keys: ["cmd+e"] },
  // The id is from when it only showed the panel; kept so a user's binding for it still applies.
  { id: "view.showExplorer", title: "Focus Explorer", category: "View", keys: ["shift+cmd+e"] },
  { id: "view.focusNextPanel", title: "Focus Next Panel", category: "View", keys: ["f6"] },
  { id: "view.focusPrevPanel", title: "Focus Previous Panel", category: "View", keys: ["shift+f6"] },
  { id: "view.zoomIn", title: "Zoom In", category: "View", keys: ["cmd+=", "shift+cmd+="] },
  { id: "view.zoomOut", title: "Zoom Out", category: "View", keys: ["cmd+-"] },
  { id: "view.zoomReset", title: "Reset Zoom", category: "View", keys: ["cmd+0"] },
  { id: "tab.goto1", title: "Go to Tab 1", category: "Tabs", keys: ["cmd+1"] },
  { id: "tab.goto2", title: "Go to Tab 2", category: "Tabs", keys: ["cmd+2"] },
  { id: "tab.goto3", title: "Go to Tab 3", category: "Tabs", keys: ["cmd+3"] },
  { id: "tab.goto4", title: "Go to Tab 4", category: "Tabs", keys: ["cmd+4"] },
  { id: "tab.goto5", title: "Go to Tab 5", category: "Tabs", keys: ["cmd+5"] },
  { id: "tab.goto6", title: "Go to Tab 6", category: "Tabs", keys: ["cmd+6"] },
  // Elsewhere ⇧Ctrl+T opens a terminal (terminal.new): unbound there.
  { id: "tab.closeOthers", title: "Close Other Tabs", category: "Tabs", keys: ["alt+cmd+t"], keysOther: [] },
  { id: "tab.closeLeft", title: "Close Tabs to the Left", category: "Tabs", keys: [] },
  { id: "tab.closeRight", title: "Close Tabs to the Right", category: "Tabs", keys: [] },
  { id: "tab.closeAll", title: "Close All Tabs", category: "Tabs", keys: [] },
  { id: "tab.reopenClosed", title: "Reopen Closed Tab", category: "Tabs", keys: ["shift+cmd+t"], keysOther: [] },
  { id: "tab.goto7", title: "Go to Tab 7", category: "Tabs", keys: ["cmd+7"] },
  { id: "tab.goto8", title: "Go to Tab 8", category: "Tabs", keys: ["cmd+8"] },
  { id: "tab.last", title: "Go to Last Tab", category: "Tabs", keys: ["cmd+9"] },
  // ⌘←/⌘→ skip text fields and the terminal (see runsWhileTyping); ⇧⌘] shows in the menu as it works everywhere.
  // Elsewhere Ctrl+Tab ("cmd" there); Ctrl+← / Ctrl+→ move by word.
  { id: "tab.next", title: "Next Tab", category: "Tabs", keys: ["shift+cmd+]", "cmd+right", "ctrl+tab"], keysOther: ["cmd+tab", "shift+cmd+]"] },
  { id: "tab.prev", title: "Previous Tab", category: "Tabs", keys: ["shift+cmd+[", "cmd+left", "ctrl+shift+tab"], keysOther: ["shift+cmd+tab", "shift+cmd+["] },
  { id: "tab.moveLeft", title: "Move Tab Left", category: "Tabs", keys: ["alt+left"], local: "the tab strip" },
  { id: "tab.moveRight", title: "Move Tab Right", category: "Tabs", keys: ["alt+right"], local: "the tab strip" },
  { id: "review.nextFile", title: "Next Changed File", category: "Review", keys: ["j"] },
  { id: "review.prevFile", title: "Previous Changed File", category: "Review", keys: ["k"] },
  { id: "review.toggleViewed", title: "Toggle File Viewed", category: "Review", keys: ["v"] },
  { id: "diff.nextChange", title: "Next Change", category: "Diff", keys: ["f7", "alt+down"] },
  { id: "diff.prevChange", title: "Previous Change", category: "Diff", keys: ["shift+f7", "alt+up"] },
  // The selected lines, else the change at the cursor (which Next / Previous Change put there). VS Code
  // has ⌘K ⌘⌥S and ⌘K ⌘N: with no two-key chords here, their second key. Its ⌘K ⌘R is Monaco's ⌥⌘R
  // (Toggle Regex in Find) and ⌘R reloads, so discard is ⌘⌫ (the file's) with ⇧. Not from text fields.
  { id: "diff.stageChange", title: "Stage Change or Selected Lines", category: "Diff", keys: ["alt+cmd+s"], outsideText: true, noRepeat: true },
  { id: "diff.unstageChange", title: "Unstage Change or Selected Lines", category: "Diff", keys: ["alt+cmd+n"], outsideText: true, noRepeat: true },
  { id: "diff.discardChange", title: "Discard Change or Selected Lines", category: "Diff", keys: ["shift+cmd+backspace"], outsideText: true, noRepeat: true },
  { id: "diff.toggleSplit", title: "Toggle Unified / Split", category: "Diff", keys: ["alt+s"] },
  { id: "diff.toggleCollapse", title: "Toggle Collapse Unchanged", category: "Diff", keys: ["alt+c"] },
  { id: "diff.toggleWhitespace", title: "Toggle Ignore Whitespace", category: "Diff", keys: ["alt+w"] },
  { id: "editor.toggleWrap", title: "Toggle Word Wrap", category: "Editor", keys: ["alt+z"] },
  { id: "editor.toggleBlame", title: "Toggle Blame", category: "Editor", keys: [] },
  { id: "editor.fontZoomIn", title: "Increase Code Font Size", category: "Editor", keys: ["alt+cmd+="] },
  { id: "editor.fontZoomOut", title: "Decrease Code Font Size", category: "Editor", keys: ["alt+cmd+-"] },
  { id: "editor.fontZoomReset", title: "Reset Code Font Size", category: "Editor", keys: ["alt+cmd+0"] },
  // Searches where focus is (lib/ui/find). Monaco's own ⌘F is taken off (monaco.ts), so this is the only key for its find.
  { id: "editor.find", title: "Find", category: "General", keys: ["cmd+f"] },
  { id: "search.findInFiles", title: "Find in Files", category: "General", keys: ["shift+cmd+f"] },
  // Where the name, import or path under the cursor is defined; ⌘-click does the same (lib/editor/definitions).
  { id: "editor.goToDefinition", title: "Go to Definition", category: "Editor", keys: ["f12", "cmd+enter"], local: "the code view" },
  { id: "editor.peekDefinition", title: "Peek Definition", category: "Editor", keys: ["alt+f12"], local: "the code view" },
  // Where it's used; ⌘-click on a definition itself does the same.
  { id: "editor.goToReferences", title: "Go to References", category: "Editor", keys: ["shift+f12"], local: "the code view" },
  { id: "media.zoomIn", title: "Zoom In Image", category: "Editor", keys: ["=", "shift+="], local: "the image view" },
  { id: "media.zoomOut", title: "Zoom Out Image", category: "Editor", keys: ["-"], local: "the image view" },
  { id: "media.fit", title: "Fit Image", category: "Editor", keys: ["0"], local: "the image view" },
  // ⌃` as in VS Code, ⌘J as its panel toggle. Elsewhere Ctrl+`, as VS Code has it there too.
  { id: "terminal.toggle", title: "Toggle Terminal", category: "Terminal", keys: ["cmd+j", "ctrl+`"], keysOther: ["cmd+j", "cmd+`"] },
  // Off macOS Ctrl+Shift, as in Linux terminals: Ctrl+letter is the shell's and desktops take Super
  // chords (KDE: Super+D, Super+W). The ones below it only run in the terminal.
  { id: "terminal.new", title: "New Terminal", category: "Terminal", keys: ["cmd+t"], keysOther: ["shift+cmd+t"] },
  { id: "terminal.split", title: "Split Terminal", category: "Terminal", keys: ["cmd+d"], keysOther: ["shift+cmd+d"], local: "the terminal" },
  { id: "terminal.clear", title: "Clear Terminal", category: "Terminal", keys: ["cmd+k"], keysOther: ["shift+cmd+k"], local: "the terminal" },
  { id: "terminal.close", title: "Close Terminal Pane", category: "Terminal", keys: ["cmd+w"], keysOther: ["shift+cmd+w"], local: "the terminal" },
  { id: "terminal.prevPane", title: "Previous Terminal Pane", category: "Terminal", keys: ["alt+cmd+left"], keysOther: ["ctrl+alt+left"], local: "the terminal" },
  { id: "terminal.nextPane", title: "Next Terminal Pane", category: "Terminal", keys: ["alt+cmd+right"], keysOther: ["ctrl+alt+right"], local: "the terminal" },
  { id: "explorer.rename", title: "Rename File", category: "Explorer", keys: ["f2"], local: "the explorer" },
  { id: "explorer.delete", title: "Delete File", category: "Explorer", keys: ["cmd+backspace"], local: "the explorer" },
  { id: "git.switchBranch", title: "Switch Branch", category: "Git", keys: [] },
  { id: "git.newBranch", title: "New Branch", category: "Git", keys: [] },
  { id: "git.switchWorktree", title: "Switch Worktree", category: "Git", keys: [] },
  { id: "git.newWorktree", title: "New Worktree", category: "Git", keys: [] },
  { id: "git.fetch", title: "Fetch", category: "Git", keys: [] },
  { id: "git.pull", title: "Pull", category: "Git", keys: [] },
  { id: "git.push", title: "Push", category: "Git", keys: [] },
  // VS Code's Sync Changes: what the remote has, then what's here.
  { id: "git.sync", title: "Sync (Pull, then Push)", category: "Git", keys: [] },
  // Not while typing: text fields keep ⌘Z for their own undo.
  { id: "git.undo", title: "Undo Git Action", category: "Git", keys: ["cmd+z"], outsideText: true },
  { id: "git.redo", title: "Redo Git Action", category: "Git", keys: ["shift+cmd+z"], outsideText: true },
  // These act on the file open in the viewer. Discard asks first; ⌘⌫ stays line delete in text.
  { id: "git.toggleStage", title: "Stage / Unstage Changes", category: "Git", keys: ["s"] },
  { id: "git.discard", title: "Discard Changes", category: "Git", keys: ["cmd+backspace"], outsideText: true },
  // Local: only `local` listens for it.
  { id: "git.commit", title: "Commit", category: "Git", keys: ["cmd+enter", "ctrl+enter"], local: "the commit message" },
  { id: "git.renameBranch", title: "Rename Branch", category: "Git", keys: ["f2"], local: "the branch picker" },
  { id: "git.selectAllChanges", title: "Select All Changes", category: "Git", keys: ["cmd+a"], local: "the changes list" },
  { id: "git.createTag", title: "Create Tag", category: "Git", keys: ["cmd+enter", "ctrl+enter"], local: "the tag message" },
  // Where "/" takes ⇧ (Turkish, German ⇧7).
  { id: "history.find", title: "Find in History", category: "Git", keys: ["/", "shift+/"], local: "the history list" },
  { id: "worktree.openTerminal", title: "Open Terminal in Worktree", category: "Git", keys: ["t"], local: "the worktree picker" },
  { id: "worktree.merge", title: "Merge Worktree Branch", category: "Git", keys: ["m"], local: "the worktree picker" },
  { id: "worktree.rename", title: "Rename Worktree", category: "Git", keys: ["f2"], local: "the worktree picker" },
  { id: "worktree.remove", title: "Remove Worktree", category: "Git", keys: ["backspace", "delete"], local: "the worktree picker" },
  { id: "project.forget", title: "Remove from Projects", category: "General", keys: ["backspace", "delete"], local: "the project list" },
  { id: "github.postComment", title: "Post Comment", category: "GitHub", keys: ["cmd+enter", "ctrl+enter"], local: "an issue comment" },
  // Only while suggestions are set up in Settings.
  { id: "git.suggestMessage", title: "Suggest Commit Message", category: "Git", keys: [] },
] as const satisfies readonly {
  id: string;
  title: string;
  category: string;
  keys: readonly string[];
  /** The defaults off macOS, where they differ. */
  keysOther?: readonly string[];
  /** Where it listens, for a command only one place handles. */
  local?: string;
  outsideText?: boolean;
  /** Held down, it runs once: each run changes what the next one would act on. */
  noRepeat?: boolean;
}[];

export type CommandId = (typeof COMMANDS)[number]["id"];
export type Command = (typeof COMMANDS)[number];
export type Overrides = Record<string, string[]>;

export const isCommandId = (id: string): id is CommandId => COMMANDS.some((c) => c.id === id);

/** macOS handles these before the page sees them (quit, hide, minimize, cycle windows, screenshots). */
const MAC_RESERVED = ["cmd+q", "cmd+h", "alt+cmd+h", "cmd+m", "cmd+`", "shift+cmd+3", "shift+cmd+4", "shift+cmd+5"];

/**
 * Whether the system takes a chord before the page sees it. Only macOS does: off it "cmd" is
 * Ctrl, and the GTK menu has no Quit, Hide or Minimize items to claim Ctrl+Q, Ctrl+H or Ctrl+M.
 */
export const isReserved = (chord: string, mac = IS_MAC) => mac && MAC_RESERVED.includes(chord);

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
  // ⇧⌘] types "}"; named by its key, as menus show it.
  "{": "[",
  "}": "]",
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
export function eventChord(e: KeyLike, mac = IS_MAC): string | null {
  if (["Meta", "Control", "Alt", "Shift", "CapsLock", "Fn"].includes(e.key)) return null;
  const key = keyToken(e);
  if (!key) return null;
  const held = { cmd: mac ? e.metaKey : e.ctrlKey, ctrl: mac ? e.ctrlKey : e.metaKey, alt: e.altKey, shift: e.shiftKey };
  return [...MODS.filter((m) => held[m]), key].join("+");
}

/**
 * What a key event may mean: the chord as typed, then for a ⌘ or ⌃ chord on a punctuation key the
 * chord a US keyboard names it, so a default like ⌃` stays on that key where the layout types
 * something else there. Letters and digits only count as typed (German ⌘Y, the US Z key, must not
 * run ⌘Z), and so do keys without ⌘ or ⌃: they type, and "?" is not "/".
 */
export function eventChords(e: KeyLike, mac = IS_MAC): string[] {
  const chord = eventChord(e, mac);
  const us = CODE_KEYS[e.code];
  if (!chord || !us || !(e.metaKey || e.ctrlKey)) return chord ? [chord] : [];
  const physical = [...chord.split("+").slice(0, -1), us].join("+");
  return physical === chord ? [chord] : [chord, physical];
}

/** A chord as the native menu (muda) reads it: there "cmd" is always the ⌘ / Windows key. */
export function menuAccelerator(chord: string, mac = IS_MAC): string {
  if (mac) return chord;
  return chord
    .split("+")
    .map((p) => (p === "cmd" ? "ctrl" : p === "ctrl" ? "super" : p))
    .join("+");
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

export function bindingsFor(id: CommandId, overrides: Overrides, mac = IS_MAC): readonly string[] {
  const c: Command = COMMANDS.find((c) => c.id === id)!;
  return overrides[id] ?? (!mac && "keysOther" in c ? c.keysOther : c.keys);
}

/** The global command a chord runs: the first listed one bound to it. */
export function commandFor(chord: string, overrides: Overrides, mac = IS_MAC): Command | undefined {
  return COMMANDS.find((c) => !("local" in c) && bindingsFor(c.id, overrides, mac).includes(chord));
}

/**
 * Whether a chord still runs its command while focus is in a text field or the terminal. Letters
 * and ⌥ chords type characters there, ⌃+letter edits the line on macOS (⌃A, ⌃K), and "cmd"-arrows
 * move the cursor (⌘←/⌘→ to the line's ends, Ctrl+←/→ by word elsewhere). Other "cmd" chords,
 * ⌃ with anything else (⌃Tab, ⌃1), the Super key and F-keys are free.
 */
export function runsWhileTyping(chord: string, command: Command, mac = IS_MAC): boolean {
  if ("outsideText" in command) return false;
  const mods = chord.split("+");
  const key = mods.pop()!;
  if (mods.includes("cmd")) return !["left", "right", "up", "down"].includes(key);
  if (mods.includes("ctrl")) return !mac || !/^[a-z]$/.test(key);
  return /^f\d+$/.test(key) && !mods.includes("alt");
}

/**
 * Whether the terminal hands a chord to its command instead of the shell: the physical Ctrl with
 * anything but a letter (⌃Tab, ⌃1), which a shell would only read as a control code. Ctrl+letter
 * (⌃C, ⌃R) is always the shell's; Ctrl+Shift+letter is the app's, as in Linux terminals.
 */
export function takenFromTerminal(chord: string, command: Command, mac = IS_MAC): boolean {
  const mods = chord.split("+");
  const key = mods.pop()!;
  const shellKey = /^[a-z]$/.test(key) && !mods.includes("shift");
  return mods.includes(mac ? "ctrl" : "cmd") && !shellKey && runsWhileTyping(chord, command, mac);
}

/**
 * Whether a chord runs its command while the terminal has focus: only what the terminal doesn't
 * read itself, so a key never goes to both. That's ⌘ (the Super key elsewhere) and what it hands
 * over (takenFromTerminal). F-keys stay the shell's: programs there use them (htop, mc).
 */
export function runsInTerminal(chord: string, command: Command, mac = IS_MAC): boolean {
  if (chord.split("+").includes(mac ? "cmd" : "ctrl")) return runsWhileTyping(chord, command, mac);
  return takenFromTerminal(chord, command, mac);
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

const NAMES: Record<string, string> = { cmd: "Ctrl", ctrl: IS_WINDOWS ? "Win" : "Super", alt: "Alt", shift: "Shift" };

/** One argument, so it can go straight into `.map`. */
export const formatChord = (chord: string) => formatChordFor(chord, IS_MAC);

/** ⇧⌘E on macOS, Ctrl+Shift+E elsewhere. */
export function formatChordFor(chord: string, mac: boolean): string {
  return chordKeysFor(chord, mac).join(mac ? "" : "+");
}

/** A chord's keys one by one, as keycaps show them: ⇧ ⌘ E on macOS, Ctrl Shift E elsewhere. */
export const chordKeys = (chord: string) => chordKeysFor(chord, IS_MAC);

function chordKeysFor(chord: string, mac: boolean): string[] {
  const parts = chord.split("+");
  if (mac) return parts.map((p) => GLYPHS[p] ?? p.toUpperCase());
  // Windows and Linux lead with Ctrl, not with ⌃⌥⇧⌘'s order.
  const key = parts.pop()!;
  const mods = ["cmd", "ctrl", "alt", "shift"].filter((m) => parts.includes(m)).map((m) => NAMES[m]);
  return [...mods, GLYPHS[key] ?? key.toUpperCase()];
}
