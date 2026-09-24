import { Channel, invoke } from "@tauri-apps/api/core";
import { FitAddon } from "@xterm/addon-fit";
import { type ISearchDecorationOptions, SearchAddon } from "@xterm/addon-search";
import { SerializeAddon } from "@xterm/addon-serialize";
import { WebglAddon } from "@xterm/addon-webgl";
import { type ITerminalOptions, Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useSyncExternalStore } from "react";
import { errorMessage } from "./api";
import { compileFind, type FindOptions } from "./findQuery";
import { appTakesFromTerminal, type CommandId, commandIn } from "./keybindings";
import { terminalLinks } from "./linkHost";
import { codeFontFamily, getSettings, subscribeSettings } from "./settings";
import { isInside } from "./worktrees";

/**
 * Terminals live here, not in React: switching worktrees remounts the whole workspace, and
 * a running shell (an agent, a dev server) must not notice. Panes mount by moving their
 * host element into whatever container shows them.
 */
interface Pane {
  id: number;
  cwd: string;
  term: Terminal;
  fit: FitAddon;
  serialize: SerializeAddon;
  search: SearchAddon;
  host: HTMLDivElement;
  pty: number | null;
  started: boolean;
  /** Typed while a write is in flight (or before the shell is up); sent next, in order. */
  pending: string;
  writing: boolean;
}

export interface PaneInfo {
  id: number;
  cwd: string;
  /** What the shell set as the window title (OSC 0/2), if anything. */
  title: string;
}

/** A tab: one or more panes side by side. */
export interface TerminalGroup {
  id: number;
  panes: PaneInfo[];
  focused: number;
}

/** The terminals of a previous run: where each shell was, and what it had printed. */
export interface SavedSession {
  savedAt: number;
  active: number;
  groups: { focused: number; panes: { cwd: string; history: string }[] }[];
}

interface State {
  open: boolean;
  groups: TerminalGroup[];
  active: number | null;
  /** Last run's terminals, until the user restores or dismisses them. */
  restorable: SavedSession | null;
}

/** Keys the terminal panel runs while it has focus (TerminalPanel), rather than the shell. */
export const TERMINAL_COMMANDS = ["terminal.split", "terminal.clear", "terminal.close", "terminal.prevPane", "terminal.nextPane"] as const satisfies readonly CommandId[];

const SESSION_KEY = "gitviber.terminals";
// Serialized with colors, 1000 lines is ~100 KB a pane; localStorage holds a few MB.
const HISTORY_LINES = 1000;

function loadSession(): SavedSession | null {
  try {
    const s = JSON.parse(localStorage.getItem(SESSION_KEY) ?? "null") as SavedSession | null;
    return s && Array.isArray(s.groups) && s.groups.length ? s : null;
  } catch {
    return null;
  }
}

const panes = new Map<number, Pane>();
let state: State = { open: false, groups: [], active: null, restorable: loadSession() };
let nextId = 1;
const listeners = new Set<() => void>();

function set(patch: Partial<State>) {
  state = { ...state, ...patch };
  listeners.forEach((l) => l());
  scheduleSave();
}

let saveTimer: number | undefined;
/** Throttled rather than debounced, so a shell that never stops printing still gets saved. */
function scheduleSave() {
  saveTimer ??= window.setTimeout(() => {
    saveTimer = undefined;
    saveSession();
  }, 2000);
}

// The throttle would lose the last seconds of output to a reload (⌘R).
window.addEventListener("pagehide", () => saveSession());

function saveSession() {
  // Nothing opened yet: keep the last run's terminals for the restore offer.
  if (!state.groups.length && state.restorable) return;
  try {
    if (!state.groups.length) return localStorage.removeItem(SESSION_KEY);
    const snapshot = (history: boolean): SavedSession => ({
      savedAt: Date.now(),
      active: Math.max(0, state.groups.findIndex((g) => g.id === state.active)),
      groups: state.groups.map((g) => ({
        focused: Math.max(0, g.panes.findIndex((p) => p.id === g.focused)),
        panes: g.panes.map(({ id, cwd }) => {
          const p = panes.get(id);
          // Alt-screen apps and terminal modes (mouse, bracketed paste) would leak into the new shell.
          return { cwd, history: history && p ? p.serialize.serialize({ scrollback: HISTORY_LINES, excludeAltBuffer: true, excludeModes: true }) : "" };
        }),
      })),
    });
    try {
      localStorage.setItem(SESSION_KEY, JSON.stringify(snapshot(true)));
    } catch {
      // Over quota: the layout alone is still worth keeping.
      localStorage.setItem(SESSION_KEY, JSON.stringify(snapshot(false)));
    }
  } catch {
    // Not critical.
  }
}

