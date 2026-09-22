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
  /** Untracked entry that is another repository's root (never one of this repo's worktrees). */
  nested: Nested | null;
}

export interface Nested {
  /** Absolute path. */
  path: string;
}

export interface Worktree {
  path: string;
  head: string | null;
  /** Null when detached (or bare). */
  branch: string | null;
  detached: boolean;
  bare: boolean;
  locked: boolean;
  /** Why it's locked, if the locker said (Claude Code: "claude session … (pid N …)"). */
  lockReason: string | null;
  /** The lock names a process that's still running: someone is working in it now. */
  inUse: boolean;
  /** Its folder is gone; `git worktree prune` would drop it. */
  prunable: boolean;
  current: boolean;
  main: boolean;
}

export interface OpenedRepo {
  root: string;
  /** The main worktree, which the projects list is keyed by. */
  main: string;
}

export interface ProjectInfo {
  /** False once the folder was moved or deleted. */
  exists: boolean;
  /** owner/name of its GitHub origin */
  github: string | null;
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
  /** Where `git push` sends this branch; a fork can pull from upstream and push to origin. */
  push: { remote: string; branch: string | null; ahead: number } | null;
  remotes: string[];
  /** Where Publish sends a branch with no upstream; null when the user has to pick a remote. */
  publish: string | null;
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
  /** Logging another branch: not in HEAD yet, so merging would bring it in. */
  notInHead: boolean;
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

export interface WorktreeState {
  uncommitted: number;
  /** Commits the default branch lacks; on the default branch itself, commits no remote has. */
  commits: number;
  /** Committed on, then fully taken into the default branch. */
  merged: boolean;
}

export interface Branch {
  name: string;
  remote: boolean;
  current: boolean;
  upstream: string | null;
  timestamp: number;
  /** Checked out in another worktree (its path); git won't switch to it here. */
  worktree: string | null;
  /** Local, not current, and fully in HEAD: deleting it loses nothing. Never the default branch. */
  merged: boolean;
  /** What its remote's HEAD points at (origin/main). */
  remoteDefault: boolean;
}

export type PullMode = "ff" | "merge" | "rebase";

/** One of the app's own git actions, as the undo history lists it. */
export interface JournalEntry {
  id: number;
  label: string;
  /** Unix seconds. */
  time: number;
}

export interface Journal {
  /** Newest first. */
  undo: JournalEntry[];
  /** Next redo first. */
  redo: JournalEntry[];
  /** Why the next undo or redo can't run now: pushed since, changed outside the app, … */
  undoBlocked: string | null;
  redoBlocked: string | null;
}

export type DiffKind = "unstaged" | "staged" | "worktree" | "commit" | "range";

export interface About {
  version: string;
  /** Short commit the build came from; empty when built outside a git checkout. */
  commit: string;
  os: string;
  arch: string;
  /** `git --version` without the prefix; null when git can't run. */
  git: string | null;
}

export const api = {
  openRepo: (path: string) => invoke<OpenedRepo>("open_repo", { path }),
  status: () => invoke<RepoStatus>("status"),
  about: () => invoke<About>("about"),
  /** HEAD's history, or `rev`'s: a remote-tracking branch (refs/remotes/…), e.g. a fork's original. */
  log: (skip: number, limit: number, rev: string | null = null) => invoke<Commit[]>("log", { rev, skip, limit }),
  commitFiles: (sha: string) => invoke<FileChange[]>("commit_files", { sha }),
  diffPair: (kind: DiffKind, path: string, oldPath: string | null, sha: string | null, base: string | null = null) =>
    invoke<DiffPair>("diff_pair", { kind, path, oldPath, sha, base }),
  /** Raw bytes of one side of a diff (`original` = the before side), for media previews. */
  media: (kind: DiffKind, path: string, oldPath: string | null, sha: string | null, base: string | null, original: boolean) =>
    invoke<ArrayBuffer>("media", { kind, path, oldPath, sha, base, original }),
  listDir: (path: string) => invoke<Entry[]>("list_dir", { path }),
  readFile: (path: string) => invoke<FileText>("read_file", { path }),
  branches: () => invoke<Branch[]>("branches"),
  /** Switches to the local branch for a remote one ("upstream/dev" → dev), creating it to track exactly that. */
  /** What a PR from HEAD into `base` (refs/remotes/…) carries: its commit count, and the one commit's message. */
  pullDraft: (base: string) => invoke<{ commits: number; subject: string | null; body: string | null }>("pull_draft", { base }),
  /** `remote.pushDefault`: every branch pushes to `remote`, whatever it pulls from. */
  setPushDefault: (remote: string) => invoke<void>("set_push_default", { remote }),
  switchTracking: (remoteRef: string) => invoke<void>("switch_tracking", { remoteRef }),
  switchBranch: (name: string, create: boolean) => invoke<void>("switch_branch", { name, create }),
  /** `force` deletes unmerged commits too (git branch -D). */
  deleteBranches: (names: string[], force: boolean) => invoke<void>("delete_branches", { names, force }),
  /** "origin/feat" → git push origin --delete feat. */
  deleteRemoteBranch: (name: string) => invoke<void>("delete_remote_branch", { name }),
  worktrees: () => invoke<Worktree[]>("worktrees"),
  /** Uncommitted files in one of this repo's worktrees, and commits found nowhere else. */
  worktreeState: (path: string) => invoke<WorktreeState>("worktree_state", { path }),
  /** Checks a branch out in a new worktree beside the main one; returns its path. */
  addWorktree: (branch: string) => invoke<string>("add_worktree", { branch }),
  /** Deletes a linked worktree's folder (its branch stays); `force` drops uncommitted files and overrides a lock. */
  removeWorktree: (path: string, force: boolean) => invoke<void>("remove_worktree", { path, force }),
  /** Nested repositories are refused unless `allowNested`: git would stage only a gitlink. */
  stage: (paths: string[], allowNested = false) => invoke<void>("stage", { paths, allowNested }),
  unstage: (paths: string[]) => invoke<void>("unstage", { paths }),
  discard: (paths: string[]) => invoke<void>("discard", { paths }),
  commit: (message: string, amend: boolean) => invoke<void>("commit", { message, amend }),
  /** `force`: --force-with-lease, after a rebase or amend. `remote`: where to publish a branch with no upstream. */
  push: (force = false, remote?: string) => invoke<void>("push", { force, remote }),
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
  /** Any saved project's folder, not just the open repo's. */
  revealProject: (path: string) => invoke<void>("reveal_project", { path }),
  projectInfo: (paths: string[]) => invoke<ProjectInfo[]>("project_info", { paths }),
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
  journal: () => invoke<Journal>("journal"),
  /** The newest entry's id; a change across an action means it was recorded. */
  journalLast: () => invoke<number | null>("journal_last"),
  /** `id`: the entry meant; refused if it's no longer the next one. */
  undo: (id?: number) => invoke<JournalEntry>("undo", { id }),
  redo: (id?: number) => invoke<JournalEntry>("redo", { id }),
};

export type ResetMode = "soft" | "mixed" | "hard";

// ---------------------------------------------------------------- GitHub

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
  comments: PullComment[];
  /** Who closed it, if closed: an author may reopen only what they closed themselves. */
  closedBy: string | null;
}

