import { useEffect, useRef, useState } from "react";
import { CANCELLED, type NetOp } from "@/lib/api";
import { gitFailed, type GitFixes } from "@/lib/app/gitFailed";
import { toast } from "@/lib/app/toast";
import { withNetActivity } from "@/lib/repo/netActivity";
import { tracked, undoAction } from "@/lib/repo/undo";

/** What only this call knows: the ways out of its failures, and what to say if it stops on conflicts. */
export interface RunExtras {
  fixes?: GitFixes;
  conflicts?: string;
}

/** Runs one action; resolves true when it went through. `done`: the success toast, none without it. */
export type GitRun = (label: string, fn: () => Promise<unknown>, done?: string, detail?: string) => Promise<boolean>;
export type NetRun = (label: string, fn: (op: NetOp) => Promise<unknown>, done?: string, extras?: RunExtras) => Promise<boolean>;

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
  const running = useRef<string | null>(null);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => void (mounted.current = false);
  }, []);

  // An error toast stays until dismissed, so its buttons can be pressed while another action
  // runs, or after another repo opened (this view is gone with its repo).
  const guarded = (fixes: GitFixes = {}): GitFixes =>
    Object.fromEntries(
      Object.entries(fixes).map(([fix, actions]) => [
        fix,
        actions?.map((a) => ({
          ...a,
          run: () => {
            if (!mounted.current) toast("info", `${a.label} not run`, "It was for the repository open before this one.");
            else if (running.current) toast("info", `${a.label} not run`, `Wait for ${running.current} to finish.`);
            else a.run();
          },
        })),
      ]),
    );

  const attempt = async (label: string, fn: () => Promise<unknown>, done?: string, detail?: string, extras: RunExtras = {}) => {
    setBusy((running.current = label));
    try {
      const [stopped, entry] = undoable ? await tracked(fn) : [await fn(), null];
      if (stopped === true) toast("info", `${label} stopped on conflicts`, extras.conflicts ?? conflicts);
      else if (done) toast("success", done, detail, undoAction(entry, refresh ?? (() => {})));
      await onDone?.();
      return true;
    } catch (e) {
      if (e === CANCELLED) toast("info", `${label} cancelled`);
      else gitFailed(`${label} failed`, e, guarded(extras.fixes));
      return false;
    } finally {
      setBusy((running.current = null));
      await refresh?.();
    }
  };

  const run: GitRun = (label, fn, done, detail) => attempt(label, fn, done, detail);

  /** Fetch, pull and push: git's progress shows in the top bar, and Cancel stops it. */
  const runNet: NetRun = (label, fn, done, extras) => attempt(label, () => withNetActivity(label, fn), done, undefined, extras);

  return { busy, run, runNet };
}
