import { useSyncExternalStore } from "react";
import { type NetOp, netOp, type Progress } from "./api";

/** The network command the top bar shows with its progress and Cancel, whichever panel started it. */
export interface NetActivity {
  label: string;
  op: NetOp;
  progress: Progress | null;
}

let current: NetActivity | null = null;
const listeners = new Set<() => void>();
const set = (next: NetActivity | null) => {
  current = next;
  listeners.forEach((l) => l());
};

/** Runs a network command as the one the top bar shows. */
export async function withNetActivity<T>(label: string, fn: (op: NetOp) => Promise<T>): Promise<T> {
  const op = netOp((progress) => current?.op === op && set({ ...current, progress }));
  set({ label, op, progress: null });
  try {
    return await fn(op);
  } finally {
    if (current?.op === op) set(null);
  }
}

export function useNetActivity() {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => current,
  );
}
