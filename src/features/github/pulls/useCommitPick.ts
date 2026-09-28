import { useMemo, useState } from "react";
import type { Commit } from "@/lib/api";
import { type CommitRun, pickedCommits, runTo } from "@/lib/github/pullCommits";

// A PR's page remounts on every tab switch, opening one of its files included: its pick stays here.
const picks = new Map<string, CommitRun>();

/** The commits Files changed is narrowed to (oldest first), null for all of them, and ways to change that. */
export function useCommitPick(url: string, commits: Commit[]) {
  const [pick, setPick] = useState(() => picks.get(url) ?? null);
  const set = (p: CommitRun | null) => {
    if (p) picks.set(url, p);
    else picks.delete(url);
    setPick(p);
  };
  // A force-push took them away: the whole PR again.
  const picked = useMemo(() => pickedCommits(commits, pick), [commits, pick]);
  // The one before or after what's picked.
  const next = (dir: -1 | 1) => (picked ? commits[commits.indexOf(dir < 0 ? picked[0] : picked[picked.length - 1]) + dir] : undefined);
  const anchor = picked && pick ? pick.anchor : null;
  return {
    picked,
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
