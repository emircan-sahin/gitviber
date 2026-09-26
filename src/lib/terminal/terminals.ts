import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { SerializeAddon } from "@xterm/addon-serialize";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useSyncExternalStore } from "react";
import { errorMessage, pty } from "../api";
import { compileFind, type FindOptions } from "../ui/findQuery";
import { appTakesFromTerminal, type CommandId, commandIn } from "../commands/keybindings";
import { terminalLinks } from "../links/linkHost";
import { getSettings, subscribeSettings } from "../settings";
import { isInside } from "../path";
import { readJson } from "../storage";
import { setTerminalFocus } from "../ui/panels";
import { findColors, terminalOptions } from "./theme";
import { pathPastes } from "./paste";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { IS_LINUX, IS_WINDOWS } from "../platform";
import { toast } from "../app/toast";

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
  /** The history last saved, until new output or a resize: a quiet pane isn't serialized again. */
  saved: string | null;
  search: SearchAddon;
  /** The WebGL renderer, while the pane has one, and its context to lose on close. */
  gl: WebglAddon | null;
  glContext: WebGL2RenderingContext | null;
  host: HTMLDivElement;
  pty: number | null;
  started: boolean;
  /** Typed while a write is in flight (or before the shell is up); sent next, in order. */
  pending: string;
  writing: boolean;
}

