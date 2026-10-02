import { arrayMove } from "@dnd-kit/sortable";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { SerializeAddon } from "@xterm/addon-serialize";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useSyncExternalStore } from "react";
import { errorMessage, pty, type PtyExit } from "../api";
import { type CommandId } from "../commands/keybindings";
import { terminalLinks } from "../links/linkHost";
import { getSettings, stepTerminalFont, subscribeSettings } from "../settings";
import { isInside } from "../path";
import { focusedPanel, focusPanel, setTerminalFocus } from "../ui/panels";
import { terminalOptions } from "./theme";
import { osc52Text } from "./osc52";
import { CommandMarks } from "./commandMarks";
import { type SaveState } from "./saveRound";
import { planFit } from "./fit";
import { ask } from "../app/ask";
import { plural } from "../format";
import { IS_LINUX, IS_MAC, IS_WINDOWS } from "../platform";
import { failed, toast } from "../app/toast";
import { copyText } from "../app/clipboard";
import { loadSession, type SavedSession, scheduleSave } from "./session";
import { lookedAt, watchAttention } from "./needsYou";
import { agentPrompted } from "./agents";
import { type PaneAgent } from "./agentState";
import { reportWheelByRow } from "./wheel";
import { paneKeys } from "./keys";
import { forgetFind, watchFind } from "./find";
import { copyFromProgram, pasteInto, pasteText } from "./pasteInput";
import { type Direction, type Layout, neighbor, removePane, resize, type Split, splitPane } from "./layout";

export { dismissRestore, restoreSession, resumable } from "./session";
export { useNeedsYou } from "./needsYou";
export { useAgentsWorking } from "./agents";
export { clearFind, endFind, findInTerminal } from "./find";

/**
 * Terminals live here, not in React: switching worktrees remounts the whole workspace, and
 * a running shell (an agent, a dev server) must not notice. Panes mount by moving their
 * host element into whatever container shows them.
 */
export interface Pane extends SaveState {
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
  pending: Input[];
  writing: boolean;
  /** Output parsed and not yet acked to pty.rs (parsed). */
  unacked: number;
  /** A column change held back from a long history (fitPane). */
  fitTimer?: number;
  /** The size the pty is sent once a resize settles. */
  ptyResizeTimer?: number;
  /** The commands shell integration marks. */
  marks: CommandMarks;
  /** Typed into the shell once it's up, with its Enter if it ends with one (openTerminal, a restored agent). */
  run?: string;
}

interface PaneInfo {
  id: number;
  cwd: string;
  /** What the shell set as the window title (OSC 0/2), if anything. */
  title: string;
  /** It rang or sent a notification while not looked at, and hasn't been since. */
  needsYou?: boolean;
  /** The user's name for it, over the title in its split header. */
  name?: string;
  /** The coding agent running in it (agents.ts), until the shell prompts again. */
  agent?: PaneAgent;
}

/** A tab: one or more panes, split right and down. */
export interface TerminalGroup {
  id: number;
  /** The user's name for the tab, over the folder's and the program's title. */
  name?: string;
  /** In the layout's reading order. */
  panes: PaneInfo[];
  layout: Layout;
  focused: number;
}

interface State {
  open: boolean;
  groups: TerminalGroup[];
  active: number | null;
  /** Last run's terminals, until the user restores or dismisses them. */
  restorable: SavedSession | null;
  /** The panel covers the whole workspace, its code view and side panels hidden behind it. */
  maximized: boolean;
  /** The open tab shows its focused pane alone, the others running on behind it. */
  zoomed: boolean;
}

/** Keys the terminal panel runs while it has focus (TerminalPanel), rather than the shell. */
export const TERMINAL_COMMANDS = [
  "terminal.split",
  "terminal.splitDown",
  "terminal.clear",
  "terminal.close",
  "terminal.prevPane",
  "terminal.nextPane",
  "terminal.focusLeft",
  "terminal.focusRight",
  "terminal.focusUp",
  "terminal.focusDown",
  "terminal.toggleMaximize",
  "terminal.zoomPane",
  "terminal.renamePane",
  "terminal.fontZoomIn",
  "terminal.fontZoomOut",
  "terminal.fontZoomReset",
] as const satisfies readonly CommandId[];

export const panes = new Map<number, Pane>();
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
export let state: State = { open: false, groups: [], active: null, restorable: loadSession(), maximized: false, zoomed: false };
let nextId = 1;
export const newId = () => nextId++;
const listeners = new Set<() => void>();

export function subscribe(l: () => void) {
  listeners.add(l);
  return () => void listeners.delete(l);
}

