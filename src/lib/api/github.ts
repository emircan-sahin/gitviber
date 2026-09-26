import { invoke } from "@tauri-apps/api/core";
import { netOp, network } from "./network";
import type { FileChange, NetOp } from "./types";

/** What the signed-in account may do in one repository. */
export interface GitHubAccess {
  repo: { owner: string; name: string };
  defaultBranch: string | null;
  /** Write access: merge, and close or edit anyone's PRs and issues. */
  push: boolean;
  /** Triage: close and reopen anyone's PRs and issues, without write access. */
  triage: boolean;
  /** The only role GitHub lets delete issues. */
  admin: boolean;
  /** Issues are switched on (forks start with them off). */
  issues: boolean;
}

export interface GitHubAccount {
  login: string;
  /** "gh" = GitHub CLI, "git" = git's credential store */
  source: "gh" | "git";
  origin: GitHubAccess | null;
  /** The repository origin was forked from. */
  parent: GitHubAccess | null;
}

export const fullName = (r: GitHubAccess["repo"]) => `${r.owner}/${r.name}`;

/** owner/name from a PR's or issue's html url. */
export const repoOf = (url: string) => url.replace("https://github.com/", "").split("/").slice(0, 2).join("/");

/** The account's access in the repository a PR or issue (by url) belongs to. */
export function accessFor(account: GitHubAccount | null, url: string) {
  const repo = repoOf(url).toLowerCase();
  return [account?.origin, account?.parent].find((a) => a && fullName(a.repo).toLowerCase() === repo) ?? null;
}

/**
 * Where a request goes: null = origin, else the parent's owner/name. The backend accepts
 * nothing else. From a PR or issue, its own url says which.
 */
export type Target = string | null;

export interface Pull {
  number: number;
  title: string;
  state: "open" | "closed" | "merged";
  draft: boolean;
  author: string;
  headRef: string;
  headSha: string;
  headRepo: string | null;
  baseRef: string;
  baseSha: string;
  createdAt: string;
  updatedAt: string;
  url: string;
}

export interface PullCheck {
  name: string;
  state: string;
  url: string | null;
}

export interface PullComment {
  author: string;
  body: string;
  createdAt: string;
  review: string | null;
}

export interface PullDetail extends Pull {
  body: string;
  additions: number;
  deletions: number;
  changedFiles: number;
  commits: number;
  mergeable: boolean | null;
  mergeableState: string;
  checks: PullCheck[];
  /** Checks couldn't be read (a token without access to them, say); `checks` is then partial. */
  checksError: string | null;
  comments: PullComment[];
  /** Who closed it, if closed: an author may reopen only what they closed themselves. */
  closedBy: string | null;
  /** Who merged it, if merged: often not its author. */
  mergedBy: string | null;
}

export interface PullFiles {
  base: string;
  head: string;
  files: FileChange[];
}

export type MergeMethod = "merge" | "squash" | "rebase";
export type ReviewEvent = "APPROVE" | "REQUEST_CHANGES" | "COMMENT";

/** A page of the PR list (github.rs `PER_PAGE`). */
export const PR_PAGE = 100;

export type CiState = "success" | "failure" | "pending";

/** A comment on a line of a PR's diff; `line` null once the diff moved past it (outdated). */
export interface ReviewComment {
  id: number;
  replyTo: number | null;
  path: string;
  line: number | null;
  side: "LEFT" | "RIGHT";
  author: string;
  body: string;
  createdAt: string;
  url: string;
}

/** How many issues or pull requests are open and closed (a PR's closed counts the merged). */
export interface StateCounts {
  open: number;
  closed: number;
}