export function useTerminals() {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => state,
  );
}

const activeGroup = () => state.groups.find((g) => g.id === state.active);

const ANSI_DARK = {
  black: "#3a3a3e",
  red: "#f47067",
  green: "#57d17a",
  yellow: "#e2b84c",
  blue: "#4a9ff5",
  magenta: "#b392f0",
  cyan: "#56c8d8",
  white: "#d4d4d8",
  brightBlack: "#6c6c73",
  brightRed: "#ff8a80",
  brightGreen: "#7ee29a",
  brightYellow: "#f0cf74",
  brightBlue: "#74b6f7",
  brightMagenta: "#c9b0f5",
  brightCyan: "#7fdbe6",
  brightWhite: "#ffffff",
};

const ANSI_LIGHT = {
  black: "#1d1d1f",
  red: "#cf222e",
  green: "#1a7f37",
  yellow: "#9a6700",
  blue: "#0969da",
  magenta: "#8250df",
  cyan: "#1b7c83",
  white: "#6e7781",
  brightBlack: "#5b5b62",
  brightRed: "#a40e26",
  brightGreen: "#116329",
  brightYellow: "#7d4e00",
  brightBlue: "#0550ae",
  brightMagenta: "#6639ba",
  brightCyan: "#136061",
  brightWhite: "#8c959f",
};

/** Styled like the code view: its font and size, the app's own background and accents. */
function terminalOptions(): ITerminalOptions {
  const s = getSettings();
  const css = getComputedStyle(document.documentElement);
  const v = (name: string) => css.getPropertyValue(name).trim();
  return {
    fontFamily: codeFontFamily(s),
    fontSize: s.codeFontSize,
    // The code view's 1.6 is for reading; TUIs draw box lines that need to touch.
    lineHeight: 1.2,
    theme: {
      ...(s.dark ? ANSI_DARK : ANSI_LIGHT),
      background: v("--background"),
      foreground: v("--foreground"),
      cursor: v("--primary"),
      cursorAccent: v("--background"),
      selectionBackground: `${v("--primary")}55`,
      // --scrollbar-thumb, -hover and -active from index.css: 18%, 36% and 50% of the foreground.
      scrollbarSliderBackground: `${v("--foreground")}2e`,
      scrollbarSliderHoverBackground: `${v("--foreground")}5c`,
      scrollbarSliderActiveBackground: `${v("--foreground")}80`,
    },
  };
}

// settings.ts sets the theme attribute before notifying, so the CSS variables are current.
subscribeSettings(() => {
  const next = terminalOptions();
  for (const p of panes.values()) {
    Object.assign(p.term.options, next);
    fitPane(p);
  }
});

function send(p: Pane, data: string) {
  p.pending += data;
  if (p.writing) return;
  const flush = () => {
    if (!p.pending || p.pty === null) {
      p.writing = false;
      return;
    }
    const data = p.pending;
    p.pending = "";
    p.writing = true;
    invoke("pty_write", { id: p.pty, data })
      .catch(() => {})
      .finally(flush);
  };
  flush();
}

