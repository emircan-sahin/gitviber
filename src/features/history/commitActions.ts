import { api, type Commit, errorMessage, type GraphRefs, type HistoryEdit, type RepoStatus, type Worktree } from "@/lib/api";
import { toast } from "@/lib/app/toast";
import type { GitRun, NetRun } from "@/hooks/useGitAction";

/** Refs are full names (refs/heads/…). */
export interface RefMenu {
  hide: (refs: string[]) => void;
  only: (ref: string) => void;
}

/** What a commit's context menu needs from the panel. */
export interface Actions {
  status: RepoStatus | null;
  /** HEAD as the history shows it; the backend refuses to move HEAD if it has changed since. */
  headSha: string;
  webUrl: string | null;
  /** An action is running or a merge/rebase/revert waits: nothing else may move HEAD. */
  locked: boolean;
  run: GitRun;
  runNet: NetRun;
  name: (kind: "branch" | "tag", commit: Commit) => void;
  /** Rewrites the branch's history (see HistoryEdit), after warning when it's pushed; `message` asks for a message first. */
  rewrite: (edit: HistoryEdit, commit: Commit) => Promise<void>;
  message: (kind: "reword" | "squash", commit: Commit) => void;
  refresh: () => unknown;
  /** `webUrl` has every listed commit, not only those reached from origin's branches. */
  everyOnWeb: boolean;
  /** Other worktrees with a branch checked out, to cherry-pick onto. */
  pickTargets: Worktree[];
  pickInto: (w: Worktree, commit: Commit) => Promise<void>;
  remotes: Set<string>;
  refMenu?: RefMenu;
  showRefs?: GraphRefs;
}

/** GitHub only has commits that reached one of origin's branches (or all, for a fork's original). */
export const commitUrl = (c: Commit, { webUrl, everyOnWeb }: Pick<Actions, "webUrl" | "everyOnWeb">) =>
  webUrl && (c.onOrigin || everyOnWeb) ? `${webUrl}/commit/${c.sha}` : undefined;

export const PUSHED_WARNING = "Some of these commits are already pushed, so you'd have to force-push, which rewrites history for everyone else on this branch.";
export const MERGE_WARNING = "This is a merge: the merged-in commits leave the branch too, and all of their changes end up staged together.";

/** Asked at click time: only ancestry, not log order, tells which pushed commits a move drops. */
export async function dropsPushed(sha: string) {
  try {
    return await api.dropsPushed(sha);
  } catch (e) {
    toast("error", "Could not compare with the upstream", errorMessage(e));
    return null;
  }
}