interface PaneInfo {
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
interface SavedSession {
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
  const s = readJson<SavedSession | null>(SESSION_KEY, null);
  return s && Array.isArray(s.groups) && s.groups.length ? s : null;
}

const panes = new Map<number, Pane>();
// The WebGL glyph atlas's page canvases. xterm shares one atlas between terminals with the same
// font and colors, and drops it with the last of them. Weak: an atlas replaced by a theme, font or
// scale change is xterm's to drop.
let atlasPages: WeakRef<HTMLCanvasElement>[] = [];

/**
 * WebKit frees a canvas's backing store, and a WebGL context, only once the canvas is collected,
 * which it puts off until the page nears 1 GB: closed panes kept hundreds of MB.
 */
function releaseCanvases(canvases: Iterable<HTMLCanvasElement>) {
  for (const c of canvases) c.width = c.height = 0;
}
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
          return { cwd, history: history && p ? (p.saved ??= p.serialize.serialize({ scrollback: HISTORY_LINES, excludeAltBuffer: true, excludeModes: true })) : "" };
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
    pty.write(p.pty, data)
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
  const p: Pane = { id, cwd, term, fit, serialize, saved: null, search, gl: null, glContext: null, host, pty: null, started: false, pending: "", writing: false };
  panes.set(id, p);
  if (restored?.history) term.write(`${restored.history}\x1b[0m\r\n\x1b[2m── Restored from ${new Date(restored.savedAt).toLocaleString()} ──\x1b[0m\r\n`);
  term.onWriteParsed(() => {
    p.saved = null;
    scheduleSave();
  });
  term.onData((data) => send(p, data));
  term.onResize(({ cols, rows }) => {
    // Reflow rewraps the history.
    p.saved = null;
    if (p.pty !== null) void pty.resize(p.pty, cols, rows).catch(() => {});
  });
  term.onTitleChange((title) => update(id, (info) => ({ ...info, title })));
  // ⌘V reads the pasteboard natively (clipboard.rs): the webview's paste carries only text, so a
  // copied image or Finder file pasted nothing. Ahead of xterm's own handler on its text area.
  // Linux reads GTK's clipboard the same way (Shift+Insert, Ctrl+Shift+V below); Windows is untried.
  // A middle-click pastes Linux's primary selection, which xterm moves its text area under the
  // pointer for: that paste is left to it.
  let middleAt = -Infinity;
  if (IS_LINUX) host.addEventListener("mousedown", (e) => e.button === 1 && (middleAt = performance.now()), true);
  if (!IS_WINDOWS)
    host.addEventListener(
      "paste",
      (e) => {
        if (performance.now() - middleAt < 1000) return void (middleAt = -Infinity);
        // Kept for when the native read fails: the text at least still pastes.
        const text = e.clipboardData?.getData("text/plain") ?? "";
        e.preventDefault();
        e.stopPropagation();
        void pasteInto(p, text);
      },
      true,
    );
  // ⌘ keys are the app's shortcuts (copy and paste arrive as clipboard events, not keys),
  // except the line-editing ones; ⌃` toggles the panel instead of sending NUL, ⌃Tab or ⌃1 run
  // their commands, and the panel's own keys stay with it whatever they're rebound to.
  term.attachCustomKeyEventHandler((e) => {
    // Linux terminals copy and paste with Ctrl+Shift+C/V: Ctrl+C and Ctrl+V belong to the shell.
    // The letter as typed (Dvorak's C isn't on the C key), or the key's place on a non-Latin layout.
    const letter = /^[a-z]$/i.test(e.key) ? e.key.toLowerCase() : e.code.replace(/^Key/, "").toLowerCase();
    if (IS_LINUX && e.ctrlKey && e.shiftKey && !e.altKey && !e.metaKey && (letter === "c" || letter === "v")) {
      if (e.type === "keydown") {
        const selection = term.getSelection();
        if (letter === "v") void pasteInto(p);
        else if (selection) void navigator.clipboard.writeText(selection).catch(() => {});
      }
      e.preventDefault();
      return false;
    }
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

/** `fallback`: the webview's own text, pasted if the native read fails or finds nothing. */
async function pasteInto(p: Pane, fallback = "") {
  const got = await pty.paste().catch((e) => {
    if (!fallback) toast("error", "Could not paste", errorMessage(e));
    return null;
  });
  if (!got || got.kind === "empty") {
    if (fallback) p.term.paste(fallback);
    return;
  }
  if (got.kind === "text") p.term.paste(got.text);
  else if (got.kind === "files") pastePaths(p, got.paths);
  else if (got.kind === "image") pastePaths(p, [got.path]);
}

/** Paths as the AI CLIs take them: each its own (bracketed) paste, never typed as keys. */
function pastePaths(p: Pane, paths: string[]) {
  for (const text of pathPastes(paths)) p.term.paste(text);
}

// Files dropped on a pane paste their paths into it (Tauri hands over the paths, the page only
// their names). The pane under the pointer is outlined while they're dragged.
let dropTarget: Pane | null = null;
function paneAt(pos: { x: number; y: number }) {
  // Typed physical, but on macOS and Linux wry hands over window points unscaled (drag_drop.rs):
  // halved on Retina, the point landed in the sidebar. Page zoom (the UI scale) makes a CSS pixel
  // bigger than a point. Windows' pixels are physical, and Chromium's ratio includes the zoom.
  const scale = IS_WINDOWS ? devicePixelRatio : getSettings().uiScale;
  const el = document.elementFromPoint(pos.x / scale, pos.y / scale);
  return el ? ([...panes.values()].find((p) => p.host.contains(el)) ?? null) : null;
}
function markDropTarget(p: Pane | null) {
  if (p === dropTarget) return;
  dropTarget?.host.classList.remove("gv-drop-target");
  p?.host.classList.add("gv-drop-target");
  dropTarget = p;
}
const dropListener = getCurrentWebview().onDragDropEvent(async ({ payload }) => {
  if (payload.type === "leave") return markDropTarget(null);
  const p = paneAt(payload.position);
  if (payload.type !== "drop") return markDropTarget(p);
  markDropTarget(null);
  if (!p) return;
  pastePaths(p, await pty.keepDropped(payload.paths).catch(() => payload.paths));
  p.term.focus();
});
dropListener.catch(() => {});
// A hot reload re-runs this module: the old listener goes, or each drop would paste twice.
import.meta.hot?.dispose(() => void dropListener.then((stop) => stop()).catch(() => {}));

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
  try {
    const { cols, rows } = p.term;
    const id = await pty.spawn(p.cwd, cols, rows, (bytes) => p.term.write(new Uint8Array(bytes)), () => closePane(p.id));
    // Closed while it was starting.
    if (!panes.has(p.id)) return void pty.kill(id).catch(() => {});
    p.pty = id;
    // A resize while it was starting had no shell to reach.
    if (p.term.cols !== cols || p.term.rows !== rows) void pty.resize(id, p.term.cols, p.term.rows).catch(() => {});
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
      gl.onContextLoss(() => {
        gl.dispose();
        p.gl = null;
        p.glContext = null;
      });
      const track = (c: HTMLCanvasElement) => void atlasPages.push(new WeakRef(c));
      gl.onAddTextureAtlasCanvas(track);
      // A new atlas (theme, font, scale) announces its first page only here.
      gl.onChangeTextureAtlas(track);
      const had = new Set(p.host.querySelectorAll("canvas"));
      p.term.loadAddon(gl);
      p.gl = gl;
      // From the canvases it just made, each made with its context: on one without, getContext
      // would make a new WebGL context, and WebKit caps them.
      for (const c of p.host.querySelectorAll("canvas")) if (!had.has(c)) p.glContext ??= c.getContext("webgl2");
      // The first page predates the listeners.
      if (gl.textureAtlas) track(gl.textureAtlas);
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
setTerminalFocus(focusActive);

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

function closePane(id: number) {
  const p = panes.get(id);
  if (!p) return;
  panes.delete(id);
  if (searching?.pane === p) searching = null;
  if (p.pty !== null) void pty.kill(p.pty).catch(() => {});
  const canvases = [...(p.term.element?.querySelectorAll("canvas") ?? [])];
  p.term.dispose();
  p.glContext?.getExtension("WEBGL_lose_context")?.loseContext();
  releaseCanvases(canvases);
  // The atlas is shared by the panes on WebGL and goes with the last of them.
  if (![...panes.values()].some((x) => x.gl)) {
    releaseCanvases(atlasPages.flatMap((r) => r.deref() ?? []));
    atlasPages = [];
  }
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

export function clearFocused() {
  const p = panes.get(activeGroup()?.focused ?? -1);
  if (!p) return;
  p.term.clear();
  // clear() skips the parser, so onWriteParsed doesn't drop the saved copy.
  p.saved = null;
  scheduleSave();
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
