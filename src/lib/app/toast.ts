import { errorMessage } from "../api";
import { createStore } from "../store";

interface Toast {
  id: number;
  kind: "error" | "success" | "info";
  title: string;
  detail?: string;
  /** A button on the toast, e.g. Undo. */
  action?: ToastAction;
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

/** Errors stay until dismissed: they carry hook, GPG or push output that takes a while to read. */
export function toast(kind: Toast["kind"], title: string, detail?: string, action?: ToastAction) {
  const id = nextId++;
  for (const old of toasts.get().slice(0, -3)) dismissToast(old.id);
  toasts.set([...toasts.get(), { id, kind, title, detail, action }]);
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
    setTimeout(() => dismissToast(id), t.action ? 6000 : 3000),
  );
}

export const useToasts = toasts.use;
