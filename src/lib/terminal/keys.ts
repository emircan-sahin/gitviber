import type { Terminal } from "@xterm/xterm";
import { appRunsFromTerminal, appTakesFromTerminal, type CommandId, commandIn } from "../commands/keybindings";
import { getSettings } from "../settings";
import { IS_LINUX, IS_MAC } from "../platform";
import { pasteInto } from "./pasteInput";
import { type Pane, TERMINAL_COMMANDS } from "./terminals";

// A pane's keys: the app's, the line editing and scrolling done here, and the shell's.
// Imported through terminals.ts only: the two import each other.

/** Whether the left ⌥ is held, for the left-only Meta setting. Its release may go to another window. */
let leftOptionDown = false;
window.addEventListener("blur", () => (leftOptionDown = false));

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

// ⌘ keys are the app's shortcuts (copy and paste arrive as clipboard events, not keys),
// except the line-editing ones; ⌃` toggles the panel instead of sending NUL, ⌃Tab or ⌃1 run
// their commands, and the panel's own keys stay with it whatever they're rebound to. Unbound,
// ⌘Home/End/PgUp/PgDn (Ctrl+Home/End elsewhere) scroll the history.
export function paneKeys(p: Pane) {
  const term = p.term;
  return (e: KeyboardEvent) => {
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
    // ⌘↑ / ⌘↓ between the marked prompts. Off macOS, Ctrl+↑/↓ stay a full-screen program's, or a
    // shell's with no marks (⌘ keys never reach one anyway). Plain typing isn't looked up.
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
  };
}