export function set(patch: Partial<State>) {
  state = { ...state, ...patch };
  listeners.forEach((l) => l());
  scheduleSave();
}

export function useTerminals() {
  return useSyncExternalStore(subscribe, () => state);
}

export const activeGroup = () => state.groups.find((g) => g.id === state.active);

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

/** `binary`: xterm's onBinary bytes, a char each, which aren't text to encode as UTF-8. */
interface Input {
  data: string;
  binary: boolean;
}

function send(p: Pane, data: string, binary = false) {
  const last = p.pending.at(-1);
  if (last?.binary === binary) last.data += data;
  else if (data) p.pending.push({ data, binary });
  if (p.writing) return;
  const flush = () => {
    if (!p.pending.length || p.pty === null) {
      p.writing = false;
      return;
    }
    const { data, binary } = p.pending.shift()!;
    p.writing = true;
    pty.write(p.pty, data, binary)
      .catch(() => {})
      .finally(flush);
  };
  flush();
}

/** How long a pane's size holds still before its program hears of it. */
const PTY_RESIZE_WAIT = 50;

/**
 * `dir`: where the shell starts, when that's not `cwd` (a split, a restore). `resume`: a restored
 * agent's command for the first prompt, and the line that says so.
 */
export function createPane(cwd: string, restored?: { history: string; savedAt: number; resume?: { hint: string; run: string } }, dir = cwd): PaneInfo {
  const id = nextId++;
  // The proposed API is the decorations, which find marks its matches with. The kitty keyboard
  // protocol is for programs that turn it on (Claude Code, Codex, neovim, fish 4): Shift+Enter is its
  // own key there, where the legacy encoding sends Enter. zsh and bash don't, so they get the keys as before.
  const term = new Terminal({ ...terminalOptions(), allowProposedApi: true, vtExtensions: { kittyKeyboard: true } });
  const fit = new FitAddon();
  term.loadAddon(fit);
  const serialize = new SerializeAddon();
  term.loadAddon(serialize);
  // Emoji and newer symbols take two cells, as in wcwidth and other terminals: at xterm's own
  // Unicode 6 widths, TUIs drew out of line and the cursor landed one cell off per emoji.
  term.loadAddon(new Unicode11Addon());
  term.unicode.activeVersion = "11";
  // Relative paths from the shell's folder, which a split may start below the worktree in.
  terminalLinks(term, () => panes.get(id)?.dir ?? dir);
  const search = new SearchAddon();
  term.loadAddon(search);
  const host = document.createElement("div");
  host.style.cssText = "width:100%;height:100%";
  // Each prompt: where a `cd` settled, and the end of any agent that ran.
  const prompted = () => {
    void shellDir(p);
    agentPrompted(p);
  };
  const p: Pane = { id, cwd, dir, term, fit, serialize, saved: restored?.history ?? null, serializedAt: 0, dirty: false, wroteAt: 0, search, gl: null, glContext: null, host, pty: null, started: false, pending: [], writing: false, unacked: 0, marks: new CommandMarks(term, prompted) };
  panes.set(id, p);
  if (restored?.history) term.write(`${restored.history}\x1b[0m\r\n\x1b[2m── Restored from ${new Date(restored.savedAt).toLocaleString()} ──\x1b[0m\r\n`);
  if (restored?.resume) {
    p.run = restored.resume.run;
    term.write(`\x1b[2m${restored.resume.hint}\x1b[0m\r\n`);
  }
  term.onWriteParsed(() => {
    [p.dirty, p.wroteAt] = [true, Date.now()];
    scheduleSave();
  });
  term.onData((data) => send(p, data));
  // Mouse reports in the default encoding: a byte a coordinate, past 127 beyond column 95, not UTF-8.
  term.onBinary((data) => send(p, data, true));
  term.onResize(() => {
    // Reflow rewraps the history.
    p.dirty = true;
    scheduleSave();
    // A divider drag changes the rows each frame, and Claude Code redrew on each SIGWINCH, leaving
    // copies of its screen in the history: the program gets the size the drag settles on.
    window.clearTimeout(p.ptyResizeTimer);
    p.ptyResizeTimer = window.setTimeout(() => sendSize(p), PTY_RESIZE_WAIT);
  });
  term.onTitleChange((title) => update(id, (info) => ({ ...info, title })));
  watchAttention(p);
  watchFind(p);
  // A mouse wheel's notch arrives as ~53-100 px off macOS, so a row per row would scroll vim or
  // htop 3-5 rows a notch where it scrolled one; the replay is for the Mac trackpad it was made on.
  if (IS_MAC) reportWheelByRow(term);
  // ⌥-drag selects past a program that reads the mouse, which Claude Code's "option+click to native
  // select" counts on. Forcing it always would cost ⌥-drag's block selection where none reads it.
  if (IS_MAC) host.addEventListener("mousedown", () => (term.options.macOptionClickForcesSelection = term.modes.mouseTrackingMode !== "none"), true);
  // XTVERSION names this app, as Ghostty and iTerm2 name themselves. Answered as xterm.js, Claude
  // Code took the pane for VS Code's terminal: 3 rows a report once the wheel slowed, and its
  // workarounds for VS Code's glyph atlas.
  term.parser.registerCsiHandler({ prefix: ">", final: "q" }, (params) => {
    if (params[0]) return false;
    term.input("\x1bP>|GitViber\x1b\\", false);
    return true;
  });
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
  host.addEventListener("wheel", pinchFont, { capture: true, passive: false });
  term.attachCustomKeyEventHandler(paneKeys(p));
  return { id, cwd, title: "" };
}