function createPane(cwd: string, restored?: { history: string; savedAt: number }): PaneInfo {
  const id = nextId++;
  // The proposed API is the decorations, which find marks its matches with.
  const term = new Terminal({ ...terminalOptions(), cursorBlink: true, scrollback: 10_000, allowProposedApi: true });
  const fit = new FitAddon();
  term.loadAddon(fit);
  const serialize = new SerializeAddon();
  term.loadAddon(serialize);
  terminalLinks(term, cwd);
  const search = new SearchAddon();
  term.loadAddon(search);
  search.onDidChangeResults(({ resultIndex, resultCount }) => searching?.pane === p && searching.onResults({ index: resultIndex + 1, total: resultCount }));
  const host = document.createElement("div");
  host.style.cssText = "width:100%;height:100%";
  const p: Pane = { id, cwd, term, fit, serialize, search, host, pty: null, started: false, pending: "", writing: false };
  panes.set(id, p);
  if (restored?.history) term.write(`${restored.history}\x1b[0m\r\n\x1b[2m── Restored from ${new Date(restored.savedAt).toLocaleString()} ──\x1b[0m\r\n`);
  term.onWriteParsed(scheduleSave);
  term.onData((data) => send(p, data));
  term.onResize(({ cols, rows }) => p.pty !== null && void invoke("pty_resize", { id: p.pty, cols, rows }).catch(() => {}));
  term.onTitleChange((title) => update(id, (info) => ({ ...info, title })));
  // ⌘ keys are the app's shortcuts (copy and paste arrive as clipboard events, not keys),
  // except the line-editing ones; ⌃` toggles the panel instead of sending NUL, ⌃Tab or ⌃1 run
  // their commands, and the panel's own keys stay with it whatever they're rebound to.
  term.attachCustomKeyEventHandler((e) => {
    const seq = lineEditKey(e);
    if (seq !== undefined) {
      if (e.type === "keydown") term.input(seq);
      e.preventDefault();
      return false;
    }
    return !e.metaKey && !appTakesFromTerminal(e) && !commandIn(TERMINAL_COMMANDS, e);
  });
  return { id, cwd, title: "" };
}

/**
 * macOS line editing, as in VS Code's terminal. xterm.js sends ⌥← / ⌥→ / ⌥⌦ as
 * `ESC[1;3D`-style sequences that neither zsh nor bash binds by default; the readline
 * sequences below work in both. ⌥⌫ is already ESC DEL (delete word).
 */
const LINE_EDIT: Record<string, string> = {
  "cmd+ArrowLeft": "\x01", // start of line (⌃A)
  "cmd+ArrowRight": "\x05", // end of line (⌃E)
  "cmd+Backspace": "\x15", // delete to start of line (⌃U)
  "alt+ArrowLeft": "\x1bb", // previous word
  "alt+ArrowRight": "\x1bf", // next word
  "alt+Delete": "\x1bd", // delete next word
};

function lineEditKey(e: KeyboardEvent): string | undefined {
  if (e.shiftKey || e.ctrlKey || e.metaKey === e.altKey) return undefined;
  return LINE_EDIT[`${e.metaKey ? "cmd" : "alt"}+${e.key}`];
}

function update(id: number, fn: (p: PaneInfo) => PaneInfo) {
  set({ groups: state.groups.map((g) => (g.panes.some((p) => p.id === id) ? { ...g, panes: g.panes.map((p) => (p.id === id ? fn(p) : p)) } : g)) });
}

/** A hidden or collapsed container would shrink the shell to one row and garble its output. */
function fitPane(p: Pane) {
  const box = p.host.parentElement;
  if (!p.term.element || !box || box.clientWidth === 0 || box.clientHeight === 0) return;
  p.fit.fit();
  if (!p.started) void start(p);
}

async function start(p: Pane) {
  p.started = true;
  const output = new Channel<ArrayBuffer>();
  output.onmessage = (bytes) => p.term.write(new Uint8Array(bytes));
  const exit = new Channel<number | null>();
  exit.onmessage = () => closePane(p.id);
  try {
    const { cols, rows } = p.term;
    const id = await invoke<number>("pty_spawn", { cwd: p.cwd, cols, rows, output, exit });
    // Closed while it was starting.
    if (!panes.has(p.id)) return void invoke("pty_kill", { id }).catch(() => {});
    p.pty = id;
    // A resize while it was starting had no shell to reach.
    if (p.term.cols !== cols || p.term.rows !== rows) void invoke("pty_resize", { id, cols: p.term.cols, rows: p.term.rows }).catch(() => {});
    send(p, "");
  } catch (e) {
    p.term.write(`\x1b[31m${errorMessage(e)}\x1b[0m\r\n`);
  }
}

