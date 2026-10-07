export type ChangeStatus = "M" | "A" | "D" | "R" | "C" | "T" | "U" | "?";

export interface FileChange {
  path: string;
  oldPath: string | null;
  status: ChangeStatus;
  additions: number | null;
  deletions: number | null;
  /** Content identity (index blob, size+mtime in the worktree, or a moved submodule's commit); changes on every edit. */
  oid: string | null;
  /** Unstaged entries: the index blob the change is against, which a mixed reset moves with nothing else changing. */
  indexOid: string | null;
  /** Conflicts only: UU both modified, AA both added, UD/DU deleted by them/us, AU/UA, DD. */
  conflict: string | null;
  /** "100644 → 100755" when the file mode changed (chmod +x); the text diff doesn't show it. */
  mode: string | null;
  /** A submodule's unstaged entry: `S<c><m><u>`, C its commit moved, M tracked changes and U untracked files inside it (else `.`). */
  submodule: string | null;
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

/** What handing a branch a linked worktree holds back to the main folder changes. */
export interface MainBack {
  main: string;
  /** The branch the main folder was on; it stays a branch. */
  mainBranch: string;
  holder: string;
  branch: string;
}

/** Paths opened from outside the window (opened.rs): what to open, and what named nothing. */
export interface Opened {
  open: {
    /** The folder to open as a project. */
    folder: string;
    /** A file in it to show, from its repository's root. */
    file: string | null;
    line: number | null;
    column: number | null;
  }[];
  missing: string[];
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
  kind: "merge" | "rebase" | "cherry-pick" | "revert" | "bisect";
  subject: string | null;
  step: number | null;
  total: number | null;
  /** A rebase stopped on a commit Split Commit took apart: its pieces are the user's to commit. */
  split: boolean;
}

export interface RepoStatus {
  root: string;
  branch: string | null;
  head: string | null;
  upstream: string | null;
  /** A checked-out pull request's number (`#7`) when the branch follows its refs/pull/<n>/head instead of an upstream. */
  follows: string | null;
  /** `upstream` is configured but isn't there (deleted on the remote, or never pushed): nothing to pull, Publish pushes it. */
  upstreamGone: boolean;
  ahead: number;
  behind: number;
  /** Where `git push` sends this branch; a fork can pull from upstream and push to origin. */
  push: { remote: string; branch: string | null; ahead: number } | null;
  remotes: string[];
  /** Origin's URL (null: none): after a `git remote set-url` the GitHub views read another repository. Undefined when git couldn't say. */
  origin?: string | null;
  /** Where Publish sends a branch with no upstream; null when the user has to pick a remote. */
  publish: string | null;
  staged: FileChange[];
  unstaged: FileChange[];
  conflicted: FileChange[];
  operation: Operation | null;
  /** Set while git has left a message for the next commit (a squash merge, `cherry-pick -n`), which commitTemplate returns; changes with it. */
  preparedMessage: string | null;
  /** origin's page on GitHub (null: not there); undefined with `origin`. */
  webUrl?: string | null;
}

export interface Commit {
  sha: string;
  shortSha: string;
  authorName: string;
  authorEmail: string;
  /** When it was written (author date). */
  timestamp: number;
  /** Who put it on the branch, and when: a rebase, cherry-pick or amend moves this, not `timestamp`. */
  committerName: string;
  committedAt: number;
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
  /** A followed file's history: its path in this commit (it may have been renamed since). */
  file?: string;
}

export interface BlameCommit {
  /** All zeros for lines not committed yet. */
  sha: string;
  authorName: string;
  authorEmail: string;
  timestamp: number;
  /** The whole message, trailers (Co-Authored-By…) included. */
  message: string;
  /** The file's path in this commit (it may have been renamed since). */
  path: string;
}

export interface Blame {
  commits: BlameCommit[];
  /** For each line of the working-tree file, its commit's index in `commits`. Lines past the end, or not in HEAD at all, are new. */
  lines: number[];
  /** Why this file has no blame (a Git LFS file). */
  unavailable: string | null;
}

/** History search (git.rs `LogFilter`): every part narrows the list. */
export interface LogFilter {
  /** Words the message must all contain, any case, as typed. */
  grep: string[];
  author: string[];
  /** Added or removed text (`git log -S`). */
  code: string | null;
  paths: string[];
  /** Follow a single path through renames (a file's history). */
  follow: boolean;
}

/** Which refs the all-branches history walks; refs are full names (refs/heads/…). */
export interface GraphRefs {
  local: boolean;
  remote: boolean;
  tags: boolean;
  /** Left out, though still listed where another shown ref reaches them. */
  hidden: string[];
  /** Just this ref's history, in place of everything else and HEAD. */
  only: string | null;
}

export interface CommitOptions {
  amend?: boolean;
  /** --signoff: a Signed-off-by trailer for the committer. */
  signOff?: boolean;
  /** --no-verify: skips the pre-commit and commit-msg hooks. */
  noVerify?: boolean;
  /** "Name <email>" each, added as Co-authored-by trailers. */
  coAuthors?: string[];
}

/** A staged file over GitHub's 100 MiB limit; `size` reads "123.4 MB". */
export interface LargeFile {
  path: string;
  size: string;
}

export interface CommitDetails {
  /** git's %G?: G good, U good but unknown validity, X/Y expired signature/key, R revoked key, B bad, E can't check, N none. */
  signature: string;
  signer: string;
  /** commit.gpgSign is on, so an unsigned commit is worth pointing out. */
  signExpected: boolean;
  trailers: [string, string][];
}

export interface FileText {
  text: string;
  binary: boolean;
  tooLarge: boolean;
  exists: boolean;
  /** Not the file's own UTF-8 text (invalid UTF-8 shown lossily, UTF-16, or a symlink's target); must never be written back. */
  lossy: boolean;
  /** A Git LFS file whose object isn't downloaded: the message to show, with its size. */
  lfsMissing: string | null;
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
  /** Ignoring whitespace hid changed lines. */
  whitespaceHidden: boolean;
  /** Every changed line differs only in its line ending (CRLF against LF). */
  eolOnly: boolean;
}

/** Whitespace a diff ignores: changes in its amount (git's -b), or all of it (-w). */
export type Whitespace = "amount" | "all";

/** Search in files (grep.rs): the query and Monaco's toggles, with VS Code's include/exclude globs. */
export interface SearchQuery {
  text: string;
  matchCase: boolean;
  wholeWord: boolean;
  regex: boolean;
  /** Comma-separated globs; one without a "/" matches at any depth. */
  include: string;
  exclude: string;
}

export interface SearchResult {
  files: { path: string; hits: { line: number; text: string }[] }[];
  count: number;
  /** Stopped at SEARCH_MAX_HITS. */
  capped: boolean;
  timedOut: boolean;
  /** The regex ran as POSIX extended: this git has no PCRE, so \d, \w, \b and lookarounds don't work as usual. */
  posix: boolean;
}

/** grep.rs MAX_HITS. */
export const SEARCH_MAX_HITS = 2000;

/** Where a name is defined (definitions.rs): 1-based line, 0-based UTF-16 columns. */
export interface Definition {
  path: string;
  line: number;
  column: number;
  endColumn: number;
}

export interface DefinitionRequest {
  path: string;
  text: string;
  line: number;
  column: number;
  rev: string | null;
}

/**
 * A change to the branch's history (rewrite.rs): reword a commit; squash `shas` and `onto` into one
 * where `onto` is (fixup, keeping the oldest's message, without a message); drop commits; move one
 * past its neighbour, or several just under `before` (on top when null); split one into pieces
 * to commit (the rebase stops on it); fix the staged changes up into one.
 */
export type HistoryEdit =
  | { kind: "reword"; sha: string; message: string }
  | { kind: "squash"; shas: string[]; onto: string; message: string | null }
  | { kind: "drop"; shas: string[] }
  | { kind: "move"; sha: string; up: boolean }
  | { kind: "reorder"; shas: string[]; before: string | null }
  | { kind: "split"; sha: string }
  | { kind: "fixupStaged"; sha: string };

/** How a rewrite ended: done, waiting on conflicts, or done with the uncommitted changes it set aside conflicting as they came back (kept in the stash too). */
export type RewriteOutcome = "done" | "conflicts" | "stashConflicts" | "split";

/** The local branches an edit or a rebase replays commits of, which can move along with them; `updateRefs`: rebase.updateRefs asks for that. */
export interface StackedBranches {
  branches: string[];
  updateRefs: boolean;
}

export interface LinesRequest {
  path: string;
  oldPath: string | null;
  kind: "unstaged" | "staged";
  action: "stage" | "unstage" | "discard" | "stash";
  /** The texts the diff showed (null: no such file), so a file changed since isn't touched. */
  original: string | null;
  modified: string | null;
  /** Removed lines by their old line, added ones by their new line. */
  removed: number[];
  added: number[];
}

/** Some lines of a diff as a patch: the sides as shown, and the chosen lines as LinesRequest has them. */
export interface LinesPatch {
  path: string;
  oldPath: string | null;
  original: string | null;
  modified: string | null;
  removed: number[];
  added: number[];
}

/** Some of a commit's changed lines to undo in the working tree, numbered as its diff shows them. */
export interface RevertLines {
  sha: string;
  path: string;
  oldPath: string | null;
  removed: number[];
  added: number[];
}

/** What a patch would do (patch.rs): its files, and whether it applies as it is ("clean"), only merged three ways ("merge"), or not (`error`). */
export interface PatchPreview {
  files: { path: string; oldPath: string | null; status: "A" | "D" | "R" | "M"; additions: number | null; deletions: number | null }[];
  applies: "clean" | "merge" | null;
  error: string | null;
}

export interface Entry {
  name: string;
  path: string;
  isDir: boolean;
  ignored: boolean;
}

export interface WorktreeState {
  uncommitted: number;
  /** Commits the default branch lacks; on the default branch itself, commits no remote has. 0 once squash- or rebase-merged upstream. */
  commits: number;
  /** Committed on, then fully taken into the default branch, or squash- or rebase-merged upstream. */
  merged: boolean;
  /** Unix seconds of its last activity: HEAD moving (made, committed, checked out) or an uncommitted file changing. */
  updated: number | null;
}

/** A worktree's ignored files: a folder ignored whole is one entry, ending in "/". */
export interface IgnoredFiles {
  entries: { path: string; bytes: number; files: number }[];
  /** False when the count stopped early or a folder couldn't be read: the sizes are at least these. */
  complete: boolean;
  /** Folders in it this user can't read or delete: removing it would stop partway, so Clean up leaves it. */
  denied: string[];
}

/** A worktree to clean up; `mergedHead`: its merged pull request's head, for a merge git can't see (a squash). */
export interface CleanUp {
  path: string;
  mergedHead: string | null;
}

export interface CleanedUp {
  removed: string[];
  /** Branches deleted with their worktrees. */
  deleted: string[];
  /** Branches kept: what they hold isn't known to be merged. */
  kept: string[];
  /** Worktrees left, and why. */
  failed: [string, string][];
}

export interface Branch {
  name: string;
  remote: boolean;
  current: boolean;
  upstream: string | null;
  /** The tip's commit id. */
  sha: string;
  timestamp: number;
  /** Checked out in another worktree (its path); git won't switch to it here. */
  worktree: string | null;
  /** Local, not current, and fully in HEAD: deleting it loses nothing. Never the default branch. */
  merged: boolean;
  /** What its remote's HEAD points at (origin/main). */
  remoteDefault: boolean;
}

/** A local branch ahead of, behind or gone from its upstream (only those are listed). */
export interface BranchTracking {
  name: string;
  /** Commits it has that its upstream hasn't, and the other way round. */
  ahead: number;
  behind: number;
  /** Its upstream is set but no longer exists (deleted on the remote, then pruned). */
  gone: boolean;
}

export type PullMode = "ff" | "merge" | "rebase";

/** The remote tags are pushed to, and the tags it has. */
export interface RemoteTags {
  remote: string;
  names: string[];
}

export interface Stash {
  /** Actions name a stash by this: stash@{n} shifts as others are pushed and dropped. */
  sha: string;
  /** Its n in stash@{n} now. */
  index: number;
  /** As git words it: "On main: message", or "WIP on main: <commit>". */
  message: string;
  author: string;
  timestamp: number;
}

export interface StashFiles {
  /** Tracked changes, against the commit the stash was made on. */
  files: FileChange[];
  /** The commit holding its untracked files (a stash made with them), diffed from nothing. */
  untrackedSha: string | null;
  untracked: FileChange[];
}

/** One of the app's own git actions, as the undo history lists it. */
export interface JournalEntry {
  id: number;
  label: string;
  /** Unix seconds. */
  time: number;
  /** The branch (or short commit) undoing it switches to, in the undo list; redoing it, in the redo list. Null: no switch. */
  switchTo: string | null;
  /** Files a redo of a file action (a restore, a patch) left with conflict markers. */
  conflicts: string[];
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

/** "quiet": allowed without banners, into Notification Center only; "unbundled": a dev build macOS can't ask (notifications.rs). */
export type NotifyPermission = "granted" | "quiet" | "denied" | "prompt" | "unbundled";

export interface OpenInApp {
  id: string;
  name: string;
  group: "editor" | "terminal" | "other";
}

export type DiffKind = "unstaged" | "staged" | "worktree" | "commit" | "range" | "base" | "files";

export interface About {
  version: string;
  /** Short commit the build came from; empty when built outside a git checkout. */
  commit: string;
  os: string;
  arch: string;
  /** `git --version` without the prefix; null when git can't run. */
  git: string | null;
  /** `gh --version`'s version; null when the GitHub CLI isn't installed. */
  gh: string | null;
  /** The web view and its version, e.g. "WebKit 20621.1.15". */
  webview: string | null;
}

export interface GitInfo {
  /** "tools": macOS's git stub, with no Command Line Tools behind it. */
  state: "ok" | "old" | "missing" | "tools";
  /** `git --version` without the prefix. */
  version: string | null;
  /** Why git can't run. */
  detail: string | null;
  /** The oldest git that works, e.g. "2.36". */
  minimum: string;
}

export interface GitIdentity {
  name: string | null;
  email: string | null;
}

/** Where a network command is, in git's words: "Receiving objects" at 45 (percent). */
export interface Progress {
  phase: string;
  /** Null for phases git only counts ("Enumerating objects"). */
  percent: number | null;
  /** False once git changes local files (a pull's merge, a clone's checkout): Cancel is ignored then. */
  cancellable: boolean;
}

/** One watched command (fetch, pull, push, clone, or a commit and its hooks): its progress, and its id for Cancel. */
export interface NetOp {
  id: string;
  onProgress?: (p: Progress) => void;
  /** The background fetch, which gives way to anything the user starts. */
  background?: boolean;
  /** Its git/ssh prompts are declined unseen: nobody asked for it, or it gives up on its own. Implied by `background`. */
  quiet?: boolean;
}

/** Something git or ssh asks during a network command: a password, a passphrase, a new host's key (askpass.rs). */
export interface AskPrompt {
  id: number;
  /** git's or ssh's own words, e.g. "Password for 'https://me@github.com': ". */
  text: string;
  /** ssh's SSH_ASKPASS_PROMPT: "confirm" (yes/no) or "none" (a notice ssh closes itself). */
  kind: string | null;
  /** The command asking, e.g. "git push". */
  label: string;
  /** The NetOp id of that command. */
  op: string | null;
  /** The hosts that command talks to; empty when unknown. */
  hosts: string[];
}

export type ResetMode = "soft" | "mixed" | "hard";

/** Which suggestion a run is for: the commit box's message, the pull request dialog's, or a guided review; each runs and cancels apart. */
export type SuggestKind = "message" | "pull" | "guide";

/** What a guided review is of: a commit, or HEAD's branch since it left `base` (a full ref). */
export type GuideTarget = { of: "commit"; sha: string } | { of: "branch"; base: string };

/** A guided review as the command printed it, and the range it read (`base..head`: a commit's parent and the commit, or the merge base and HEAD). */
export interface Guided {
  text: string;
  base: string;
  head: string;
}

/** Two points compared (the Compare screen): the commit ids they name, and what lies between them. */
export interface Comparison {
  base: string;
  head: string;
  /** Where each file's old side is: the merge base, or `base` itself. */
  from: string;
  /** No commit in common: the files are then the plain difference. */
  unrelated: boolean;
  /** Commits `head` has that `base` doesn't, and the other way round. */
  ahead: number;
  behind: number;
  files: FileChange[];
}

/** What merging a point into HEAD would do; `conflicts` null: git is too old to tell. */
export interface MergeCheck {
  incoming: number;
  unrelated: boolean;
  conflicts: string[] | null;
}