/**
 * A pinch (a trackpad's arrives as ctrl+wheel) or Ctrl+wheel sizes the font, as in iTerm2 and
 * Windows Terminal, rather than scrolling or zooming the page. A pinch sends many small deltas, a
 * mouse notch one big one: either way about a point per notch.
 */
let pinched = 0;
/** Pixels a line-mode wheel's line counts as. */
const PX_PER_LINE = 16;
/** Pixels of pinch (or wheel) a font step takes. */
const PINCH_STEP_PX = 24;
function pinchFont(e: WheelEvent) {
  if (!e.ctrlKey) return;
  e.preventDefault();
  e.stopPropagation();
  pinched += e.deltaMode === WheelEvent.DOM_DELTA_LINE ? e.deltaY * PX_PER_LINE : e.deltaY;
  if (Math.abs(pinched) < PINCH_STEP_PX) return;
  stepTerminalFont(pinched < 0 ? 1 : -1);
  pinched = 0;
}

export function update(id: number, fn: (p: PaneInfo) => PaneInfo) {
  set({ groups: state.groups.map((g) => (g.panes.some((p) => p.id === id) ? { ...g, panes: g.panes.map((p) => (p.id === id ? fn(p) : p)) } : g)) });
}

/** `now`: the columns held back from a long history (planFit) are due. */
function fitPane(p: Pane, now = false) {
  if (!p.term.element) return;
  // The host, not its container, whose padding stays when the pane has no room.
  const box = { width: p.host.clientWidth, height: p.host.clientHeight };
  const plan = planFit(box, p.fit.proposeDimensions(), p.term, now || !p.started ? null : p.term.buffer.normal.length);
  if (!plan) return;
  window.clearTimeout(p.fitTimer);
  p.fitTimer = undefined;
  p.term.resize(plan.size.cols, plan.size.rows);
  if (plan.colsLater)
    p.fitTimer = window.setTimeout(() => {
      p.fitTimer = undefined;
      fitPane(p, true);
    }, 100);
  if (!p.started) void start(p);
}

/** Tells the program its pane's size once a resize settled, columns held back (fitPane) included: one SIGWINCH, not one a step. */
function sendSize(p: Pane) {
  if (p.fitTimer !== undefined) p.ptyResizeTimer = window.setTimeout(() => sendSize(p), PTY_RESIZE_WAIT);
  else if (p.pty !== null) void pty.resize(p.pty, p.term.cols, p.term.rows).catch(() => {});
}

async function start(p: Pane) {
  p.started = true;
  try {
    const { cols, rows } = p.term;
    const began = performance.now();
    const { id, integrated } = await pty.spawn(p.cwd, p.dir !== p.cwd ? p.dir : null, cols, rows, getSettings().shellIntegration, (bytes) => p.term.write(new Uint8Array(bytes), () => parsed(p, bytes.byteLength)), (exit) => exited(p, exit, performance.now() - began));
    // Closed while it was starting.
    if (!panes.has(p.id)) return void pty.kill(id).catch(() => {});
    p.pty = id;
    // What was parsed before the id came back.
    parsed(p, 0);
    // A resize while it was starting had no shell to reach.
    if (p.term.cols !== cols || p.term.rows !== rows) void pty.resize(id, p.term.cols, p.term.rows).catch(() => {});
    send(p, "");
    if (p.run) runAtPrompt(p, p.run, integrated);
  } catch (e) {
    p.term.write(`\x1b[31m${errorMessage(e)}\x1b[0m\r\n`);
  }
}

