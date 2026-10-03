import type { IMarker, Terminal } from "@xterm/xterm";

/**
 * An OSC 133 mark (shell integration): A a prompt starts, B the typed command does, C its output
 * does, D it ended, with its exit code when the shell gives one. C may say what was typed: fish's
 * `cmdline_url=` (percent-encoded, as kitty's), or our zsh's `cmdline=`, the rest of the mark as
 * typed with control characters made spaces.
 */
export function parseMark(data: string): { kind: "A" | "B" | "C" | "D"; exit?: number; command?: string } | null {
  const [kind, arg] = data.split(";");
  if (kind !== "A" && kind !== "B" && kind !== "C" && kind !== "D") return null;
  if (kind === "C") {
    const command = commandLine(data);
    return command ? { kind, command } : { kind };
  }
  return kind === "D" && arg !== undefined && /^\d+$/.test(arg) ? { kind, exit: Number(arg) } : { kind };
}

/** A notification's or a tooltip's worth of it: a long one ends in "…". */
const MAX_COMMAND = 80;

function commandLine(data: string) {
  let text: string | undefined;
  const raw = /;cmdline=([\s\S]*)$/.exec(data);
  if (raw) text = raw[1];
  else {
    const url = /;cmdline_url=([^;]*)/.exec(data);
    try {
      text = url ? decodeURIComponent(url[1]) : undefined;
    } catch {
      // Malformed escapes: no name, rather than a wrong one.
    }
  }
  const line = text?.replace(/\s+/g, " ").trim();
  return line && (line.length > MAX_COMMAND ? `${line.slice(0, MAX_COMMAND - 1)}…` : line);
}

/** How a command ended, from its output's mark (C) to its end (D) or the next prompt. */
export interface CommandEnd {
  /** As typed, when the shell said (zsh and fish do; bash doesn't). */
  command?: string;
  ms: number;
  /** Unknown when the shell was interrupted, or doesn't say. */
  exit?: number;
}

/** "350ms", "4.2s", "45s", "2m 3s", "5m", "1h 5m": how long a command took. */
export function formatDuration(ms: number) {
  if (Math.round(ms) < 1000) return `${Math.round(ms)}ms`;
  const tenths = Math.round(ms / 100);
  if (tenths < 100) return `${tenths / 10}s`;
  const whole = Math.round(ms / 1000);
  if (whole < 60) return `${whole}s`;
  const [h, m, s] = [Math.floor(whole / 3600), Math.floor((whole % 3600) / 60), whole % 60];
  const pair = (a: number, x: string, b: number, y: string) => (b ? `${a}${x} ${b}${y}` : `${a}${x}`);
  return h ? pair(h, "h", m, "m") : pair(m, "m", s, "s");
}

/** The mark's hover, as VS Code words its command decorations. */
export function endTitle({ ms, exit }: CommandEnd) {
  const took = formatDuration(ms);
  if (exit === undefined) return `Ended after ${took}`;
  return exit === 0 ? `Took ${took}` : `Failed after ${took} (exit code ${exit})`;
}

/**
 * The program and its subcommand, for a notification: "pnpm test", "cargo build". Arguments can
 * hold tokens and passwords (`curl -H "Authorization: …"`), and Notification Center keeps what it
 * shows, so the words stop at the first that is a flag, an assignment, a path, a URL or an address,
 * or long; three at most. Leading `NAME=value` words are dropped, and a path is its file name.
 */
export function shortCommand(line: string | undefined) {
  const words = (line ?? "").split(/\s+/).filter(Boolean);
  while (words[0]?.includes("=")) words.shift();
  const program = words.shift()?.split("/").pop();
  if (!program || program.length > 24 || program.startsWith("-")) return undefined;
  const out = [program];
  for (const w of words) {
    if (out.length === 3 || w.length > 24 || /^-|[=/:@]/.test(w)) break;
    out.push(w);
  }
  return out.join(" ");
}

/** What a notification says of a long command: "pnpm test failed after 2m 3s (exit code 1)". */
export function endText({ command, ms, exit }: CommandEnd) {
  const took = formatDuration(ms);
  const what = shortCommand(command) ?? "A command";
  if (exit === undefined) return `${what} ended after ${took}`;
  return exit === 0 ? `${what} finished after ${took}` : `${what} failed after ${took} (exit code ${exit})`;
}


/**
 * What a full-screen program turns off on its way out: ?1000l ends any mouse tracking in xterm.js
 * (9, 1000, 1002 and 1003 alike), ?1006l puts reports back in the default encoding (from SGR and SGR
 * pixels), ?1004l ends focus reports, which the shell would read as typed ^[[I and ^[[O.
 */
const MODES_OFF = "\x1b[?1000l\x1b[?1006l\x1b[?1004l";

/**
 * A prompt (A) in the history's buffer while the mouse is still reported: the program that asked
 * for it died without turning it off (a crash, `kill -9`), and a drag at the prompt would select
 * nothing. A shell under a full-screen program (tmux) prompts in the other buffer. Focus reports and
 * kitty keyboard flags on their own aren't a sign: fish turns them on for its own prompt.
 */
export const mouseLeftOn = (kind: string, buffer: "normal" | "alternate", mouse: string) => kind === "A" && buffer === "normal" && mouse !== "none";

/** Commands kept marked: each marker is updated as lines scroll off or get cleared. */
const MAX_COMMANDS = 300;

interface Command {
  prompt: IMarker;
  /** When its output started (performance.now), and what was typed, if the shell said. */
  started?: number;
  command?: string;
  /** Where its output starts (C), and where it ended (D) with the cursor's column there. */
  output?: IMarker;
  end?: IMarker;
  endX?: number;
}

