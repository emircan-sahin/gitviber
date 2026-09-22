import { useSyncExternalStore } from "react";

export interface Toast {
  id: number;
  kind: "error" | "success" | "info";
  title: string;
  detail?: string;
}

let toasts: Toast[] = [];
let nextId = 1;
const listeners = new Set<() => void>();
const emit = () => listeners.forEach((l) => l());

const timers = new Map<number, ReturnType<typeof setTimeout>>();

export function dismissToast(id: number) {
  clearTimeout(timers.get(id));
  timers.delete(id);
  toasts = toasts.filter((t) => t.id !== id);
  emit();
}

/** Errors stay until dismissed: they carry hook, GPG or push output that takes a while to read. */
export function toast(kind: Toast["kind"], title: string, detail?: string) {
  const id = nextId++;
  for (const old of toasts.slice(0, -3)) dismissToast(old.id);
  toasts = [...toasts, { id, kind, title, detail }];
  emit();
  holdToast(id, false);
}

/** Pauses a toast's timer while it's hovered or focused, and restarts it after. */
export function holdToast(id: number, held: boolean) {
  const t = toasts.find((x) => x.id === id);
  clearTimeout(timers.get(id));
  timers.delete(id);
  if (!t || held || t.kind === "error") return;
  timers.set(
    id,
    setTimeout(() => dismissToast(id), 3000),
  );
}

export function useToasts() {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => toasts,
  );
}