export interface PullFiles {
  base: string;
  head: string;
  files: FileChange[];
}

export type MergeMethod = "merge" | "squash" | "rebase";
export type ReviewEvent = "APPROVE" | "REQUEST_CHANGES" | "COMMENT";

/** Backend's marker for "no GitHub credentials found" (show setup, not an error). */
export const GITHUB_NOT_CONNECTED = "github:not-connected";

export const github = {
  account: () => invoke<GitHubAccount>("gh_account"),
  /** Origin's branches under branch protection (names without "origin/"). */
  protectedBranches: () => invoke<string[]>("gh_protected_branches"),
  list: (target: Target, filter: "open" | "closed" | "all") => invoke<Pull[]>("pr_list", { target, filter }),
  detail: (target: Target, number: number) => invoke<PullDetail>("pr_detail", { target, number }),
  /** Signed image links for a private repo's attachments, by attachment id. */
  attachments: (target: Target, number: number) => invoke<Record<string, string>>("pr_attachments", { target, number }),
  files: (target: Target, p: Pick<Pull, "number" | "baseRef" | "baseSha" | "headSha">) =>
    invoke<PullFiles>("pr_files", { target, number: p.number, baseRef: p.baseRef, baseSha: p.baseSha, headSha: p.headSha }),
  /** Into the parent, `head` is a branch of origin's, and `maintainerEdits` lets its maintainers push to it. */
  create: (target: Target, title: string, body: string, head: string, base: string, draft: boolean, maintainerEdits = true) =>
    invoke<Pull>("pr_create", { target, title, body, head, base, draft, maintainerEdits }),
  merge: (target: Target, number: number, method: MergeMethod) => invoke<void>("pr_merge", { target, number, method }),
  setOpen: (target: Target, number: number, open: boolean) => invoke<Pull>("pr_set_open", { target, number, open }),
  review: (target: Target, number: number, event: ReviewEvent, body: string) => invoke<void>("pr_review", { target, number, event, body }),
  /** `sameRepo`: the PR's branch lives on origin, so it's checked out under its own name. */
  checkout: (target: Target, number: number, headRef: string, sameRepo: boolean) => invoke<void>("pr_checkout", { target, number, headRef, sameRepo }),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  /** The remote for a fork's original, owner/name (fetched first if `fetch`); null when there is none. Works offline. */
  originalRemote: (original: string, fetch: boolean) => invoke<string | null>("gh_original_remote", { original, fetch }),
  /** This repo's remotes and the GitHub repositories (owner/name) behind them. Local only. */
  remotes: () => invoke<{ name: string; repo: string | null }[]>("gh_remotes"),
  /** GitHub's "Sync fork" for origin's `branch`, then a fetch of origin: "fast-forward", "merge" or "none". */
  syncFork: (branch: string) => invoke<"fast-forward" | "merge" | "none">("gh_sync_fork", { branch }),
  /** Adds the fork's original as a remote ("upstream") and fetches it; returns its name. */
  addOriginalRemote: () => invoke<string>("gh_add_original_remote"),
};

