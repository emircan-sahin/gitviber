import { invoke } from "@tauri-apps/api/core";

export type ChangeStatus = "M" | "A" | "D" | "R" | "C" | "T" | "U" | "?";

export interface FileChange {
  path: string;
  oldPath: string | null;
  status: ChangeStatus;
  additions: number | null;
  deletions: number | null;
  /** Content identity (index blob, or size+mtime in the worktree); changes on every edit. */
  oid: string | null;
  /** Conflicts only: UU both modified, AA both added, UD/DU deleted by them/us, AU/UA, DD. */
  conflict: string | null;
}

export interface Operation {
  kind: "merge" | "rebase" | "cherry-pick" | "revert";
  subject: string | null;
  step: number | null;
  total: number | null;
}

export interface RepoStatus {
  root: string;
  branch: string | null;
  head: string | null;
  upstream: string | null;
  ahead: number;
  behind: number;
  staged: FileChange[];
  unstaged: FileChange[];
  conflicted: FileChange[];
  operation: Operation | null;
}

export interface Commit {
  sha: string;
  shortSha: string;
  authorName: string;
  authorEmail: string;
  timestamp: number;
  parents: string[];
  refs: string[];
  subject: string;
  body: string;
  /** Ahead of the upstream; false when there is none or it is gone (unknown). */
  unpushed: boolean;
  /** Reachable from an origin remote-tracking branch, so it exists on the origin host. */
  onOrigin: boolean;
}

export interface FileText {
  text: string;
  binary: boolean;
  tooLarge: boolean;
  exists: boolean;
  /** Not valid UTF-8; shown lossily and must never be written back. */
  lossy: boolean;
}

/** k: 0 unchanged, 1 added, 2 removed. o/n: 1-based line numbers (0 = absent). e: emphasized UTF-16 ranges. */
export interface DiffRow {
  k: 0 | 1 | 2;
  o: number;
  n: number;
  e?: [number, number][];
}

export interface DiffPair {
  original: FileText;
  modified: FileText;
  rows: DiffRow[];
}

export interface Entry {
  name: string;
  path: string;
  isDir: boolean;
  ignored: boolean;
}

export interface Branch {
  name: string;
  remote: boolean;
  current: boolean;
  upstream: string | null;
  timestamp: number;
}

export type PullMode = "ff" | "merge" | "rebase";

export type DiffKind = "unstaged" | "staged" | "worktree" | "commit" | "range";