/** Shows a pane in `container`; returns the detach. */
export function attachPane(id: number, container: HTMLElement) {
  const p = panes.get(id);
  if (!p) return () => {};
  container.appendChild(p.host);
  if (!p.term.element) {
    p.term.open(p.host);
    p.term.textarea?.addEventListener("focus", () => focusPane(id));
    try {
      const gl = new WebglAddon();
      // WebKit caps live WebGL contexts; past that, a pane falls back to the DOM renderer.
      gl.onContextLoss(() => gl.dispose());
      p.term.loadAddon(gl);
    } catch {
      // DOM renderer.
    }
  }
  fitPane(p);
  const observer = new ResizeObserver(() => fitPane(p));
  observer.observe(container);
  return () => {
    observer.disconnect();
    p.host.remove();
  };
}

/** Focuses the active tab's pane once it's on screen. */
export function focusActive() {
  requestAnimationFrame(() => {
    const g = activeGroup();
    if (g) panes.get(g.focused)?.term.focus();
  });
}

export function openTerminal(cwd: string) {
  const pane = createPane(cwd);
  const group = { id: nextId++, panes: [pane], focused: pane.id };
  set({ open: true, groups: [...state.groups, group], active: group.id });
  focusActive();
}

/** Adds a pane beside the focused one, in the same folder. */
export function splitActive() {
  const g = activeGroup();
  if (!g) return;
  const at = g.panes.findIndex((p) => p.id === g.focused);
  const pane = createPane(g.panes[at].cwd);
  const next = { ...g, panes: [...g.panes.slice(0, at + 1), pane, ...g.panes.slice(at + 1)], focused: pane.id };
  set({ groups: state.groups.map((x) => (x.id === g.id ? next : x)) });
  focusActive();
}

export function closePane(id: number) {
  const p = panes.get(id);
  if (!p) return;
  panes.delete(id);
  if (searching?.pane === p) searching = null;
  if (p.pty !== null) void invoke("pty_kill", { id: p.pty }).catch(() => {});
  p.term.dispose();
  p.host.remove();
  const groups = state.groups.flatMap((g) => {
    const i = g.panes.findIndex((x) => x.id === id);
    if (i < 0) return [g];
    const rest = g.panes.filter((x) => x.id !== id);
    if (!rest.length) return [];
    return [{ ...g, panes: rest, focused: g.focused === id ? rest[Math.min(i, rest.length - 1)].id : g.focused }];
  });
  let active = state.active;
  if (!groups.some((g) => g.id === active)) {
    const i = state.groups.findIndex((g) => g.id === active);
    active = groups[Math.min(i, groups.length - 1)]?.id ?? null;
  }
  set({ groups, active, open: state.open && groups.length > 0 });
  focusActive();
}

export function closeFocused() {
  const g = activeGroup();
  if (g) closePane(g.focused);
}

export function closeGroup(id: number) {
  state.groups.find((g) => g.id === id)?.panes.forEach((p) => closePane(p.id));
}

/** The pane find searches, and where its count goes (index 0: past the addon's 1000 marked matches). */
let searching: { pane: Pane; onResults: (at: { index: number; total: number }) => void } | null = null;

/**
 * Find in the active tab's focused pane: `step` 0 as the query is typed (staying on the match
 * on show while it still matches), 1 or -1 for the next or previous. An empty query clears it;
 * a regex that doesn't parse is returned as the error, nothing searched.
 */
export function findInTerminal(query: string, find: FindOptions, step: 0 | 1 | -1, onResults: (at: { index: number; total: number }) => void): string | null {
  const g = activeGroup();
  const p = g && panes.get(g.focused);
  if (searching && searching.pane !== p) searching.pane.search.clearDecorations();
  searching = p ? { pane: p, onResults } : null;
  if (!p) return null;
  const re = compileFind(query, find);
  if (!query || re instanceof Error) {
    p.search.clearDecorations();
    onResults({ index: 0, total: 0 });
    return re instanceof Error ? re.message : null;
  }
  const options = { caseSensitive: find.matchCase, wholeWord: find.wholeWord, regex: find.regex, decorations: findColors(), incremental: step === 0 };
  if (step === -1) p.search.findPrevious(query, options);
  else p.search.findNext(query, options);
  return null;
}

/** Find's marks go, and its count stops being reported (the box went, or searches elsewhere now). */
export function clearFind() {
  searching?.pane.search.clearDecorations();
  searching = null;
}