export interface IssueLabel {
  name: string;
  /** Hex without the '#'. */
  color: string;
  description: string;
}

export interface Issue {
  number: number;
  title: string;
  state: "open" | "closed";
  stateReason: "completed" | "not_planned" | "reopened" | null;
  author: string;
  labels: IssueLabel[];
  assignees: string[];
  /** Comment count. */
  comments: number;
  createdAt: string;
  updatedAt: string;
  url: string;
}

export interface IssueDetail extends Issue {
  body: string;
  thread: PullComment[];
  /** Who closed it, if closed: an author may reopen only what they closed themselves. */
  closedBy: string | null;
}

export interface IssueCounts {
  open: number;
  closed: number;
}

export type CloseReason = "completed" | "not_planned";

export const issues = {
  /** `labels`: only issues carrying all of them. */
  list: (target: Target, filter: "open" | "closed" | "all", labels: string[] = []) => invoke<Issue[]>("issue_list", { target, filter, labels }),
  /** How many issues are open and closed, carrying all of `labels`. */
  counts: (target: Target, labels: string[] = []) => invoke<IssueCounts>("issue_counts", { target, labels }),
  /** Every label defined in the repository. */
  labels: (target: Target) => invoke<IssueLabel[]>("issue_labels", { target }),
  detail: (target: Target, number: number) => invoke<IssueDetail>("issue_detail", { target, number }),
  create: (target: Target, title: string, body: string) => invoke<Issue>("issue_create", { target, title, body }),
  edit: (target: Target, number: number, title: string, body: string) => invoke<Issue>("issue_edit", { target, number, title, body }),
  setOpen: (target: Target, number: number, open: boolean, reason: CloseReason = "completed") =>
    invoke<Issue>("issue_set_open", { target, number, open, reason }),
  comment: (target: Target, number: number, body: string) => invoke<void>("issue_comment", { target, number, body }),
  /** Replaces the issue's labels; returns the ones it carries now. */
  setLabels: (target: Target, number: number, labels: string[]) => invoke<IssueLabel[]>("issue_set_labels", { target, number, labels }),
  /** Permanent; GitHub allows it to repository admins only. */
  delete: (target: Target, number: number) => invoke<void>("issue_delete", { target, number }),
};

export function errorMessage(e: unknown) {
  const raw = typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
  return raw === GITHUB_NOT_CONNECTED ? "GitHub sign-in missing or expired. Sign in again (see the PRs tab)." : raw;
}

export const isNotConnected = (e: unknown) => e === GITHUB_NOT_CONNECTED;