/** Acks go to pty.rs in steps of this, not an IPC a chunk; well under its 512 KiB high water. */
const ACK_STEP = 64 * 1024;

function parsed(p: Pane, bytes: number) {
  p.unacked += bytes;
  if (p.unacked < ACK_STEP || p.pty === null) return;
  void pty.ack(p.pty, p.unacked).catch(() => {});
  p.unacked = 0;
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
  // The first fit is the observer's, once layout is done. Fitted here, the panel ⌘J brought back
  // wasn't sized yet: one row, then its size again before the pty heard, and Claude Code redrew
  // its screen two rows off what xterm had kept of it.
  const observer = new ResizeObserver(() => fitPane(p));
  observer.observe(container);
  return () => {
    observer.disconnect();
    // WebKit fires no blur for a focused element taken out of the page: the pane would go on drawing
    // its cursor as focused, and a program that asked for focus reports (Claude Code) never hear it left.
    if (p.host.contains(document.activeElement)) p.term.blur();
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
  if (run) panes.get(pane.id)!.run = `${run}\r`;
  const group = { id: nextId++, panes: [pane], layout: pane.id, focused: pane.id };
  set({ open: true, groups: [...state.groups, group], active: group.id });
  focusActive();
}

/**
 * Where a pane's shell is now, asked of its process as VS Code's inherited split folder is (on a
 * split, a save, or a prompt shell integration marks); else where it was last seen.
 */
export async function shellDir(p: Pane) {
  if (p.pty !== null) p.dir = (await pty.cwd(p.pty).catch(() => null)) ?? p.dir;
  return p.dir;
}

/** Adds a pane right of or below the focused one (or `at`), where its shell is now, in the same worktree. */
export async function splitActive(way: Split["dir"], at = activeGroup()?.focused) {
  const from = panes.get(at ?? -1);
  if (!from) return;
  const dir = await shellDir(from);
  // Read again after the lookup, which the tabs may have moved on from.
  const g = state.groups.find((x) => x.panes.some((p) => p.id === from.id));
  if (!g) return;
  const i = g.panes.findIndex((p) => p.id === from.id);
  const pane = createPane(g.panes[i].cwd, undefined, dir);
  // Right after `from` in reading order too (splitPane).
  const next = { ...g, panes: [...g.panes.slice(0, i + 1), pane, ...g.panes.slice(i + 1)], layout: splitPane(g.layout, from.id, pane.id, way), focused: pane.id };
  // A zoomed pane's split shows the two side by side.
  set({ groups: state.groups.map((x) => (x.id === g.id ? next : x)), zoomed: false });
  focusActive();
}

/** `byUser`: ⌘W, a tab's ✕ and the like, rather than the shell exiting. */
function closePane(id: number, byUser = false) {
  const p = panes.get(id);
  if (!p) return;
  const focused = document.activeElement;
  panes.delete(id);
  window.clearTimeout(p.fitTimer);
  window.clearTimeout(p.ptyResizeTimer);
  forgetFind(p);
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
    const layout = removePane(g.layout, id);
    if (layout === null) return [];
    return [{ ...g, panes: rest, layout, focused: g.focused === id ? rest[Math.min(i, rest.length - 1)].id : g.focused }];
  });
  let active = state.active;
  if (!groups.some((g) => g.id === active)) {
    const i = state.groups.findIndex((g) => g.id === active);
    active = groups[Math.min(i, groups.length - 1)]?.id ?? null;
  }
  set({ groups, active, open: state.open && groups.length > 0, maximized: state.maximized && groups.length > 0 });
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
  if (id !== undefined) await killPane(id);
}

export function killPane(id: number) {
  return kill([id], "this terminal", "Kill terminal");
}

/** Keys to a pane, as a click into it gives them. */
export function focusTerminalPane(id: number) {
  panes.get(id)?.term.focus();
}

/** Where a pane's shell was last seen (shellDir). */
export const paneDir = (id: number) => panes.get(id)?.dir;

/** False when the user kept it. */
export function closeGroup(id: number) {
  return kill(state.groups.find((g) => g.id === id)?.panes.map((p) => p.id) ?? [], "this terminal", "Kill terminal");
}

export async function closeOtherGroups(id: number) {
  await kill(state.groups.flatMap((g) => (g.id === id ? [] : g.panes.map((p) => p.id))), "the other terminals", "Kill other terminals");
}

/** Names a split pane; an empty name gives it back the program's title. */
export function renamePane(id: number, name: string) {
  update(id, (p) => ({ ...p, name: name.trim() || undefined }));
}

