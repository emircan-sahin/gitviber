import { errorMessage } from "../api";
import { logError } from "./errorLog";
import { createStore } from "../store";

interface Toast {
  id: number;
  kind: "error" | "success" | "info";
  title: string;
  /** Plain words above `detail`, which then folds away under Details: what a git error means. */
  note?: string;
  detail?: string;
  /** Buttons on the toast, e.g. Undo. */
  actions: ToastAction[];
}

export interface ToastAction {
  label: string;
  run: () => void;
}

const toasts = createStore<Toast[]>([]);
let nextId = 1;

const timers = new Map<number, ReturnType<typeof setTimeout>>();

export function dismissToast(id: number) {
  clearTimeout(timers.get(id));
  timers.delete(id);
  toasts.set(toasts.get().filter((t) => t.id !== id));
}

/**
 * Errors stay until dismissed: they carry hook, GPG or push output that takes a while to read. They
 * also go to the error log (errors.rs blanks out credentials), for a bug report; a cancelled or
 * conflicted action is an info toast, so it doesn't.
 */
export function toast(kind: Toast["kind"], title: string, detail?: string, action?: ToastAction) {
  show({ kind, title, detail, actions: action ? [action] : [] });
}

/** An error the app can explain: `note` says what it means and `detail` keeps git's own words. */
export function explainedError(title: string, note: string, detail: string, actions: ToastAction[]) {
  show({ kind: "error", title, note, detail, actions });
}

// The log gets git's own words, not the note: that's the app's canned explanation of them.
function show(t: Omit<Toast, "id">) {
  if (t.kind === "error") logError("toast", t.detail ? `${t.title}: ${t.detail}` : t.title);
  const id = nextId++;
  for (const old of toasts.get().slice(0, -3)) dismissToast(old.id);
  toasts.set([...toasts.get(), { id, ...t }]);
  holdToast(id, false);
}

/** An error toast titled `title` for a rejection: `.catch(failed("Could not push"))`. */
export const failed = (title: string) => (e: unknown) => toast("error", title, errorMessage(e));

/** Pauses a toast's timer while it's hovered or focused, and restarts it after. */
export function holdToast(id: number, held: boolean) {
  const t = toasts.get().find((x) => x.id === id);
  clearTimeout(timers.get(id));
  timers.delete(id);
  if (!t || held || t.kind === "error") return;
  timers.set(
    id,
    // An action needs time to reach for.
    setTimeout(() => dismissToast(id), t.actions.length ? 6000 : 3000),
  );
}

export const useToasts = toasts.use;
