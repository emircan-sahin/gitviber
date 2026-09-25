import { useState } from "react";
import { api, CANCELLED, errorMessage, github, type NetOp, type PullMode } from "@/lib/api";
import { askForIdentity } from "@/lib/app/identity";
import { explainedError, failed, toast, type ToastAction } from "@/lib/app/toast";
import { explainGitError, type GitFix, SIGNING_HELP } from "@/lib/git/gitErrors";
import { withNetActivity } from "@/lib/repo/netActivity";
import { tracked, undoAction } from "@/lib/repo/undo";

/** Runs one action; resolves true when it went through. `done`: the success toast, none without it. */
export type GitRun = (label: string, fn: () => Promise<unknown>, done?: string, detail?: string) => Promise<boolean>;
/** `autostash`: true when retried from the toast of a pull that uncommitted changes were in the way of. */
export type NetRun = (label: string, fn: (op: NetOp, autostash: boolean) => Promise<unknown>, done?: string) => Promise<boolean>;

type Fixes = Partial<Record<GitFix, ToastAction[]>>;

const ALWAYS: Fixes = {
  identity: [{ label: "Set name and email", run: askForIdentity }],
  signing: [{ label: "Signing guide", run: () => void github.openUrl(SIGNING_HELP).catch(failed("Could not open the link")) }],
};

/**
 * The error toast for a failed git action: git's own words, or for a failure gitErrors knows,
 * what it means and the way out, with git's words under Details. `fixes`: the ways out only
 * the caller can take (a pull, a retry).
 */
export function gitFailed(title: string, e: unknown, fixes: Fixes = {}) {
  const message = errorMessage(e);
  const help = explainGitError(message);
  if (!help) return toast("error", title, message);
  explainedError(help.title, help.explanation, message, (help.fix && { ...ALWAYS, ...fixes }[help.fix]) || []);
}

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

  const attempt = async (label: string, fn: () => Promise<unknown>, done?: string, detail?: string, autostash?: () => unknown) => {
    setBusy(label);
    try {
      const [stopped, entry] = undoable ? await tracked(fn) : [await fn(), null];
      if (stopped === true) toast("info", `${label} stopped on conflicts`, conflicts);
      else if (done) toast("success", done, detail, undoAction(entry, refresh ?? (() => {})));
      await onDone?.();
      return true;
    } catch (e) {
      if (e === CANCELLED) toast("info", `${label} cancelled`);
      else
        gitFailed(`${label} failed`, e, {
          pull: [pullAction("merge"), pullAction("rebase")],
          autostash: autostash && [{ label: "Retry with autostash", run: autostash }],
        });
      return false;
    } finally {
      setBusy(null);
      await refresh?.();
    }
  };

  const run: GitRun = (label, fn, done, detail) => attempt(label, fn, done, detail);

  /** Fetch, pull and push: git's progress shows in the top bar, and Cancel stops it. */
  const runNet: NetRun = (label, fn, done) =>
    attempt(label, () => withNetActivity(label, (op) => fn(op, false)), done, undefined, () => runNet(label, (op) => fn(op, true), done));

  const pull = (mode: PullMode) => runNet("Pull", (op, autostash) => api.pull(mode, op, autostash), mode === "ff" ? "Pulled" : `Pulled (${mode})`);
  const pullAction = (mode: "merge" | "rebase"): ToastAction => ({ label: `Pull (${mode})`, run: () => pull(mode) });

  return { busy, run, runNet, pull };
}
