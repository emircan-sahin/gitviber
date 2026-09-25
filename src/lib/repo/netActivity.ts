import { CANCELLED, errorMessage, type NetOp, netOp, type Progress } from "../api";
import { notifyIfAway } from "../app/notify";
import { createStore } from "../store";

/** The network command the top bar shows with its progress and Cancel, whichever panel started it. */
interface NetActivity {
  label: string;
  op: NetOp;
  progress: Progress | null;
}

// Every command running, oldest first. The top bar shows the newest; when it ends, the one
// still running before it comes back with its own progress and Cancel.
let active: NetActivity[] = [];
const current = createStore<NetActivity | null>(null);
const update = (next: NetActivity[]) => {
  active = next;
  current.set(active.at(-1) ?? null);
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

export const useNetActivity = current.use;
