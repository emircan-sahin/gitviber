import type { FindOptions } from "../ui/findQuery";

/**
 * A line for the code view to show once its file is on screen: a link's (path:12, #L12), at its
 * column, or a search result's, with its match selected. It's shown in the source: a rendered
 * Markdown or SVG preview switches to its code for it (Viewer). Anything else opened meanwhile
 * drops it (Workspace), so it never lands on a later visit to the file.
 */
export type CodeReveal = { path: string; line: number } & ({ column: number } | { query: string; options: FindOptions });

let pending: CodeReveal | null = null;
const listeners = new Set<(r: CodeReveal) => void>();

/** Asks for `r`: now if its file is on show, else once it is. */
export function revealInCode(r: CodeReveal) {
  pending = r;
  listeners.forEach((l) => l(r));
}

/** Whether a reveal waits for `path`. */
export const revealWaits = (path: string) => pending?.path === path;

/** The reveal waiting for `path`, taken: it applies once. `shown`: `path` is on show now, so one for another file is dropped. */
export function takeReveal(path: string, shown = false): CodeReveal | null {
  const r = pending;
  if (r && (r.path === path || shown)) pending = null;
  return r?.path === path ? r : null;
}

export function dropReveal() {
  pending = null;
}

/** Calls `l` on each reveal asked for from now on; returns the unsubscribe. */
export function onReveal(l: (r: CodeReveal) => void) {
  listeners.add(l);
  return () => void listeners.delete(l);
}
