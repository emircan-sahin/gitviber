import { useState } from "react";
import { CANCELLED, errorMessage, type NetOp } from "@/lib/api";
import { toast } from "@/lib/app/toast";
import { withNetActivity } from "@/lib/repo/netActivity";
import { tracked, undoAction } from "@/lib/repo/undo";

/** Runs one action; resolves true when it went through. `done`: the success toast, none without it. */
export type GitRun = (label: string, fn: () => Promise<unknown>, done?: string, detail?: string) => Promise<boolean>;
export type NetRun = (label: string, fn: (op: NetOp) => Promise<unknown>, done?: string) => Promise<boolean>;

interface Options {
  /** Re-reads the repo: after every action, gone through or not, and after an undo from its toast. */
  refresh?: () => unknown;
  /** Only after an action that went through, before `busy` clears. */
  onDone?: () => unknown;
  /** false: the action records no undo entry (stashes, GitHub writes), so its toast offers none. */
  tracked?: boolean;
  /** What to do after an action stops on conflicts. */
  conflicts?: string;
}

/**
 * The toasts, busy state and refresh around a git action. An action that can stop on conflicts
 * resolves to true; that's a state to handle, not an error. `busy` is the running action's label.
 */
export function useGitAction({ refresh, onDone, tracked: undoable = true, conflicts = "Resolve them in Changes, then continue." }: Options = {}) {
  const [busy, setBusy] = useState<string | null>(null);

  const run: GitRun = async (label, fn, done, detail) => {
    setBusy(label);
    try {
      const [stopped, entry] = undoable ? await tracked(fn) : [await fn(), null];
      if (stopped === true) toast("info", `${label} stopped on conflicts`, conflicts);
      else if (done) toast("success", done, detail, undoAction(entry, refresh ?? (() => {})));
      await onDone?.();
      return true;
    } catch (e) {
      if (e === CANCELLED) toast("info", `${label} cancelled`);
      else toast("error", `${label} failed`, errorMessage(e));
      return false;
    } finally {
      setBusy(null);
      await refresh?.();
    }
  };

  /** Fetch, pull and push: git's progress shows in the top bar, and Cancel stops it. */
  const runNet: NetRun = (label, fn, done) => run(label, () => withNetActivity(label, fn), done);

  return { busy, run, runNet };
}
