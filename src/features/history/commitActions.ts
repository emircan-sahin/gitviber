import { api, type Commit, errorMessage, type GraphRefs, type HistoryEdit, type RepoStatus, type Worktree } from "@/lib/api";
import { ask } from "@/lib/app/ask";
import { toast } from "@/lib/app/toast";
import type { GitRun, NetRun } from "@/hooks/useGitAction";
import type { Points } from "@/lib/repo/compareMark";

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
  /** Rewrites the branch's history (see HistoryEdit), after warning when it's pushed; `commits`: the ones it's about. `message` asks for a message first. */
  rewrite: (edit: HistoryEdit, commits: Commit[]) => Promise<boolean>;
  /** Asks for a new message for it. */
  reword: (commit: Commit) => void;
  /** Makes `commits` one with `onto`, where it is; `message` asks for the message, else the oldest one's stays. */
  squash: (commits: Commit[], onto: string, message: boolean) => void;
  refresh: () => unknown;
  /** `webUrl` has every listed commit, not only those reached from origin's branches. */
  everyOnWeb: boolean;
  /** Other worktrees with a branch checked out, to cherry-pick onto. */
  pickTargets: Worktree[];
  pickInto: (w: Worktree, commit: Commit) => Promise<void>;
  /** Picks `commits` (newest first, as listed) onto the current branch, oldest first. */
  pickMany: (commits: Commit[]) => Promise<boolean>;
  remotes: Set<string>;
  refMenu?: RefMenu;
  showRefs?: GraphRefs;
  /** Opens every file of the commit in one scroll. */
  openAll: (commit: Commit) => void;
  /** Opens the agent CLI's guided review of the commit, asking for one if there's none. */
  explain: (commit: Commit) => void;
  /** Lists that can show a comparison in their place. */
  comparePoints?: (points: Points) => void;
  filterAuthor?: AuthorFilter;
}

/** Narrowing History to a commit's author, through the search box: only lists it searches have one. */
export interface AuthorFilter {
  set: (c: Commit) => void;
  /** The search is that author already. */
  has: (c: Commit) => boolean;
}

/** GitHub only has commits that reached one of origin's branches (or all, for a fork's original). */
export const commitUrl = (c: Commit, { webUrl, everyOnWeb }: Pick<Actions, "webUrl" | "everyOnWeb">) =>
  webUrl && (c.onOrigin || everyOnWeb) ? `${webUrl}/commit/${c.sha}` : undefined;

export const PUSHED_WARNING = "Some of these commits are already pushed, so you'd have to force-push, which rewrites history for everyone else on this branch.";
export const MERGE_WARNING = "This is a merge: the merged-in commits leave the branch too, and all of their changes end up staged together.";

/** Asked at click time: only ancestry, not log order, tells which pushed commits a move drops. */
export async function dropsPushed(sha: string | null) {
  try {
    return await api.dropsPushed(sha);
  } catch (e) {
    toast("error", "Could not compare with the upstream", errorMessage(e));
    return null;
  }
}

/** Undoes HEAD's commit `sha`, its changes staged again; refused if HEAD has moved since. */
export const undoCommit = (sha: string, run: GitRun) => run("Undo", () => api.undoCommit(sha), "Commit undone; its changes are staged");

/** Checks out `c` with no branch, after saying what that means. */
export async function checkoutDetached(c: Commit, run: GitRun) {
  const ok = await ask(`Check out ${c.shortSha} without a branch (detached HEAD)? New commits made there belong to no branch until you create one.`, {
    title: "Checkout commit",
    kind: "warning",
    okLabel: "Checkout",
  });
  if (ok) await run("Checkout", () => api.checkoutCommit(c.sha), `Checked out ${c.shortSha}`);
}
