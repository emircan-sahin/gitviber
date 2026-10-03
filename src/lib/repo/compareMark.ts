import { createStore } from "../store";

/** A commit to compare, by its full id, and what to call it. */
export interface Point {
  sha: string;
  label: string;
}

/** Two points of history, or one and the working tree (`head` null): what `base` → `head` changed, as `git diff` has it, no merge base. */
export interface Points {
  base: Point;
  head: Point | null;
}

/** What Select for Compare picked, in the repo at `root`: the old side of the next "Compare with …". */
function compareMark<T>() {
  const picked = createStore<(T & { root: string }) | null>(null);
  return {
    set: picked.set,
    use: (root: string | undefined) => {
      const m = picked.use();
      return m && m.root === root ? m : null;
    },
  };
}

/** A commit, in History. */
export const commitMark = compareMark<Point>();
/** A file, in the explorer. */
export const fileMark = compareMark<{ path: string }>();