/** Closes find: its marks go, and the pane gets the keys back. */
export function endFind() {
  clearFind();
  focusActive();
}

// The app's find colors (index.css). Matches take solid colors only: the washes are laid on the background here.
function findColors(): ISearchDecorationOptions {
  const css = getComputedStyle(document.documentElement);
  const v = (name: string) => css.getPropertyValue(name).trim();
  const mark = solid(v("--find-mark"), v("--background"));
  return { matchBackground: solid(v("--find-match"), v("--background")), activeMatchBackground: solid(v("--find-current"), v("--background")), matchOverviewRuler: mark, activeMatchColorOverviewRuler: mark };
}

const probe = document.createElement("canvas").getContext("2d", { willReadFrequently: true })!;
/** `color` over `under`, as #RRGGBB. */
function solid(color: string, under: string) {
  probe.fillStyle = under;
  probe.fillRect(0, 0, 1, 1);
  probe.fillStyle = color;
  probe.fillRect(0, 0, 1, 1);
  return "#" + [...probe.getImageData(0, 0, 1, 1).data.slice(0, 3)].map((n) => n.toString(16).padStart(2, "0")).join("");
}

export function clearFocused() {
  const g = activeGroup();
  if (g) panes.get(g.focused)?.term.clear();
}

/** `focus: false` keeps focus where it is: arrowing along the tabs. */
export function activateGroup(id: number, focus = true) {
  if (state.active !== id) set({ active: id });
  if (focus) focusActive();
}

function focusPane(id: number) {
  const g = state.groups.find((x) => x.panes.some((p) => p.id === id));
  if (!g || (g.focused === id && state.active === g.id)) return;
  set({ active: g.id, groups: state.groups.map((x) => (x === g ? { ...g, focused: id } : x)) });
}

/** The next split pane of the open tab (⌥⌘←/→, as in VS Code), wrapping around. */
export function stepPane(dir: 1 | -1) {
  const g = activeGroup();
  if (!g || g.panes.length < 2) return;
  const at = g.panes.findIndex((p) => p.id === g.focused);
  focusPane(g.panes[(at + dir + g.panes.length) % g.panes.length].id);
  focusActive();
}

/** Opens the panel (with a first terminal in `cwd` if there is none), or hides it. */
export function togglePanel(cwd: string) {
  if (state.open) return set({ open: false });
  if (!state.groups.length) return openTerminal(cwd);
  set({ open: true });
  focusActive();
}

/** Reopens last run's terminals beside any opened since: same folders, their output, new shells. */
export function restoreSession() {
  const saved = state.restorable;
  if (!saved) return;
  const groups = saved.groups.map((g) => {
    const infos = g.panes.map((p) => createPane(p.cwd, { history: p.history, savedAt: saved.savedAt }));
    return { id: nextId++, panes: infos, focused: (infos[g.focused] ?? infos[0]).id };
  });
  set({ open: true, groups: [...state.groups, ...groups], active: (groups[saved.active] ?? groups[0]).id, restorable: null });
  focusActive();
}

export function dismissRestore() {
  set({ restorable: null });
}

const within = (cwd: string, dir: string) => cwd === dir || isInside(cwd, dir);

/** How many terminals were started in `dir` or below it. */
export const terminalsIn = (dir: string) => state.groups.reduce((n, g) => n + g.panes.filter((p) => within(p.cwd, dir)).length, 0);

/** A folder was moved. Its shells went along (a cwd is the folder, not its path), so splits and restores follow. */
export function folderMoved(from: string, to: string) {
  const moved = (cwd: string) => (within(cwd, from) ? to + cwd.slice(from.length) : cwd);
  for (const p of panes.values()) p.cwd = moved(p.cwd);
  set({ groups: state.groups.map((g) => ({ ...g, panes: g.panes.map((p) => ({ ...p, cwd: moved(p.cwd) })) })) });
}

/** Switching to a worktree brings up a terminal that's already in it. */
export function showWorktree(cwd: string) {
  if (activeGroup()?.panes.some((p) => p.cwd === cwd)) return;
  const g = state.groups.find((x) => x.panes.some((p) => p.cwd === cwd));
  if (g) set({ active: g.id });
}
