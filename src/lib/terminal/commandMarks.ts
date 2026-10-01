import type { IMarker, Terminal } from "@xterm/xterm";

/**
 * An OSC 133 mark (shell integration): A a prompt starts, B the typed command does, C its output
 * does, D it ended, with its exit code when the shell gives one.
 */
export function parseMark(data: string): { kind: "A" | "B" | "C" | "D"; exit?: number } | null {
  const [kind, arg] = data.split(";");
  if (kind !== "A" && kind !== "B" && kind !== "C" && kind !== "D") return null;
  return kind === "D" && arg !== undefined && /^\d+$/.test(arg) ? { kind, exit: Number(arg) } : { kind };
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
  private prompted = () => {};
  /** The shell's first prompt is up: it reads what's typed now. */
  readonly ready = new Promise<void>((resolve) => (this.prompted = resolve));

  /** `onPrompt`: each prompt, where a `cd` has settled. */
  constructor(term: Terminal, onPrompt = () => {}) {
    this.term = term;
    this.onPrompt = onPrompt;
    term.parser.registerOscHandler(133, (data) => {
      const mark = parseMark(data);
      // To xterm, not the shell: as if the program had turned them off.
      if (mark && mouseLeftOn(mark.kind, term.buffer.active.type, term.modes.mouseTrackingMode)) term.write(MODES_OFF);
      if (mark) this.on(mark.kind, mark.exit);
      return true;
    });
  }

  private on(kind: string, exit?: number) {
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
    const mark = this.term.registerDecoration({ marker: c.prompt });
    mark?.onRender((el) => {
      if (el.firstChild) return;
      // The dot sits in the pane's left padding; the cell under the decoration stays clickable.
      el.style.pointerEvents = "none";
      const dot = document.createElement("div");
      dot.className = "gv-command-mark";
      if (failed) dot.dataset.failed = "";
      dot.title = exit === undefined ? "Command ended" : `Exit code ${exit}`;
      el.appendChild(dot);
    });
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