export const api = {
  openRepo: (path: string) => invoke<string>("open_repo", { path }),
  status: () => invoke<RepoStatus>("status"),
  log: (skip: number, limit: number) => invoke<Commit[]>("log", { skip, limit }),
  commitFiles: (sha: string) => invoke<FileChange[]>("commit_files", { sha }),
  diffPair: (kind: DiffKind, path: string, oldPath: string | null, sha: string | null, base: string | null = null) =>
    invoke<DiffPair>("diff_pair", { kind, path, oldPath, sha, base }),
  /** Raw bytes of one side of a diff (`original` = the before side), for media previews. */
  media: (kind: DiffKind, path: string, oldPath: string | null, sha: string | null, base: string | null, original: boolean) =>
    invoke<ArrayBuffer>("media", { kind, path, oldPath, sha, base, original }),
  listDir: (path: string) => invoke<Entry[]>("list_dir", { path }),
  readFile: (path: string) => invoke<FileText>("read_file", { path }),
  branches: () => invoke<Branch[]>("branches"),
  switchBranch: (name: string, create: boolean) => invoke<void>("switch_branch", { name, create }),
  stage: (paths: string[]) => invoke<void>("stage", { paths }),
  unstage: (paths: string[]) => invoke<void>("unstage", { paths }),
  discard: (paths: string[]) => invoke<void>("discard", { paths }),
  commit: (message: string, amend: boolean) => invoke<void>("commit", { message, amend }),
  push: () => invoke<void>("push"),
  // The boolean results mean "stopped on conflicts".
  pull: (mode: PullMode) => invoke<boolean>("pull", { mode }),
  merge: (name: string) => invoke<boolean>("merge", { name }),
  rebase: (onto: string) => invoke<boolean>("rebase", { onto }),
  opContinue: () => invoke<boolean>("op_continue"),
  opAbort: () => invoke<void>("op_abort"),
  rebaseSkip: () => invoke<boolean>("rebase_skip"),
  resolveSide: (path: string, side: "ours" | "theirs") => invoke<void>("resolve_side", { path, side }),
  writeFile: (path: string, content: string) => invoke<void>("write_file", { path, content }),
  createFile: (path: string) => invoke<void>("create_file", { path }),
  createDir: (path: string) => invoke<void>("create_dir", { path }),
  renamePath: (from: string, to: string) => invoke<void>("rename_path", { from, to }),
  /** Moves to the macOS Trash, so it can be put back. */
  trashPath: (path: string) => invoke<void>("trash_path", { path }),
  revealPath: (path: string) => invoke<void>("reveal_path", { path }),
  fetch: () => invoke<void>("fetch"),
  // History actions. `sha` on undo and `head` on reset are the HEAD the user saw (refused if it moved).
  undoCommit: (sha: string) => invoke<void>("undo_commit", { sha }),
  reset: (sha: string, mode: ResetMode, head: string) => invoke<void>("reset", { sha, mode, head }),
  /** Moving HEAD to `sha` would drop commits the upstream already has (needs a force-push). */
  dropsPushed: (sha: string) => invoke<boolean>("drops_pushed", { sha }),
  revert: (sha: string) => invoke<boolean>("revert", { sha }),
  checkoutCommit: (sha: string) => invoke<void>("checkout_commit", { sha }),
  createBranchAt: (name: string, sha: string) => invoke<void>("create_branch_at", { name, sha }),
  createTag: (name: string, sha: string) => invoke<void>("create_tag", { name, sha }),
  /** https://github.com/owner/name, or null when origin isn't on GitHub. */
  githubWebUrl: () => invoke<string | null>("github_web_url"),
};

export type ResetMode = "soft" | "mixed" | "hard";

// ---------------------------------------------------------------- GitHub

export interface GitHubAccount {
  login: string;
  /** "gh" = GitHub CLI, "git" = git's credential store */
  source: "gh" | "git";
  repo: { owner: string; name: string } | null;
  defaultBranch: string | null;
}

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
  comments: PullComment[];
}

export interface PullFiles {
  base: string;
  head: string;
  files: FileChange[];
}

export type MergeMethod = "merge" | "squash" | "rebase";

/** Backend's marker for "no GitHub credentials found" (show setup, not an error). */
export const GITHUB_NOT_CONNECTED = "github:not-connected";

export const github = {
  account: () => invoke<GitHubAccount>("gh_account"),
  list: (filter: "open" | "closed" | "all") => invoke<Pull[]>("pr_list", { filter }),
  detail: (number: number) => invoke<PullDetail>("pr_detail", { number }),
  files: (p: Pick<Pull, "number" | "baseRef" | "baseSha" | "headSha">) =>
    invoke<PullFiles>("pr_files", { number: p.number, baseRef: p.baseRef, baseSha: p.baseSha, headSha: p.headSha }),
  create: (title: string, body: string, head: string, base: string, draft: boolean) => invoke<Pull>("pr_create", { title, body, head, base, draft }),
  merge: (number: number, method: MergeMethod) => invoke<void>("pr_merge", { number, method }),
  checkout: (number: number, headRef: string, sameRepo: boolean) => invoke<void>("pr_checkout", { number, headRef, sameRepo }),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
};

export function errorMessage(e: unknown) {
  const raw = typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
  return raw === GITHUB_NOT_CONNECTED ? "GitHub sign-in missing or expired. Sign in again (see the PRs tab)." : raw;
}

export const isNotConnected = (e: unknown) => e === GITHUB_NOT_CONNECTED;