/** Names a tab; an empty name gives it back the folder's. */
export function renameGroup(id: number, name: string) {
  const trimmed = name.trim();
  set({ groups: state.groups.map((g) => (g.id === id ? { ...g, name: trimmed || undefined } : g)) });
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

/**
 * A program reading the mouse (tmux, vim `mouse=a`) gets the right-click, and shows its own menu;
 * with the key that forces a selection past it (⌥, ⇧ off macOS) the click is the pane's.
 */
export function paneTakesMouse(id: number, e: MouseEvent) {
  const mode = panes.get(id)?.term.modes.mouseTrackingMode;
  return !!mode && mode !== "none" && !(IS_MAC ? e.altKey : e.shiftKey);
}

export function copyPaneSelection(id: number) {
  const text = panes.get(id)?.term.getSelection();
  if (text) navigator.clipboard.writeText(text).catch(failed("Could not copy"));
}

export function pasteIntoPane(id: number) {
  const p = panes.get(id);
  if (p) void pasteInto(p);
}

/**
 * Pastes `text` into the terminal `cwd`'s worktree last had focus in (the open tab's pane, else its
 * first tab's), shown and focused, without pressing Enter. False when the worktree has no terminal.
 */
export function pasteToWorktree(cwd: string, text: string) {
  const here = (g: TerminalGroup | undefined) => !!g && g.panes.some((p) => p.id === g.focused && p.cwd === cwd);
  const g = [activeGroup(), ...state.groups].find(here);
  const p = g && panes.get(g.focused);
  if (!g || !p) return false;
  set({ open: true, active: g.id });
  void pasteText(p, text).then(focusActive);
  return true;
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

/** The next split pane of the open tab, wrapping around. */
export function stepPane(dir: 1 | -1) {
  const g = activeGroup();
  if (!g || g.panes.length < 2) return;
  const at = g.panes.findIndex((p) => p.id === g.focused);
  focusPane(g.panes[(at + dir + g.panes.length) % g.panes.length].id);
  focusActive();
}

/** The open tab's pane on that side of the focused one (⇧⌘ or ⌥⌘ arrows), as drawn: a pane's minimum size can outweigh its saved share. */
export function focusToward(dir: Direction) {
  const g = activeGroup();
  if (!g) return;
  const rects = new Map(g.panes.flatMap(({ id }) => (panes.has(id) ? [[id, panes.get(id)!.host.getBoundingClientRect()] as const] : [])));
  const id = neighbor(rects, g.focused, dir);
  if (id === undefined) return;
  focusPane(id);
  focusActive();
}

/** A divider dragged: the new sizes of the split at `path` (layout.ts resize), kept for the session save. */
export function resizeSplit(group: number, path: number[], sizes: number[]) {
  set({ groups: state.groups.map((g) => (g.id === group ? { ...g, layout: resize(g.layout, path, sizes) } : g)) });
}

/** The panel over the whole workspace, or back in its place; opened first (with a terminal in `cwd`) when hidden. */
export function toggleMaximize(cwd: string) {
  if (!state.maximized && !state.open) togglePanel(cwd);
  set({ maximized: !state.maximized });
  focusActive();
}

/** The open tab's focused pane alone in the panel, or all of them again; the panel opened first as above. */
export function toggleZoom(cwd: string) {
  if (!state.zoomed && !state.open) togglePanel(cwd);
  set({ zoomed: !state.zoomed });
  focusActive();
}

/** Back in its place: another worktree, or a file opened in the code view it covers. */
export function unmaximize() {
  if (state.maximized) set({ maximized: false });
}

// Focus moving to a panel hidden behind the maximized one (F6, ⌘E) brings it back into view.
document.addEventListener("focusin", () => {
  if (!state.maximized) return;
  const panel = focusedPanel();
  if (panel && panel !== "terminal") unmaximize();
});

/** Opens the panel (with a first terminal in `cwd` if there is none), or hides it. */
export function togglePanel(cwd: string) {
  if (state.open) return set({ open: false, maximized: false });
  if (!state.groups.length) return openTerminal(cwd);
  set({ open: true });
  focusActive();
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
  return useSyncExternalStore(subscribe, () => state.open);
}

export function useTerminalsMaximized() {
  return useSyncExternalStore(subscribe, () => state.maximized);
}

/** How many tabs the panel shows, 0 while it's hidden: what decides which of goGroup and stepGroup apply. */
export function useTerminalTabCount() {
  return useSyncExternalStore(subscribe, () => (state.open ? state.groups.length : 0));
}
