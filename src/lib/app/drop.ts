import { isInside, slashes } from "../path.ts";

// Files dragged in from Finder land in a terminal pane under the pointer (terminal/pasteInput);
// anywhere else they go to the workspace (App), when it takes them: the settings window doesn't.

let elsewhere: ((paths: string[]) => void) | null = null;
/** Takes the drops no terminal pane is under; returns what lets go of them. */
export function takeDrops(handler: (paths: string[]) => void) {
  elsewhere = handler;
  return () => {
    if (elsewhere === handler) elsewhere = null;
  };
}
/** Whether a drop away from the panes goes anywhere, for the outline while dragging. */
export const takesDrops = () => elsewhere !== null;
export const dropElsewhere = (paths: string[]) => elsewhere?.(paths);

/** Dropped files split by whether they're in the repo at `root`; the ones in it by their path from there. */
export function filesIn(paths: string[], root: string) {
  const dir = slashes(root).replace(/\/+$/, "");
  const inside: string[] = [];
  const outside: string[] = [];
  for (const p of paths) {
    const path = slashes(p);
    if (isInside(path, dir)) inside.push(path.slice(dir.length + 1));
    else outside.push(p);
  }
  return { inside, outside };
}
