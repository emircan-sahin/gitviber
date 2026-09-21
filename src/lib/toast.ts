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

export function dismissToast(id: number) {
  toasts = toasts.filter((t) => t.id !== id);
  emit();
}

export function toast(kind: Toast["kind"], title: string, detail?: string) {
  const id = nextId++;
  toasts = [...toasts.slice(-3), { id, kind, title, detail }];
  emit();
  setTimeout(() => dismissToast(id), kind === "error" ? 8000 : 3000);
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