/**
 * The commands the shell marks with OSC 133, as VS Code's and Ghostty's terminals read them: a
 * marker per prompt, a mark beside it once the command ends (red for a non-zero exit), and the
 * last command's output. Nothing is done per byte: xterm calls in only for the marks.
 */
export class CommandMarks {
  /** Commands that ran, oldest first. A prompt trimmed off the history or cleared has a disposed marker. */
  private commands: Command[] = [];
  /** The one at the prompt now, or running. */
  private current: Command | null = null;
  /** The last one that ended: the only one that keeps its output's markers. */
  private last: Command | null = null;
  private term: Terminal;
  private onPrompt: () => void;
  private onEnd: (end: CommandEnd) => void;
  private prompted = () => {};
  /** The shell's first prompt is up: it reads what's typed now. */
  readonly ready = new Promise<void>((resolve) => (this.prompted = resolve));

  /** `onPrompt`: each prompt, where a `cd` has settled. `onEnd`: each command that ends. */
  constructor(term: Terminal, onPrompt = () => {}, onEnd: (end: CommandEnd) => void = () => {}) {
    this.term = term;
    this.onPrompt = onPrompt;
    this.onEnd = onEnd;
    term.parser.registerOscHandler(133, (data) => {
      const mark = parseMark(data);
      // To xterm, not the shell: as if the program had turned them off.
      if (mark && mouseLeftOn(mark.kind, term.buffer.active.type, term.modes.mouseTrackingMode)) term.write(MODES_OFF);
      if (mark) this.on(mark.kind, mark.exit, mark.command);
      return true;
    });
  }

  private on(kind: string, exit?: number, command?: string) {
    // A shell under a full-screen program (tmux) marks lines that aren't the history's.
    if (this.term.buffer.active.type !== "normal") return;
    if (kind === "A") {
      const c = this.current;
      // No D (the shell was interrupted, or doesn't send one): it ended here, how isn't known.
      if (c?.output && !c.end) this.ended(c);
      // A prompt nothing ran from (an empty line, ^C) isn't kept.
      else if (c && !c.output) c.prompt.dispose();
      this.current = { prompt: this.term.registerMarker(0) };
      this.prompted();
      this.onPrompt();
    } else if (kind === "C" && this.current && !this.current.output) {
      this.current.output = this.term.registerMarker(0);
      // Timed here, as Ghostty times it: from the command's start to its end, not from the prompt.
      this.current.started = performance.now();
      this.current.command = command;
      this.commands = this.commands.filter((x) => !x.prompt.isDisposed);
      this.commands.push(this.current);
      // Its mark goes with its marker.
      if (this.commands.length > MAX_COMMANDS) this.commands.shift()!.prompt.dispose();
    } else if (kind === "D" && this.current?.output && !this.current.end) {
      this.ended(this.current, exit);
      this.current = null;
    }
  }

  private ended(c: Command, exit?: number) {
    c.end = this.term.registerMarker(0);
    c.endX = this.term.buffer.active.cursorX;
    if (this.last) {
      this.last.output?.dispose();
      this.last.end?.dispose();
    }
    this.last = c;
    const failed = exit !== undefined && exit !== 0;
    const end: CommandEnd = { command: c.command, ms: performance.now() - c.started!, exit };
    const mark = this.term.registerDecoration({ marker: c.prompt });
    mark?.onRender((el) => {
      if (el.firstChild) return;
      // The dot sits in the pane's left padding; the cell under the decoration stays clickable.
      el.style.pointerEvents = "none";
      const dot = document.createElement("div");
      dot.className = "gv-command-mark";
      if (failed) dot.dataset.failed = "";
      dot.title = endTitle(end);
      el.appendChild(dot);
    });
    this.onEnd(end);
  }

  /**
   * The prompt of the command above (-1) or below (1) the top of the view scrolled to the top, as
   * Ghostty's jump_to_prompt; below the last one, the bottom.
   */
  jump(dir: 1 | -1) {
    const top = this.term.buffer.active.viewportY;
    const lines = this.commands.flatMap((c) => (c.prompt.isDisposed ? [] : [c.prompt.line]));
    const to = dir < 0 ? lines.filter((l) => l < top).pop() : lines.find((l) => l > top);
    if (to !== undefined) this.term.scrollToLine(to);
    else if (dir > 0) this.term.scrollToBottom();
  }

  /** The rows of the last ended command's output, the last one up to `endX`; null without one. */
  private lastRows() {
    const c = this.last;
    if (!c?.output || !c.end || c.end.isDisposed) return null;
    // Its start scrolled off the history: what's left of it.
    return { start: Math.max(0, c.output.line), end: c.end.line, endX: c.endX ?? 0 };
  }

  /** Whether any command is marked (on screen or in the history), for the jump keys. */
  hasCommands() {
    return this.commands.some((c) => !c.prompt.isDisposed);
  }

  hasOutput() {
    return this.lastRows() !== null;
  }

  lastOutput(): string | null {
    const rows = this.lastRows();
    if (!rows) return null;
    const buffer = this.term.buffer.normal;
    let text = "";
    for (let y = rows.start; y <= rows.end; y++) {
      const line = buffer.getLine(y);
      if (!line) break;
      // A wrapped row continues the line above it.
      if (y > rows.start && !line.isWrapped) text += "\n";
      text += y === rows.end ? line.translateToString(true, 0, rows.endX) : line.translateToString(true);
    }
    return text.replace(/\n+$/, "");
  }

  selectLastOutput() {
    const rows = this.lastRows();
    if (!rows) return;
    const end = rows.endX > 0 ? rows.end : rows.end - 1;
    if (end < rows.start) return;
    this.term.selectLines(rows.start, end);
    if (rows.start < this.term.buffer.active.viewportY) this.term.scrollToLine(rows.start);
  }
}