export const github = {
  account: () => invoke<GitHubAccount>("gh_account"),
  /** Origin's branches under branch protection (names without "origin/"). */
  protectedBranches: () => invoke<string[]>("gh_protected_branches"),
  /** The most recently updated `pages` × PR_PAGE. */
  list: (target: Target, filter: "open" | "closed" | "all", pages = 1) => invoke<Pull[]>("pr_list", { target, filter, pages }),
  /** All of them, not just the pages listed. */
  counts: (target: Target) => invoke<StateCounts>("pr_counts", { target }),
  detail: (target: Target, number: number) => invoke<PullDetail>("pr_detail", { target, number }),
  reviewComments: (target: Target, number: number) => invoke<ReviewComment[]>("pr_review_comments", { target, number }),
  /** On `line` of `path` at the PR's head `commit`, on `side`; with `replyTo`, an answer in that thread. */
  commentLine: (target: Target, number: number, commit: string, path: string, line: number, side: "LEFT" | "RIGHT", body: string, replyTo: number | null = null) =>
    invoke<ReviewComment>("pr_comment_line", { target, number, commit, path, line, side, replyTo, body }),
  /** The account's repositories (and those it works on), most recently updated first. */
  ownRepos: () => invoke<{ fullName: string; description: string; private: boolean; cloneUrl: string; updatedAt: string }[]>("gh_own_repos"),
  /** CI's rollup per commit, for those GitHub has checks on (up to 100 at once). */
  ciStates: (target: Target, shas: string[]) => invoke<Record<string, CiState>>("ci_states", { target, shas }),
  /** Signed image links for a private repo's attachments, by attachment id. */
  attachments: (target: Target, number: number) => invoke<Record<string, string>>("pr_attachments", { target, number }),
  /** Fetches the PR's commits first when they're missing. */
  files: (target: Target, p: Pick<Pull, "number" | "baseRef" | "baseSha" | "headSha">, op?: NetOp) =>
    network<PullFiles>("pr_files", { target, number: p.number, baseRef: p.baseRef, baseSha: p.baseSha, headSha: p.headSha }, op),
  /** Into the parent, `head` is a branch of origin's, and `maintainerEdits` lets its maintainers push to it. */
  create: (target: Target, title: string, body: string, head: string, base: string, draft: boolean, maintainerEdits = true) =>
    invoke<Pull>("pr_create", { target, title, body, head, base, draft, maintainerEdits }),
  merge: (target: Target, number: number, method: MergeMethod) => invoke<void>("pr_merge", { target, number, method }),
  setOpen: (target: Target, number: number, open: boolean) => invoke<Pull>("pr_set_open", { target, number, open }),
  review: (target: Target, number: number, event: ReviewEvent, body: string) => invoke<void>("pr_review", { target, number, event, body }),
  /** `sameRepo`: the PR's branch lives on origin, so it's checked out under its own name. */
  checkout: (target: Target, number: number, headRef: string, sameRepo: boolean, op?: NetOp) => network<void>("pr_checkout", { target, number, headRef, sameRepo }, op),
  /** The same branch, checked out in a new worktree in `dir` (default: beside the main one); returns its path. */
  checkoutWorktree: (target: Target, number: number, headRef: string, sameRepo: boolean, dir: string | null, op?: NetOp) =>
    network<string>("pr_checkout_worktree", { target, number, headRef, sameRepo, dir }, op),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  /** The remote for a fork's original, owner/name (fetched first if `fetch`); null when there is none. Works offline. */
  // Without `fetch` it's a local lookup: marked background, so it doesn't stop a background fetch.
  originalRemote: (original: string, fetch: boolean, op = netOp(undefined, !fetch)) => network<string | null>("gh_original_remote", { original, fetch }, op),
  /** This repo's remotes and the GitHub repositories (owner/name) behind them. Local only. */
  remotes: () => invoke<{ name: string; repo: string | null }[]>("gh_remotes"),
  /** GitHub's "Sync fork" for origin's `branch`, then a fetch of origin: "fast-forward", "merge" or "none". */
  syncFork: (branch: string, op?: NetOp) => network<"fast-forward" | "merge" | "none">("gh_sync_fork", { branch }, op),
  /** Adds the fork's original as a remote ("upstream") and fetches it; returns its name. */
  addOriginalRemote: (op?: NetOp) => network<string>("gh_add_original_remote", {}, op),
};
