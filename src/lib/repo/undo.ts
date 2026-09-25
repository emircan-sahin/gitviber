import { api, errorMessage } from "../api";
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
 * Undoes (or with `forward`, redoes) the entries `ids` in order, each the next one at its
 * turn; the backend refuses any that isn't. Stops at the first that can't be done.
 */
export async function travel(forward: boolean, ids: number[], refresh: Refresh) {
  const verb = forward ? "Redone" : "Undone";
  const done: number[] = [];
  let label = "";
  try {
    for (const id of ids) {
      label = (await (forward ? api.redo(id) : api.undo(id))).label;
      done.push(id);
    }
  } catch (e) {
    const what = forward ? "Could not redo" : "Could not undo";
    toast("error", done.length ? `${what} past ${label}` : what, errorMessage(e));
  } finally {
    await refresh();
  }
  if (done.length === ids.length) {
    // The way back is the same entries in reverse.
    const back = { label: forward ? "Undo" : "Redo", run: () => void travel(!forward, [...done].reverse(), refresh) };
    toast("success", done.length === 1 ? `${verb}: ${label}` : `${verb} ${done.length} actions`, undefined, back);
  }
}
