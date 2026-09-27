import { useEffect } from "react";
import { type CiState, fullName, type GitHubAccount, github, type Pull, repoOf, type Target } from "@/lib/api";
import { pullForBranch } from "@/lib/github/branchPulls";
import { useCi } from "@/lib/github/ci";
import { cached, cachedRows, revalidate, useGitHubCacheVersion } from "@/lib/github/githubCache";

/**
 * One request per repository for every worktree: its most recently updated PRs, open or closed,
 * kept under the Pull Requests panel's own key for its All filter, so each reuses the other's.
 * Nothing is asked without a GitHub origin; a missing sign-in is remembered by the cache.
 */
async function loadLists() {
  const account = await revalidate("account", github.account, 600_000);
  if (!account.origin) return;
  const upstream = account.parent ? fullName(account.parent.repo) : null;
  await Promise.all([
    revalidate("pulls:origin:all:1", () => github.list(null, "all")),
    upstream && revalidate(`pulls:${upstream}:all:1`, () => github.list(upstream, "all")),
  ]);
}

export interface BranchPull {
  pull: Pull;
  /** The repository it's in, for its checks: null = origin, else the fork's original. */
  target: Target;
  ci?: CiState;
}

/**
 * Each branch's pull request, from whatever PR lists are cached, and its head's checks when open.
 * `fetch` (the picker opening) loads the lists once, no polling; `read` false neither reads nor
 * re-renders. Checks are asked once for `ciFor`, and only for a PR a list had: a token is in hand
 * then, so nothing runs gh or the keychain unasked.
 */
export function useWorktreePulls(branches: (string | null)[], ciFor: (string | null)[], read: boolean, fetch: boolean) {
  useGitHubCacheVersion(read);
  useEffect(() => {
    if (fetch) loadLists().catch(() => {});
  }, [fetch]);

  const account = read ? cached<GitHubAccount>("account") : undefined;
  const origin = account?.origin ? fullName(account.origin.repo) : null;
  const upstream = account?.parent ? fullName(account.parent.repo) : null;
  const rows = origin ? cachedRows<Pull>("pulls:") : [];
  const found = new Map<string, BranchPull>();
  for (const branch of branches) {
    const pull = branch && origin ? pullForBranch(rows, branch, origin) : undefined;
    if (!pull) continue;
    const repo = repoOf(pull.url).toLowerCase();
    if (repo === origin!.toLowerCase()) found.set(branch!, { pull, target: null });
    else if (repo === upstream?.toLowerCase()) found.set(branch!, { pull, target: upstream });
  }
  const heads = (target: Target) =>
    ciFor.flatMap((b) => {
      const f = b ? found.get(b) : undefined;
      return f?.pull.state === "open" && f.target === target ? [f.pull.headSha] : [];
    });
  const own = useCi(null, heads(null), false);
  const up = useCi(upstream, upstream ? heads(upstream) : [], false);
  return (branch: string | null): BranchPull | undefined => {
    const f = branch ? found.get(branch) : undefined;
    return f && { ...f, ci: f.pull.state === "open" ? (f.target ? up : own)[f.pull.headSha] : undefined };
  };
}
