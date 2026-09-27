import { arrayMove } from "@dnd-kit/sortable";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { SerializeAddon } from "@xterm/addon-serialize";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useMemo, useSyncExternalStore } from "react";
import { errorMessage, pty, type PtyExit } from "../api";
import { compileFind, type FindOptions } from "../ui/findQuery";
import { appRunsFromTerminal, appTakesFromTerminal, type CommandId, commandIn } from "../commands/keybindings";
import { terminalLinks } from "../links/linkHost";
import { getSettings, subscribeSettings } from "../settings";
import { folderName, isInside } from "../path";
import { readJson } from "../storage";
import { focusedPanel, focusPanel, setTerminalFocus } from "../ui/panels";
import { findColors, terminalOptions } from "./theme";
import { pastedLines, pathPastes } from "./paste";
import { osc52Text } from "./osc52";
import { CommandMarks } from "./commandMarks";
import { dueForSave, SAVE_MS, type SaveState } from "./saveRound";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { ask } from "../app/ask";
import { plural } from "../format";
import { IS_LINUX, IS_MAC, IS_WINDOWS } from "../platform";
import { failed, toast } from "../app/toast";
import { copyText } from "../app/clipboard";
import { kittyNotes, type Note, osc777Note, osc9Note } from "./attention";
import { notifyIfAway } from "../app/notify";

/**
 * Terminals live here, not in React: switching worktrees remounts the whole workspace, and
 * a running shell (an agent, a dev server) must not notice. Panes mount by moving their
 * host element into whatever container shows them.
 */
interface Pane extends SaveState {
  id: number;
  /** Where it opened, which decides the worktree it belongs to; `dir` is where its shell went since. */
  cwd: string;
  /** Where the shell was last seen: where it started, else as looked up for a split or a save (shellDir). */
  dir: string;
  term: Terminal;
  fit: FitAddon;
  serialize: SerializeAddon;
  /** The history last serialized for the session save (saveRound has when, and what's due). */
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
  /** A column change held back from a long history (fitPane). */
  fitTimer?: number;
  /** The commands shell integration marks. */
  marks: CommandMarks;
  /** A command to type into the shell once it's up (openTerminal). */
  run?: string;
}

interface PaneInfo {
  id: number;
  cwd: string;
  /** What the shell set as the window title (OSC 0/2), if anything. */
  title: string;
  /** It rang or sent a notification while not looked at, and hasn't been since. */
  needsYou?: boolean;
}

/** A tab: one or more panes side by side. */
export interface TerminalGroup {
  id: number;
  /** The user's name for the tab, over the folder's and the program's title. */
  name?: string;
  panes: PaneInfo[];
  focused: number;
}

/** The terminals of a previous run: where each shell was, and what it had printed. */
interface SavedSession {
  savedAt: number;
  active: number;
  groups: { name?: string; focused: number; panes: { cwd: string; dir?: string; history: string }[] }[];
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
  saveTimer ??= window.setTimeout(async () => {
    const due = dueForSave(panes.values(), Date.now(), false);
    // Where their shells are now, asked before they're saved (a reload's save keeps the last answer).
    await Promise.all(due.map(shellDir));
    saveTimer = undefined;
    saveSession(false, due.filter((p) => panes.has(p.id)));
  }, SAVE_MS);
}

// Out of sight (a reload, a quit (lib/app/quit), the window hidden) a stall goes unseen: every changed pane is saved.
window.addEventListener("pagehide", () => saveSession(true));
document.addEventListener("visibilitychange", () => document.hidden && saveSession(true));

/** The layout last written, to skip a write that changes nothing (a title changing, a pane still printing). */
let written = "";

