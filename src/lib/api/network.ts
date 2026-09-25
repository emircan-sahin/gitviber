import { Channel, invoke } from "@tauri-apps/api/core";
import type { NetOp, Progress } from "./types";

// Unique across page reloads too: a command from before a reload may still be running.
const netSession = Date.now().toString(36);
let netCount = 0;
export const netOp = (onProgress?: (p: Progress) => void, background = false): NetOp => ({ id: `${netSession}-${++netCount}`, onProgress, background });

const running = new Set<NetOp>();
/** Some network command is running. */
export const networkBusy = () => running.size > 0;

export function network<T>(cmd: string, args: Record<string, unknown>, op = netOp()): Promise<T> {
  // Two fetches at once can fail on ref locks; the user's command wins.
  if (!op.background) for (const o of running) if (o.background) void cancelNetwork(o);
  running.add(op);
  // A channel ends with its command, so every call gets a new one; a retry may reuse the op.
  return invoke<T>(cmd, { ...args, op: op.id, progress: new Channel<Progress>(op.onProgress) }).finally(() => running.delete(op));
}

/** Stops a network command; its call then rejects with CANCELLED. */
export const cancelNetwork = (op: NetOp) => invoke<void>("cancel_network", { op: op.id });

/** A background command's prompts are declined unseen: a dialog nobody asked for would be a surprise. */
export const isBackgroundOp = (id: string | null) => [...running].some((o) => o.id === id && o.background);

/** Answers a git or ssh prompt; null is Cancel. The answer goes to git once and is kept nowhere. */
export const answerPrompt = (id: number, answer: string | null) => invoke<void>("askpass_answer", { id, answer });
