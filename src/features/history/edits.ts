import type { Commit, HistoryEdit } from "../../lib/api/types.ts";

/** The commits an edit names; it makes everything from the oldest of them on again. `about`: the commits it's about. */
export function editedShas(edit: HistoryEdit, about: Commit[]): string[] {
  switch (edit.kind) {
    case "reword":
      return [edit.sha];
    case "move":
      // Down swaps it with its parent.
      return [edit.sha, ...(edit.up ? [] : (about[0]?.parents.slice(0, 1) ?? []))];
    case "squash":
      return [...edit.shas, edit.onto];
    case "reorder":
      return edit.before ? [...edit.shas, edit.before] : edit.shas;
    case "drop":
      return edit.shas;
  }
}

/**
 * What the rewrite keeps as it is, for asking whether it drops pushed commits: the parent of the
 * oldest of `shas` in `commits` (newest first), null from the root. One not listed yet stands in
 * for its parent.
 */
export function keptUpTo(shas: string[], commits: Commit[]): string | null {
  const at = shas.map((sha) => commits.findIndex((c) => c.sha === sha));
  const unlisted = shas.find((_, i) => at[i] < 0);
  if (unlisted) return unlisted;
  return commits[Math.max(...at)]?.parents[0] ?? null;
}

/** The message being asked for: a reword, or a squash of `shas` into `onto` (`commits`: the listed ones, oldest first, their messages to start with). */
export type Messaging =
  | { kind: "reword"; commit: Commit }
  | { kind: "squash"; commits: Commit[]; shas: string[]; onto: string; apart: boolean };

/** Where dragged commits would land: on a commit (squash) or above or below it (move). */
export interface DropAt {
  sha: string;
  where: "above" | "onto" | "below";
}

/**
 * Where commits dropped in a gap go, for a reorder edit: just under `before`, the nearest commit
 * above the gap that isn't moving, or on top (null). Undefined when they're there already.
 * `line`: the listed commits, newest first.
 */
export function reorderBefore(line: string[], moved: Set<string>, at: DropAt): string | null | undefined {
  const i = line.indexOf(at.sha);
  const before = at.where === "below" ? at.sha : (line.slice(0, i).reverse().find((sha) => !moved.has(sha)) ?? null);
  const rest = line.filter((sha) => !moved.has(sha));
  const k = before === null ? 0 : rest.indexOf(before) + 1;
  const next = [...rest.slice(0, k), ...line.filter((sha) => moved.has(sha)), ...rest.slice(k)];
  // Only spares the toast: the backend checks the move against the branch's first-parent line.
  return next.every((sha, j) => sha === line[j]) ? undefined : before;
}
