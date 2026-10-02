import type { IDisposable } from "@xterm/xterm";
import { copyText } from "../app/clipboard";
import { commandIn } from "../commands/keybindings";
import { toast } from "../app/toast";
import { followTerminalLink, shownLinks, type ShownLink } from "../links/linkHost";
import { HINT_ALPHABET, hintLabels } from "./hintLabels";
import type { Pane } from "./terminals";

// Link hints: a letter label on every link on a pane's screen, as kitty's hints kitten and Vimium
// do it. Typing a label opens its link, with Shift copies it; Esc, a scroll or a resize ends them.
// The labels are a few DOM nodes over the screen, placed by cell: nothing is drawn by the renderer.

interface Session {
  links: ShownLink[];
  labels: string[];
  typed: string;
  /** Shift was held on a letter of the label: copy rather than open. */
  copy: boolean;
  overlay: HTMLElement;
  listeners: IDisposable[];
}

const sessions = new Map<number, Session>();
/** Panes whose links are being looked up: the key again meanwhile does nothing. */
const asking = new Set<number>();

/** Labels the links on `p`'s screen, or takes them away when they show. */
export async function toggleHints(p: Pane) {
  if (sessions.has(p.id)) return endHints(p);
  const screen = p.term.element?.querySelector<HTMLElement>(".xterm-screen");
  if (!screen || asking.has(p.id)) return;
  const viewport = p.term.buffer.active.viewportY;
  asking.add(p.id);
  const links = await shownLinks(p.term, p.dir).finally(() => asking.delete(p.id));
  // Scrolled, or gone, while the repo was asked: what it said is of another screen.
  if (!screen.isConnected || p.term.buffer.active.viewportY !== viewport) return;
  if (!links.length) return toast("info", "No links on screen", "Hints go on the URLs, files, commits and #123 the terminal shows.");
  // Bottom first: the latest output is the likeliest pick.
  const labels = hintLabels(links.length).reverse();
  const overlay = document.createElement("div");
  overlay.className = "gv-hints";
  overlay.style.fontFamily = p.term.options.fontFamily ?? "";
  screen.appendChild(overlay);
  // The keys come to the pane (hintKey): from the palette, focus was elsewhere.
  p.term.focus();
  const end = () => endHints(p);
  const textarea = p.term.textarea;
  textarea?.addEventListener("blur", end);
  p.host.addEventListener("mousedown", end, true);
  const listeners = [
    p.term.onScroll(end),
    p.term.onResize(end),
    p.term.buffer.onBufferChange(end),
    {
      dispose: () => {
        textarea?.removeEventListener("blur", end);
        p.host.removeEventListener("mousedown", end, true);
      },
    },
  ];
  const session: Session = { links, labels, typed: "", copy: false, overlay, listeners };
  sessions.set(p.id, session);
  draw(p, session, screen);
}

export function endHints(p: Pane) {
  const s = sessions.get(p.id);
  if (!s) return;
  sessions.delete(p.id);
  for (const l of s.listeners) l.dispose();
  s.overlay.remove();
}

/** Each link's box, and its label at its first cell, the part typed so far dimmed; others hidden. */
function draw(p: Pane, s: Session, screen: HTMLElement) {
  const { cols, rows } = p.term;
  const [w, h] = [screen.clientWidth / cols, screen.clientHeight / rows];
  const top = p.term.buffer.active.viewportY;
  const nodes: HTMLElement[] = [];
  s.links.forEach((link, i) => {
    const label = s.labels[i];
    if (!label.startsWith(s.typed)) return;
    const { start, end } = link.range;
    // A box a row of it: a long link wraps.
    for (let y = start.y; y <= end.y && y - 1 - top < rows; y++) {
      const [from, to] = [y === start.y ? start.x - 1 : 0, y === end.y ? end.x : cols];
      const box = document.createElement("div");
      box.className = "gv-hint-box";
      box.style.cssText = `left:${from * w}px;top:${(y - 1 - top) * h}px;width:${(to - from) * w}px;height:${h}px`;
      nodes.push(box);
    }
    const tag = document.createElement("div");
    tag.className = "gv-hint";
    tag.style.cssText = `left:${(start.x - 1) * w}px;top:${(start.y - 1 - top) * h}px;height:${h}px;font-size:${Math.round(h * 0.6)}px`;
    const done = document.createElement("span");
    done.textContent = s.typed;
    tag.append(done, label.slice(s.typed.length));
    nodes.push(tag);
  });
  s.overlay.replaceChildren(...nodes);
}

/**
 * A key while `p` shows hints: a letter types toward a label, Backspace takes one back, Esc ends
 * them, and they're all kept from the program. A key with ⌘, ⌃ or ⌥ ends them and goes on as
 * usual (the hints' own key ends them itself). Undefined when there are no hints.
 */
export function hintKey(p: Pane, e: KeyboardEvent): false | undefined {
  const s = sessions.get(p.id);
  if (!s) return undefined;
  if (e.metaKey || e.ctrlKey || e.altKey) {
    if (e.type === "keydown" && !commandIn(["terminal.hints"], e)) endHints(p);
    return undefined;
  }
  e.preventDefault();
  if (e.type !== "keydown") return false;
  const key = e.key.toLowerCase();
  if (e.key === "Escape") endHints(p);
  else if (e.key === "Backspace") s.typed = s.typed.slice(0, -1);
  else if (key.length === 1 && HINT_ALPHABET.includes(key) && s.labels.some((l) => l.startsWith(s.typed + key))) {
    s.typed += key;
    s.copy ||= e.shiftKey;
    const at = s.labels.indexOf(s.typed);
    if (at >= 0) {
      endHints(p);
      const link = s.links[at];
      if (s.copy) void copyText(link.text, "Copied", link.text);
      else followTerminalLink(link.target);
      return false;
    }
  } else return false;
  const screen = s.overlay.parentElement;
  if (screen && sessions.get(p.id) === s) draw(p, s, screen);
  return false;
}
