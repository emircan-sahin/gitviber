import { focusActive } from "./terminals";

/**
 * Keyboard focus across the workspace's panels, as in VS Code: each is an element marked
 * `data-panel`. Focusing one lands on its list's current row, the code view's editor, or the
 * active terminal, else on the panel itself (it takes focus, so F6 and Esc still know where it is).
 */
export type Panel = "git" | "code" | "explorer" | "terminal";

/** F6 order. */
export const PANELS: Panel[] = ["git", "code", "explorer", "terminal"];

const panelOf = (el: Element | null) => (el?.closest<HTMLElement>("[data-panel]")?.dataset.panel as Panel | undefined) ?? null;
const panelEl = (p: Panel) => document.querySelector<HTMLElement>(`[data-panel="${p}"]`);

// Where focus was last, and the list Esc in the code view goes back to.
let last: Panel | null = null;
let lastList: "git" | "explorer" = "git";
document.addEventListener("focusin", (e) => {
  const p = panelOf(e.target as Element);
  if (!p) return;
  last = p;
  if (p === "git" || p === "explorer") lastList = p;
});

/** Focus has fallen to the page (the element holding it went away, as the code view does on a tab switch). */
const lost = () => !document.activeElement || document.activeElement === document.body;

/** The panel holding focus; after it was lost, the one it was in. */
export function focusedPanel(): Panel | null {
  return lost() ? last : panelOf(document.activeElement);
}

// The code view's editor, while one is shown (MonacoView).
let focusEditor: (() => void) | null = null;
export function setCodeEditor(fn: () => void) {
  focusEditor = fn;
  return () => {
    if (focusEditor === fn) focusEditor = null;
  };
}

/** A new file in the code view takes focus when the view had it: its panel does, or it was lost from there. */
export const codeWantsFocus = () => (lost() ? last === "code" : document.activeElement === panelEl("code"));

/** Moves focus into a panel; false when it isn't on screen. */
export function focusPanel(p: Panel): boolean {
  const el = panelEl(p);
  if (!el || !el.offsetWidth || !el.offsetHeight) return false;
  if (p === "terminal") focusActive();
  else if (p === "code") (focusEditor ?? (() => el.focus()))();
  // A list's one tab stop (useListNav, the Changes list), the explorer's tree.
  else (el.querySelector<HTMLElement>('[data-row][tabindex="0"]') ?? el.querySelector<HTMLElement>('[tabindex="0"]') ?? el).focus();
  return true;
}

/** Esc in the code view: back to the list it was opened from. */
export const focusList = () => focusPanel(lastList) || focusPanel(lastList === "git" ? "explorer" : "git");
