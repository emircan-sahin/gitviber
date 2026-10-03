import { api, errorMessage } from "../api";
import { gitFailed } from "../app/gitFailed";
import { toast, type ToastAction } from "../app/toast";

type Refresh = () => unknown;

/** Runs a git action; also returns the undo entry it recorded, if it moved anything. */
export async function tracked<T>(fn: () => Promise<T>): Promise<[T, number | null]> {
  const before = await api.journalLast().catch(() => null);
  const value = await fn();
  const after = await api.journalLast().catch(() => null);
  return [value, after !== null && after !== before ? after : null];
}

/** The Undo button for a success toast, when the action recorded entry `id`. */
export const undoAction = (id: number | null, refresh: Refresh): ToastAction | undefined =>
  id === null ? undefined : { label: "Undo", run: () => void travel(false, [id], refresh) };

/**
 * Runs an action that rewrites working-tree files through the journal (a restore, a revert, a
 * patch), and says how it went: the files a merge left conflicts in, else where the old versions
 * went, with Undo. `write` returns the conflicted files, if it can have any.
 */
export async function rewriteFiles(done: string, failed: string, write: () => Promise<string[] | void>, refresh: Refresh) {
  try {
    const [conflicts, entry] = await tracked(write);
    const marked = conflicts?.length ? `Conflicts are marked in ${conflicts.join(", ")}.` : undefined;
    toast(marked ? "info" : "success", done, marked ?? "The versions it replaced are in the Trash.", undoAction(entry, refresh));
  } catch (e) {
    toast("error", failed, errorMessage(e));
  }
  await refresh();
}

/**
 * Undoes (or with `forward`, redoes) the entries `ids` in order, each the next one at its
 * turn; the backend refuses any that isn't. Stops at the first that can't be done.
 */
export async function travel(forward: boolean, ids: number[], refresh: Refresh) {
  const verb = forward ? "Redone" : "Undone";
  const done: number[] = [];
  let label = "";
  // Where HEAD ends up, when a step switched branches: ⌘Z must not switch without saying so.
  let switched: string | null = null;
  // A redone patch or restore can meet edits made since.
  const conflicts = new Set<string>();
  try {
    for (const id of ids) {
      const step = await (forward ? api.redo(id) : api.undo(id));
      label = step.label;
      switched = step.switchTo ?? switched;
      step.conflicts.forEach((c) => conflicts.add(c));
      done.push(id);
    }
  } catch (e) {
    const what = forward ? "Could not redo" : "Could not undo";
    gitFailed(done.length ? `${what} past ${label}` : what, e);
  } finally {
    await refresh();
  }
  if (done.length === ids.length) {
    // The way back is the same entries in reverse.
    const back = { label: forward ? "Undo" : "Redo", run: () => void travel(!forward, [...done].reverse(), refresh) };
    const marked = conflicts.size ? `Conflicts are marked in ${[...conflicts].join(", ")}.` : null;
    const detail = [switched && `Switched ${forward ? "" : "back "}to ${switched}.`, marked].filter(Boolean).join(" ") || undefined;
    toast(marked ? "info" : "success", done.length === 1 ? `${verb}: ${label}` : `${verb} ${done.length} actions`, detail, back);
  }
}
