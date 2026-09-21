import { Channel, invoke } from "@tauri-apps/api/core";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { type ITerminalOptions, Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useSyncExternalStore } from "react";
import { errorMessage } from "./api";
import { CODE_FONTS, getSettings, subscribeSettings } from "./settings";

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

interface State {
  open: boolean;
  groups: TerminalGroup[];
  active: number | null;
}

const panes = new Map<number, Pane>();
let state: State = { open: false, groups: [], active: null };
let nextId = 1;
const listeners = new Set<() => void>();

function set(patch: Partial<State>) {
  state = { ...state, ...patch };
  listeners.forEach((l) => l());
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
    fontFamily: CODE_FONTS[s.codeFont],
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
      scrollbarSliderBackground: `${v("--foreground")}1f`,
      scrollbarSliderHoverBackground: `${v("--foreground")}33`,
      scrollbarSliderActiveBackground: `${v("--foreground")}4d`,
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

function createPane(cwd: string): PaneInfo {
  const id = nextId++;
  const term = new Terminal({ ...terminalOptions(), cursorBlink: true, scrollback: 10_000 });
  const fit = new FitAddon();
  term.loadAddon(fit);
  const host = document.createElement("div");
  host.style.cssText = "width:100%;height:100%";
  const p: Pane = { id, cwd, term, fit, host, pty: null, started: false, pending: "", writing: false };
  panes.set(id, p);
  term.onData((data) => send(p, data));
  term.onResize(({ cols, rows }) => p.pty !== null && void invoke("pty_resize", { id: p.pty, cols, rows }).catch(() => {}));
  term.onTitleChange((title) => update(id, (info) => ({ ...info, title })));
  // ⌘ keys are the app's shortcuts (copy and paste arrive as clipboard events, not keys);
  // ⌃` toggles the panel instead of sending NUL.
  term.attachCustomKeyEventHandler((e) => !e.metaKey && !(e.ctrlKey && e.code === "Backquote"));
  return { id, cwd, title: "" };
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

export function clearFocused() {
  const g = activeGroup();
  if (g) panes.get(g.focused)?.term.clear();
}

export function activateGroup(id: number) {
  if (state.active !== id) set({ active: id });
  focusActive();
}

function focusPane(id: number) {
  const g = state.groups.find((x) => x.panes.some((p) => p.id === id));
  if (!g || (g.focused === id && state.active === g.id)) return;
  set({ active: g.id, groups: state.groups.map((x) => (x === g ? { ...g, focused: id } : x)) });
}

/** Opens the panel (with a first terminal in `cwd` if there is none), or hides it. */
export function togglePanel(cwd: string) {
  if (state.open) return set({ open: false });
  if (!state.groups.length) return openTerminal(cwd);
  set({ open: true });
  focusActive();
}

/** Switching to a worktree brings up a terminal that's already in it. */
export function showWorktree(cwd: string) {
  if (activeGroup()?.panes.some((p) => p.cwd === cwd)) return;
  const g = state.groups.find((x) => x.panes.some((p) => p.cwd === cwd));
  if (g) set({ active: g.id });
}