/** `due`: the panes whose history this round saves (dueForSave), when already picked. */
function saveSession(all = false, due?: Pane[]) {
  // Nothing opened yet: keep the last run's terminals for the restore offer.
  if (!state.groups.length && state.restorable) return;
  try {
    if (!state.groups.length) {
      written = "";
      return localStorage.removeItem(SESSION_KEY);
    }
    const now = Date.now();
    const saving = due ?? dueForSave(panes.values(), now, all);
    for (const p of saving) {
      // Alt-screen apps and terminal modes (mouse, bracketed paste) would leak into the new shell.
      p.saved = p.serialize.serialize({ scrollback: HISTORY_LINES, excludeAltBuffer: true, excludeModes: true });
      [p.dirty, p.serializedAt] = [false, now];
    }
    const snapshot = (history: boolean): SavedSession => ({
      savedAt: now,
      active: Math.max(0, state.groups.findIndex((g) => g.id === state.active)),
      groups: state.groups.map((g) => ({
        name: g.name,
        focused: Math.max(0, g.panes.findIndex((p) => p.id === g.focused)),
        panes: g.panes.map(({ id, cwd }) => {
          const dir = panes.get(id)?.dir;
          return { cwd, dir: dir !== cwd ? dir : undefined, history: (history && panes.get(id)?.saved) || "" };
        }),
      })),
    });
    const layout = JSON.stringify({ ...snapshot(false), savedAt: 0 });
    if (saving.length || layout !== written) {
      try {
        localStorage.setItem(SESSION_KEY, JSON.stringify(snapshot(true)));
      } catch {
        // Over quota: the layout alone is still worth keeping.
        localStorage.setItem(SESSION_KEY, JSON.stringify(snapshot(false)));
      }
      written = layout;
    }
    // Another round for panes left for later, until each is saved.
    if ([...panes.values()].some((p) => p.dirty)) scheduleSave();
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
// Every setting notifies (the viewer's image toggle too), and xterm takes even an equal new theme
// object as a change: it rebuilt its glyph atlas and redrew each pane. The rest is set each time:
// a pane's macOptionIsMeta may differ from the settings' (left ⌥ held), and equal values are no-ops.
let appliedTheme = "";
subscribeSettings(() => {
  const { theme, ...rest } = terminalOptions();
  const key = JSON.stringify(theme);
  const next = key === appliedTheme ? rest : { ...rest, theme };
  appliedTheme = key;
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

/** `dir`: where the shell starts, when that's not `cwd` (a split, a restore). */
function createPane(cwd: string, restored?: { history: string; savedAt: number }, dir = cwd): PaneInfo {
  const id = nextId++;
  // The proposed API is the decorations, which find marks its matches with. The kitty keyboard
  // protocol is for programs that turn it on (Claude Code, Codex, neovim, fish 4): Shift+Enter is its
  // own key there, where the legacy encoding sends Enter. zsh and bash don't, so they get the keys as before.
  const term = new Terminal({ ...terminalOptions(), cursorBlink: true, scrollback: 10_000, allowProposedApi: true, vtExtensions: { kittyKeyboard: true } });
  const fit = new FitAddon();
  term.loadAddon(fit);
  const serialize = new SerializeAddon();
  term.loadAddon(serialize);
  // Emoji and newer symbols take two cells, as in wcwidth and other terminals: at xterm's own
  // Unicode 6 widths, TUIs drew out of line and the cursor landed one cell off per emoji.
  term.loadAddon(new Unicode11Addon());
  term.unicode.activeVersion = "11";
  // Relative paths from where the shell starts (a split may start below the worktree).
  terminalLinks(term, dir);
  const search = new SearchAddon();
  term.loadAddon(search);
  search.onDidChangeResults(({ resultIndex, resultCount }) => searching?.pane === p && searching.onResults({ index: resultIndex + 1, total: resultCount }));
  const host = document.createElement("div");
  host.style.cssText = "width:100%;height:100%";
  const p: Pane = { id, cwd, dir, term, fit, serialize, saved: restored?.history ?? null, serializedAt: 0, dirty: false, wroteAt: 0, search, gl: null, glContext: null, host, pty: null, started: false, pending: "", writing: false, marks: new CommandMarks(term) };
  panes.set(id, p);
  if (restored?.history) term.write(`${restored.history}\x1b[0m\r\n\x1b[2m── Restored from ${new Date(restored.savedAt).toLocaleString()} ──\x1b[0m\r\n`);
  term.onWriteParsed(() => {
    [p.dirty, p.wroteAt] = [true, Date.now()];
    scheduleSave();
  });
  term.onData((data) => send(p, data));
  term.onResize(({ cols, rows }) => {
    // Reflow rewraps the history.
    p.dirty = true;
    scheduleSave();
    if (p.pty !== null) void pty.resize(p.pty, cols, rows).catch(() => {});
  });
  term.onTitleChange((title) => update(id, (info) => ({ ...info, title })));
  watchAttention(p);
  // OSC 52 copies (clipboard.rs writes macOS's and Linux's only). Taken from the pane in use alone,
  // where a yank or a tmux copy happens: output in the background can't replace the clipboard.
  if (!IS_WINDOWS)
    term.parser.registerOscHandler(52, (data) => {
      if (!state.open || activeGroup()?.focused !== id || !document.hasFocus()) return true;
      const text = osc52Text(data);
      if (text !== null) copyFromProgram(text);
      return true;
    });
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
  // their commands, and the panel's own keys stay with it whatever they're rebound to. Unbound,
  // ⌘Home/End/PgUp/PgDn (Ctrl+Home/End elsewhere) scroll the history.
  term.attachCustomKeyEventHandler((e) => {
    // xterm's Meta ⌥ can't tell left from right: for the left one only, it's set as a key is typed
    // with ⌥ (xterm reads it after this), and only when the side changed, as a change redraws.
    if (getSettings().optionAsMeta === "left") {
      if (e.code === "AltLeft") leftOptionDown = e.type === "keydown";
      else if (e.altKey && e.key !== "Alt" && term.options.macOptionIsMeta !== leftOptionDown) term.options.macOptionIsMeta = leftOptionDown;
    }
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
    // ⌘A selects the terminal's text, as in VS Code, iTerm2 and Ghostty; the webview's own select
    // all only reached xterm's hidden text area.
    if (IS_MAC && e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey && letter === "a" && !appRunsFromTerminal(e) && !commandIn(TERMINAL_COMMANDS, e)) {
      if (e.type === "keydown") term.selectAll();
      e.preventDefault();
      return false;
    }
    // ⌘↑ / ⌘↓ between the marked prompts. A full-screen program keeps the keys, and so does a
    // shell with no marks (Ctrl+↑/↓ off macOS). Plain typing isn't looked up.
    const jump = (e.metaKey || e.ctrlKey || e.altKey) && commandIn(JUMP_COMMANDS, e);
    if (jump && term.buffer.active.type === "normal" && p.marks.hasCommands()) {
      if (e.type === "keydown") p.marks.jump(jump === "terminal.prevCommand" ? -1 : 1);
      e.preventDefault();
      return false;
    }
    const scroll = scrollKey(e);
    if (scroll && term.buffer.active.type === "normal" && !appRunsFromTerminal(e) && !commandIn(TERMINAL_COMMANDS, e)) {
      if (e.type === "keydown") scroll(term);
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

let copiedFromProgram = false;
/** OSC 52. Said once a run: a program over SSH, or a file being `cat`, can write the clipboard too. */
function copyFromProgram(text: string) {
  const first = !copiedFromProgram;
  copiedFromProgram = true;
  pty.copy(text).then(() => first && toast("info", "Copied from the terminal", "A program in the terminal put text on the clipboard."), failed("Could not copy"));
}

/** Whether the left ⌥ is held, for the left-only Meta setting. Its release may go to another window. */
let leftOptionDown = false;
window.addEventListener("blur", () => (leftOptionDown = false));

/** `fallback`: the webview's own text, pasted if the native read fails or finds nothing. */
async function pasteInto(p: Pane, fallback = "") {
  const got = await pty.paste().catch((e) => {
    if (!fallback) toast("error", "Could not paste", errorMessage(e));
    return null;
  });
  if (!got || got.kind === "empty") {
    if (fallback) await pasteText(p, fallback);
    return;
  }
  if (got.kind === "text") await pasteText(p, got.text);
  else if (got.kind === "files") pastePaths(p, got.paths);
  else if (got.kind === "image") pastePaths(p, [got.path]);
}

/** Several lines into a program without bracketed paste run one by one as they arrive: asked first, as in VS Code. */
async function pasteText(p: Pane, text: string) {
  // Its shell is gone (exited).
  if (p.term.options.disableStdin) return;
  const lines = pastedLines(text);
  if (lines > 1 && !p.term.modes.bracketedPasteMode) {
    const ok = await ask(`The program in this terminal takes a paste as typed keys, so each of the ${lines} lines runs as it arrives.`, { title: `Paste ${lines} lines`, kind: "warning", okLabel: "Paste" });
    if (!ok || !panes.has(p.id)) return;
  }
  p.term.paste(text);
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

const JUMP_COMMANDS = ["terminal.prevCommand", "terminal.nextCommand"] as const satisfies readonly CommandId[];

/** ⌘Home/End/PgUp/PgDn, as in Ghostty and VS Code; a full-screen program's keys stay its own. */
const SCROLL_KEYS: Record<string, (term: Terminal) => void> = {
  Home: (t) => t.scrollToTop(),
  End: (t) => t.scrollToBottom(),
  PageUp: (t) => t.scrollPages(-1),
  PageDown: (t) => t.scrollPages(1),
};

/**
 * Off macOS only Ctrl+Home/End: Ctrl+PgUp/PgDn switch tabs in other terminals, and xterm pages with
 * Shift+PgUp/PgDn itself, as VS Code does.
 */
function scrollKey(e: KeyboardEvent) {
  const alone = IS_MAC ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey && (e.key === "Home" || e.key === "End");
  return alone && !e.shiftKey && !e.altKey ? SCROLL_KEYS[e.key] : undefined;
}

function lineEditKey(e: KeyboardEvent): string | undefined {
  if (e.shiftKey || e.ctrlKey || e.metaKey === e.altKey) return undefined;
  return LINE_EDIT[`${e.metaKey ? "cmd" : "alt"}+${e.key}`];
}

function update(id: number, fn: (p: PaneInfo) => PaneInfo) {
  set({ groups: state.groups.map((g) => (g.panes.some((p) => p.id === id) ? { ...g, panes: g.panes.map((p) => (p.id === id ? fn(p) : p)) } : g)) });
}

/** History lines past which a column change waits for the resize to settle (VS Code's threshold). */
const REWRAP_LINES = 200;

/** A hidden or collapsed container would shrink the shell to one row and garble its output. */
function fitPane(p: Pane, now = false) {
  const box = p.host.parentElement;
  if (!p.term.element || !box || box.clientWidth === 0 || box.clientHeight === 0) return;
  const size = p.fit.proposeDimensions();
  if (!size || isNaN(size.cols) || isNaN(size.rows)) return;
  window.clearTimeout(p.fitTimer);
  // New columns rewrap the whole history, so past REWRAP_LINES they wait 100 ms for a drag to
  // settle, as in VS Code; rows follow at once.
  if (!now && p.started && size.cols !== p.term.cols && p.term.buffer.normal.length > REWRAP_LINES) {
    p.term.resize(p.term.cols, size.rows);
    p.fitTimer = window.setTimeout(() => fitPane(p, true), 100);
  } else p.term.resize(size.cols, size.rows);
  if (!p.started) void start(p);
}

async function start(p: Pane) {
  p.started = true;
  try {
    const { cols, rows } = p.term;
    const began = performance.now();
    const { id, integrated } = await pty.spawn(p.cwd, p.dir !== p.cwd ? p.dir : null, cols, rows, getSettings().shellIntegration, (bytes) => p.term.write(new Uint8Array(bytes)), (exit) => exited(p, exit, performance.now() - began));
    // Closed while it was starting.
    if (!panes.has(p.id)) return void pty.kill(id).catch(() => {});
    p.pty = id;
    // A resize while it was starting had no shell to reach.
    if (p.term.cols !== cols || p.term.rows !== rows) void pty.resize(id, p.term.cols, p.term.rows).catch(() => {});
    send(p, "");
    if (p.run) runAtPrompt(p, `${p.run}\r`, integrated);
  } catch (e) {
    p.term.write(`\x1b[31m${errorMessage(e)}\x1b[0m\r\n`);
  }
}

/**
 * Typed at the shell's first prompt when shell integration says when that is, so an rc file that
 * reads the terminal or clears it can't take it; else (or after RUN_WAIT) typed ahead as it starts.
 */
function runAtPrompt(p: Pane, command: string, integrated: boolean) {
  p.run = undefined;
  if (!integrated) return send(p, command);
  let sent = false;
  const type = () => {
    if (sent || !panes.has(p.id)) return;
    sent = true;
    // Whatever was typed before the prompt is on its line: ⌃U clears it first. VS Code sends ⌃C
    // before a command when the line may hold text; ⌃U does it without a new prompt.
    send(p, `\x15${command}`);
  };
  void p.marks.ready.then(type);
  window.setTimeout(type, RUN_WAIT);
}
/** How long a command waits for the first prompt, as VS Code's shell integration timeout: past it, typing ahead is no worse. */
const RUN_WAIT = 5000;

/** A shell gone within a second (a broken rc file or login shell) leaves its pane up to be read, for ⌘W to close. */
function exited(p: Pane, exit: PtyExit | null, lived: number) {
  if (!panes.has(p.id)) return;
  if (lived > 1000) return closePane(p.id);
  p.pty = null;
  p.term.options.disableStdin = true;
  const how = exit?.signal ? `: ${exit.signal}` : exit?.code != null ? ` with code ${exit.code}` : "";
  p.term.write(`\r\n\x1b[2m[shell exited${how}]\x1b[0m\r\n`);
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

/** `run`: typed into the shell at its first prompt (runAtPrompt), which stays once the command exits. */
export function openTerminal(cwd: string, run?: string) {
  const pane = createPane(cwd);
  if (run) panes.get(pane.id)!.run = run;
  const group = { id: nextId++, panes: [pane], focused: pane.id };
  set({ open: true, groups: [...state.groups, group], active: group.id });
  focusActive();
}

/**
 * Where a pane's shell is now, asked of its process as VS Code's inherited split folder is (only
 * on a split or a save); else where it was last seen.
 */
async function shellDir(p: Pane) {
  if (p.pty !== null) p.dir = (await pty.cwd(p.pty).catch(() => null)) ?? p.dir;
  return p.dir;
}

/** Adds a pane beside the focused one, where its shell is now, in the same worktree. */
export async function splitActive() {
  const from = panes.get(activeGroup()?.focused ?? -1);
  if (!from) return;
  const dir = await shellDir(from);
  // Read again after the lookup, which the tabs may have moved on from.
  const g = state.groups.find((x) => x.panes.some((p) => p.id === from.id));
  if (!g) return;
  const at = g.panes.findIndex((p) => p.id === from.id);
  const pane = createPane(g.panes[at].cwd, undefined, dir);
  const next = { ...g, panes: [...g.panes.slice(0, at + 1), pane, ...g.panes.slice(at + 1)], focused: pane.id };
  set({ groups: state.groups.map((x) => (x.id === g.id ? next : x)) });
  focusActive();
}

/** `byUser`: ⌘W, a tab's ✕ and the like, rather than the shell exiting. */
function closePane(id: number, byUser = false) {
  const p = panes.get(id);
  if (!p) return;
  const focused = document.activeElement;
  panes.delete(id);
  window.clearTimeout(p.fitTimer);
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
  // A shell exiting moves only focus it took away (the commit box keeps it); the user's close also
  // moves it on from the panel's buttons, but ⌫ on a tab stays on the tabs.
  requestAnimationFrame(() => {
    const el = document.activeElement;
    const move = !el || el === document.body ? byUser || (focused && focused !== document.body) : byUser && focusedPanel() === "terminal" && !el.closest('[role="tab"]');
    if (!move) return;
    if (groups.length) focusActive();
    else focusPanel("code");
  });
}

let killing = false;
/**
 * Kills `ids` (`what` to the user), asking first when one of them runs a command (an agent, a dev
 * server), never for a shell at its prompt. A second ⌘W while that's checked or asked does nothing.
 */
async function kill(ids: number[], what: string, title: string) {
  if (killing || !ids.length) return false;
  killing = true;
  try {
    const ptys = ids.flatMap((id) => panes.get(id)?.pty ?? []);
    const busy = ptys.length ? await pty.busy(ptys).catch(() => 0) : 0;
    if (busy && !(await ask(`Killing ${what} stops ${plural(busy, "command")} still running.`, { title, kind: "warning", okLabel: "Kill" }))) return false;
  } finally {
    killing = false;
  }
  for (const id of ids) closePane(id, true);
  return true;
}

export async function closeFocused() {
  const id = activeGroup()?.focused;
  if (id !== undefined) await kill([id], "this terminal", "Kill terminal");
}

/** False when the user kept it. */
export function closeGroup(id: number) {
  return kill(state.groups.find((g) => g.id === id)?.panes.map((p) => p.id) ?? [], "this terminal", "Kill terminal");
}

export async function closeOtherGroups(id: number) {
  await kill(state.groups.flatMap((g) => (g.id === id ? [] : g.panes.map((p) => p.id))), "the other terminals", "Kill other terminals");
}

/** Names a tab; an empty name gives it back the folder's. */
export function renameGroup(id: number, name: string) {
  const trimmed = name.trim();
  set({ groups: state.groups.map((g) => (g.id === id ? { ...g, name: trimmed || undefined } : g)) });
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

export async function clearFocused() {
  const p = panes.get(activeGroup()?.focused ?? -1);
  if (!p) return;
  // A running program (Claude Code, vim) is left alone, as in cmux: it redraws relative to the rows
  // it drew, and clear() pulls the cursor's row to the top under it, so half its screen went missing.
  if (p.term.buffer.active.type === "alternate") return;
  const busy = p.pty !== null && (await pty.busy([p.pty]).catch(() => 0)) > 0;
  if (busy || !panes.has(p.id)) return;
  p.term.clear();
  // clear() skips the parser, so onWriteParsed doesn't mark the pane.
  p.dirty = true;
  scheduleSave();
}

/** What a pane's menu can do now: copy a selection, paste, and reach the last command's output. */
export function paneMenuState(id: number) {
  const p = panes.get(id);
  return { selection: !!p?.term.hasSelection(), paste: !!p && !p.term.options.disableStdin, output: !!p?.marks.hasOutput() };
}

export function copyPaneSelection(id: number) {
  const text = panes.get(id)?.term.getSelection();
  if (text) navigator.clipboard.writeText(text).catch(failed("Could not copy"));
}

export function pasteIntoPane(id: number) {
  const p = panes.get(id);
  if (p) void pasteInto(p);
}

/** The last command's output, from shell integration's marks (commandMarks.ts). */
export function copyLastOutput(id: number) {
  const text = panes.get(id)?.marks.lastOutput();
  if (text) void copyText(text, "Output copied", plural(text.split("\n").length, "line"));
  else if (text === "") toast("info", "Nothing to copy", "The last command printed nothing.");
}

export function selectLastOutput(id: number) {
  panes.get(id)?.marks.selectLastOutput();
}

/** `focus: false` keeps focus where it is: arrowing along the tabs. */
export function activateGroup(id: number, focus = true) {
  if (state.active !== id) set({ active: id });
  if (focus) focusActive();
}

// From a tab, focus stays on the tabs (the panel moves it to the one opened), as ←/→ keep it.
const pick = (id: number) => activateGroup(id, !document.activeElement?.closest('[role="tab"]'));

/** ⌘1–⌘9 with focus in the panel: the tab at `i`, -1 the last; undefined while the panel shows none. */
export function goGroup(i: number) {
  if (!state.open || !state.groups.at(i)) return undefined;
  return () => {
    const g = state.groups.at(i);
    if (g) pick(g.id);
  };
}

/** Next / previous tab with focus in the panel, wrapping; undefined while the panel shows fewer than two. */
export function stepGroup(dir: 1 | -1) {
  if (!state.open || state.groups.length < 2) return undefined;
  return () => {
    const n = state.groups.length;
    const at = state.groups.findIndex((g) => g.id === state.active);
    pick(state.groups[(at + dir + n) % n].id);
  };
}

/** Moves a tab one place along (⌥←/⌥→ on the tabs, as on the code view's). */
export function moveGroup(id: number, dir: 1 | -1) {
  const i = state.groups.findIndex((g) => g.id === id);
  if (i >= 0 && state.groups[i + dir]) set({ groups: arrayMove(state.groups, i, i + dir) });
}

function focusPane(id: number) {
  lookedAt(id);
  const g = state.groups.find((x) => x.panes.some((p) => p.id === id));
  if (!g || (g.focused === id && state.active === g.id)) return;
  set({ active: g.id, groups: state.groups.map((x) => (x === g ? { ...g, focused: id } : x)) });
}

/**
 * A bell, or a notification escape (OSC 9, 777, 99: Claude Code, Codex), from a pane not being
 * looked at marks it, its tab and its worktree, and tells the OS when the app is in the background
 * and the user turned that on. Once until it's looked at: a program ringing on and on is one mark.
 */
function watchAttention(p: Pane) {
  p.term.onBell(() => needsYou(p));
  const readers: [number, (data: string) => Note | null][] = [[9, osc9Note], [777, osc777Note], [99, kittyNotes()]];
  for (const [code, read] of readers)
    p.term.parser.registerOscHandler(code, (data) => {
      const note = read(data);
      if (note) needsYou(p, note);
      // Not a notification (OSC 9;4 is a progress bar): left to any other handler.
      return !!note;
    });
}

function needsYou(p: Pane, note?: Note) {
  const g = state.groups.find((x) => x.panes.some((i) => i.id === p.id));
  const info = g?.panes.find((i) => i.id === p.id);
  if (!g || !info || info.needsYou || (document.hasFocus() && document.activeElement === p.term.textarea)) return;
  update(p.id, (i) => ({ ...i, needsYou: true }));
  const text = [note?.title, note?.body].filter(Boolean).join(": ");
  notifyIfAway(g.name ?? (folderName(p.cwd) || p.cwd), text || info.title || "Needs you");
}

function lookedAt(id: number) {
  if (state.groups.some((g) => g.panes.some((p) => p.id === id && p.needsYou))) update(id, (p) => ({ ...p, needsYou: false }));
}

// Back in the window, the pane that has the keys is looked at again.
window.addEventListener("focus", () => {
  for (const p of panes.values()) if (document.activeElement === p.term.textarea) lookedAt(p.id);
});

/** Folders of the panes that need the user, "\0"-joined: a string, so a hook re-renders only when it changes. */
const needing = () =>
  state.groups
    .flatMap((g) => g.panes.filter((p) => p.needsYou).map((p) => p.cwd))
    .sort()
    .join("\0");

/** The folders of panes that need the user (needsYou), for the worktree picker's marks. */
export function useNeedsYou() {
  const key = useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    needing,
  );
  return useMemo(() => (key ? key.split("\0") : []), [key]);
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
    const infos = g.panes.map((p) => createPane(p.cwd, { history: p.history, savedAt: saved.savedAt }, typeof p.dir === "string" ? p.dir : undefined));
    return { id: nextId++, name: typeof g.name === "string" ? g.name : undefined, panes: infos, focused: (infos[g.focused] ?? infos[0]).id };
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
  for (const p of panes.values()) {
    p.cwd = moved(p.cwd);
    p.dir = moved(p.dir);
  }
  set({ groups: state.groups.map((g) => ({ ...g, panes: g.panes.map((p) => ({ ...p, cwd: moved(p.cwd) })) })) });
}

/** Switching to a worktree brings up a terminal that's already in it. */
export function showWorktree(cwd: string) {
  if (activeGroup()?.panes.some((p) => p.cwd === cwd)) return;
  const g = state.groups.find((x) => x.panes.some((p) => p.cwd === cwd));
  if (g) set({ active: g.id });
}

/** Whether the panel is open, alone: useTerminals re-renders on every title a program sets. */
export function useTerminalsOpen() {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => state.open,
  );
}
