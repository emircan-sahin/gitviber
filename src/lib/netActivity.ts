import { useSyncExternalStore } from "react";
import { CANCELLED, errorMessage, type NetOp, netOp, type Progress } from "./api";
import { notifyIfAway } from "./notify";

/** The network command the top bar shows with its progress and Cancel, whichever panel started it. */
export interface NetActivity {
  label: string;
  op: NetOp;
  progress: Progress | null;
}

// Every command running, oldest first. The top bar shows the newest; when it ends, the one
// still running before it comes back with its own progress and Cancel.
let active: NetActivity[] = [];
let current: NetActivity | null = null;
const listeners = new Set<() => void>();
const update = (next: NetActivity[]) => {
  active = next;
  current = active.at(-1) ?? null;
  listeners.forEach((l) => l());
};

/** Runs a network command as one the top bar shows. */
export async function withNetActivity<T>(label: string, fn: (op: NetOp) => Promise<T>): Promise<T> {
  const op = netOp((progress) => update(active.map((a) => (a.op === op ? { ...a, progress } : a))));
  update([...active, { label, op, progress: null }]);
  try {
    const value = await fn(op);
    notifyIfAway(`${label} finished`);
    return value;
  } catch (e) {
    if (e !== CANCELLED) notifyIfAway(`${label} failed`, errorMessage(e));
    throw e;
  } finally {
    update(active.filter((a) => a.op !== op));
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
