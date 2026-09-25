import { accessFor, fullName, type GitHubAccount, type Pull, repoOf } from "@/lib/api";
import type { PullSource } from "@/features/worktrees/WorktreeDialogs";

/**
 * Where Checkout puts a PR: its own branch when that lives on origin (a pushed fix then updates
 * the PR), otherwise pr/<n>, or pr/<owner>/<n> for a fork's original, which numbers its own.
 * Unknown until the account loads: a guess could fetch another repo's same-named branch.
 */
export function pullSource(p: Pull, account: GitHubAccount | null): PullSource | null {
  if (!account) return null;
  const origin = account.origin ? fullName(account.origin.repo) : null;
  const sameRepo = !!origin && p.headRepo?.toLowerCase() === origin.toLowerCase();
  const access = accessFor(account, p.url);
  const inOrigin = !!access && access === account.origin;
  const target = repoOf(p.url);
  const branch = sameRepo ? p.headRef : inOrigin ? `pr/${p.number}` : `pr/${target.split("/")[0]}/${p.number}`;
  return { target, number: p.number, headRef: p.headRef, sameRepo, branch };
}
