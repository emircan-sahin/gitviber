import { useMemo, useState } from "react";
import type { Commit } from "@/lib/api";
import { type CommitRun, isWholePr, pickedCommits, runTo } from "@/lib/github/pullCommits";

// A PR's page remounts on every tab switch, opening one of its files included: its pick stays here.
const picks = new Map<string, CommitRun>();

/**
 * The picked commits (oldest first), null for none, and ways to change that. `narrowed` is what
 * Files changed shows of them: null when they're every commit from the merge base `base`, as that's
 * the whole PR, with its tab and line comments.
 */
export function useCommitPick(url: string, commits: Commit[], base: string | undefined) {
  const [pick, setPick] = useState(() => picks.get(url) ?? null);
  const set = (p: CommitRun | null) => {
    if (p) picks.set(url, p);
    else picks.delete(url);
    setPick(p);
  };
  // A force-push took them away: the whole PR again.
  const picked = useMemo(() => pickedCommits(commits, pick), [commits, pick]);
  const narrowed = picked && !isWholePr(commits, picked, base) ? picked : null;
  // The one before or after what's picked.
  const next = (dir: -1 | 1) => (picked ? commits[commits.indexOf(dir < 0 ? picked[0] : picked[picked.length - 1]) + dir] : undefined);
  const anchor = picked && pick ? pick.anchor : null;
  return {
    picked,
    narrowed,
    /** Where a ⇧-click's run starts. */
    anchor,
    pick: (sha: string, extend: boolean) => set(runTo(commits, extend ? anchor : null, sha)),
    run: (from: string, to: string) => set(runTo(commits, from, to)),
    next,
    step: (dir: -1 | 1) => {
      const c = next(dir);
      if (c) set({ anchor: c.sha, to: c.sha });
    },
    clear: () => set(null),
  };
}

export type CommitPick = ReturnType<typeof useCommitPick>;
