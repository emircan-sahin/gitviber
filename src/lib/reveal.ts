import type { FindOptions } from "./findQuery";

/**
 * A search result's match for the code view to show and select once its file is on screen, as
 * source: a rendered Markdown or SVG preview switches to its code for it (Viewer). Anything else
 * opened meanwhile drops it (Workspace), so it never lands on a later visit to the file.
 */
export interface CodeReveal {
  path: string;
  line: number;
  query: string;
  options: FindOptions;
}

let pending: CodeReveal | null = null;
const listeners = new Set<(r: CodeReveal) => void>();

/** Asks for `r`: now if its file is on show, else once it is. */
export function revealInCode(r: CodeReveal) {
  pending = r;
  listeners.forEach((l) => l(r));
}

/** Whether a reveal waits for `path`. */
export const revealWaits = (path: string) => pending?.path === path;

/** The reveal waiting for `path`, taken: it applies once. */
export function takeReveal(path: string): CodeReveal | null {
  if (pending?.path !== path) return null;
  const r = pending;
  pending = null;
  return r;
}

export function dropReveal() {
  pending = null;
}

/** Calls `l` on each reveal asked for from now on; returns the unsubscribe. */
export function onReveal(l: (r: CodeReveal) => void) {
  listeners.add(l);
  return () => void listeners.delete(l);
}
