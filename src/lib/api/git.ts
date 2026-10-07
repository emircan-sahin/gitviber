import { invoke } from "@tauri-apps/api/core";
import { network } from "./network";
import type { About, Blame, Branch, CleanUp, CleanedUp, Commit, CommitDetails, CommitOptions, Comparison, Definition, DefinitionRequest, DiffKind, DiffPair, Entry, FileChange, FileText, GitIdentity, GitInfo, GraphRefs, HistoryEdit, IgnoredFiles, Journal, JournalEntry, LargeFile, LinesPatch, LinesRequest, LogFilter, MainBack, MergeCheck, NetOp, NotifyPermission, OpenInApp, Opened, OpenedRepo, PatchPreview, ProjectInfo, PullMode, RemoteTags, RepoStatus, ResetMode, RevertLines, RewriteOutcome, SearchQuery, SearchResult, StackedBranches, Stash, StashFiles, SuggestKind, Whitespace, Worktree, WorktreeState } from "./types";

/** Commits per history page: every list asks for this many, and a full page means there may be more. */
export const LOG_PAGE = 200;

export const api = {
  openRepo: (path: string) => invoke<OpenedRepo>("open_repo", { path }),
  /** Checked once per launch; `recheck` runs `git --version` again. */
  gitInfo: (recheck = false) => invoke<GitInfo>("git_info", { recheck }),
  /** Opens macOS's Command Line Tools installer. */
  installGit: () => invoke<void>("install_git"),
  /** The open repo's user.name/email; `suggested` comes from a GitHub account already signed in. */
  gitIdentity: () => invoke<{ current: GitIdentity; suggested: GitIdentity | null }>("git_identity"),
  /** Sets the given parts in the global git config. */
  setGitIdentity: (name: string | null, email: string | null) => invoke<void>("set_git_identity", { name, email }),
  remoteList: () => invoke<{ name: string; url: string; pushUrl: string | null }[]>("remote_list"),
  /** Adds (name, url), removes (name), renames (name, to) or repoints (name, url) a remote. */
  remoteEdit: (action: "add" | "remove" | "rename" | "set-url", name: string, value: string | null = null) => invoke<void>("remote_edit", { action, name, value }),
  /** The open repository's own identity (its .git/config), and the global one. */
  repoIdentity: () => invoke<{ own: GitIdentity; global: GitIdentity }>("repo_identity"),
  /** Both, or null for none: the global identity applies again. */
  setRepoIdentity: (identity: { name: string; email: string } | null) => invoke<void>("set_repo_identity", identity ?? { name: null, email: null }),
  status: () => invoke<RepoStatus>("status"),
  about: () => invoke<About>("about"),
  /** macOS's material behind the window (the Translucency setting). */
  setTranslucent: (on: boolean) => invoke<void>("set_translucent", { on }),
  /** macOS's Reduce transparency accessibility setting; false elsewhere. */
  reduceTransparency: () => invoke<boolean>("reduce_transparency"),
  /**
   * HEAD's history, or `rev`'s: a remote-tracking branch (refs/remotes/…), e.g. a fork's original.
   * `all`: every branch, remote-tracking branch and tag it lets through, with what HEAD lacks marked.
   */
  log: (skip: number, limit: number, rev: string | null = null, filter: LogFilter | null = null, all: GraphRefs | null = null) =>
    invoke<Commit[]>("log", { rev, skip, limit, filter, all }),
  /** Comparing HEAD with a full ref: what it has that HEAD doesn't (`incoming`), or the other way. */
  logCompare: (ref: string, incoming: boolean, skip: number, limit: number) => invoke<Commit[]>("log_compare", { with: ref, incoming, skip, limit }),
  /** [HEAD has and `ref` doesn't, `ref` has and HEAD doesn't]. */
  /** Each submodule, the commit recorded for it, and how its checkout stands ("missing", "moved", "conflict", "ok"). */
  submodules: () => invoke<{ path: string; sha: string; state: "missing" | "moved" | "conflict" | "ok" }[]>("submodules"),
  /** Sets up and checks out every submodule at its recorded commit, nested ones too. */
  submoduleUpdate: (op?: NetOp) => network<void>("submodule_update", {}, op),
  /** Downloads a Git LFS file's object, so it shows. */
  lfsPull: (path: string, op?: NetOp) => network<void>("lfs_pull", { path }, op),
  /** Starts a bisect: `good` lacked the bug, HEAD has it. */
  bisectStart: (good: string) => invoke<{ message: string; firstBad: string | null }>("bisect_start", { good }),
  bisectMark: (verdict: "good" | "bad" | "skip") => invoke<{ message: string; firstBad: string | null }>("bisect_mark", { verdict }),
  /** Where HEAD has been, newest first (HEAD@{0} is where it is). */
  reflog: (limit = 200) => invoke<{ sha: string; selector: string; message: string; timestamp: number }[]>("reflog", { limit }),
  /** What a full ref changed since it and HEAD parted (a PR of it), as a range and its files. */
  compareFiles: (ref: string) => invoke<{ base: string; head: string; files: FileChange[] }>("compare_files", { with: ref }),
  /** What HEAD's branch changed since it left a full ref, uncommitted work included: the merge base, and files from it to the working tree. */
  branchReview: (base: string) => invoke<{ base: string; files: FileChange[] }>("branch_review", { base }),
  /** The working tree against a commit itself, no merge base: its full id, and the files from it to the working tree. */
  compareWorktree: (sha: string) => invoke<{ base: string; files: FileChange[] }>("compare_worktree", { sha }),
  /** Two points (HEAD, a full ref, or a commit id) against each other; `mergeBase`: the files are what `head` changed since they parted, as a PR shows them. */
  compare: (base: string, head: string, mergeBase: boolean) => invoke<Comparison>("compare", { base, head, mergeBase }),
  /** The commits `head` has that `base` doesn't, newest first (both commit ids, from a comparison). */
  logBetween: (base: string, head: string, skip: number, limit: number) => invoke<Commit[]>("log_between", { base, head, skip, limit }),
  /** What merging a point into HEAD would bring, and which files would conflict. */
  mergeCheck: (ref: string) => invoke<MergeCheck>("merge_check", { with: ref }),
  compareCounts: (ref: string) => invoke<[number, number]>("compare_counts", { with: ref }),
  /** The commit a SHA or SHA prefix names, if exactly one, or a full ref's tip (refs/heads/…). */
  findCommit: (sha: string) => invoke<Commit | null>("find_commit", { sha }),
  /** The full id of the commit each SHA or prefix names (null: none, or several); at most 500 at once. */
  knownCommits: (shas: string[]) => invoke<(string | null)[]>("known_commits", { shas }),
  commitFiles: (sha: string) => invoke<FileChange[]>("commit_files", { sha }),
  /** Files changed from `from` to `to`, both commit ids: a run of a PR's commits, from the first one's parent. */
  rangeFiles: (from: string, to: string) => invoke<FileChange[]>("range_files", { from, to }),
  diffPair: (kind: DiffKind, path: string, oldPath: string | null, sha: string | null, base: string | null = null, whitespace: Whitespace | null = null) =>
    invoke<DiffPair>("diff_pair", { kind, path, oldPath, sha, base, whitespace }),
  /** Raw bytes of one side of a diff (`original` = the before side), for media previews. */
  media: (kind: DiffKind, path: string, oldPath: string | null, sha: string | null, base: string | null, original: boolean) =>
    invoke<ArrayBuffer>("media", { kind, path, oldPath, sha, base, original }),
  /** That side as a file to copy: the working tree's own, or a stored version saved under its name. */
  mediaFile: (kind: DiffKind, path: string, oldPath: string | null, sha: string | null, base: string | null, original: boolean) =>
    invoke<string>("media_file", { kind, path, oldPath, sha, base, original }),
  /** Puts `gitviber` on PATH (cli.rs) and says where. */
  installCli: () => invoke<string>("install_cli"),
  /** Ends the app, once the page saved what it keeps (lib/app/quit). */
  quit: () => invoke<void>("quit"),
  /** Brings the settings window forward, on `section` if given, opening it when `open`; false when there's none to bring. */
  settingsWindow: (section: string | null, open: boolean) => invoke<boolean>("settings_window", { section, open }),
  /** Paths opened from outside the window (a CLI, the Dock, a second launch) since last asked. */
  takeOpened: () => invoke<Opened>("take_opened"),
  /** Absolute paths onto the pasteboard as Finder copies files; an image carries its picture too. macOS only. */
  copyFiles: (paths: string[]) => invoke<void>("copy_files", { paths }),
  listDir: (path: string) => invoke<Entry[]>("list_dir", { path }),
  /** Tracked and untracked files, not ignored ones: what quick open searches. */
  listFiles: () => invoke<string[]>("list_files"),
  /** Ignored files, and ignored folders whole (node_modules, not what's in it): what the explorer's filter adds to listFiles. */
  listIgnored: () => invoke<Entry[]>("list_ignored"),
  /** Each repo path's kind on disk, null when it's missing: ignored files and folders, which listFiles leaves out. */
  pathKinds: (paths: string[]) => invoke<("file" | "dir" | null)[]>("path_kinds", { paths }),
  /** Rejects with SEARCH_CANCELLED when a newer search stops it. */
  searchFiles: (query: SearchQuery) => invoke<SearchResult>("search_files", { query }),
  cancelSearch: () => invoke<void>("cancel_search"),
  /** Rejects with DEFINITIONS_CANCELLED when a newer lookup stops it. */
  definitions: (request: DefinitionRequest) => invoke<Definition[]>("definitions", { request }),
  /** Where the name is used, its definition and imports included; rejects like `definitions`. */
  references: (request: DefinitionRequest) => invoke<Definition[]>("references", { request }),
  /** Modified times (ms) of repo files; null: not on disk. */
  fileMtimes: (paths: string[]) => invoke<(number | null)[]>("file_mtimes", { paths }),
  readFile: (path: string) => invoke<FileText>("read_file", { path }),
  /** Every file in a commit (`<sha>`, or `<sha>^` for its parent): where its links resolve. */
  treePaths: (rev: string) => invoke<string[]>("tree_paths", { rev }),
  textAt: (rev: string, path: string) => invoke<FileText>("text_at", { rev, path }),
  /** `git blame` of the working-tree file. */
  blame: (path: string) => invoke<Blame>("blame", { path }),
  branches: () => invoke<Branch[]>("branches"),
  /** Local branches squash- or rebase-merged on the remote, which then deleted them. */
  mergedUpstream: () => invoke<string[]>("merged_upstream"),
  /** Where this worktree's HEAD went last (its reflog's checkouts), newest first; some may be gone or be commits. */
  recentBranches: () => invoke<string[]>("recent_branches"),
  /** Deletes `merged` branches (git branch -d) and `upstream` ones, checked again as they are now, with -D. One undo. */
  deleteMerged: (merged: string[], upstream: string[]) => invoke<void>("delete_merged", { merged, upstream }),
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
  deleteRemoteBranch: (name: string, op?: NetOp) => network<void>("delete_remote_branch", { name }, op),
  /** A new branch at `base` (refs/heads/…, refs/remotes/… or refs/tags/…), not tracking it; `switchTo` checks it out. */
  createBranch: (name: string, base: string, switchTo: boolean) => invoke<void>("create_branch", { name, base, switch: switchTo }),
  /** `remote`: also push the new name, track it, and delete the upstream's old name there. */
  renameBranch: (old: string, name: string, remote: boolean, op?: NetOp) => network<void>("rename_branch", { old, new: name, remote }, op),
  /** `upstream`: a remote-tracking branch (origin/feat), or null to track nothing. */
  setUpstream: (branch: string, upstream: string | null) => invoke<void>("set_upstream", { branch, upstream }),
  /** Tag names, newest first. */
  tags: () => invoke<string[]>("tags"),
  worktrees: () => invoke<Worktree[]>("worktrees"),
  /** Uncommitted files in one of this repo's worktrees, and commits found nowhere else. */
  /** `upstream`: also whether a squash or rebase merge upstream took its commits, which reads diffs. */
  worktreeState: (path: string, upstream = true) => invoke<WorktreeState>("worktree_state", { path, upstream }),
  /**
   * Checks a branch out in a new worktree in `dir` (default: beside the main one); returns its path.
   * With `base` (a full ref, or HEAD) the branch is new, made there; with `track` it tracks `base` (its remote namesake).
   */
  addWorktree: (branch: string, base: string | null = null, dir: string | null = null, track = false) => invoke<string>("add_worktree", { branch, base, track, dir }),
  /** What `moveMainBack` would change for `branch`, which a linked worktree holds; rejects with why not (changes in either folder, a detached main folder…). */
  mainBackPlan: (branch: string) => invoke<MainBack>("main_back_plan", { branch }),
  /** Detaches the worktree holding `branch` where it stands, then switches the main folder to it. */
  moveMainBack: (branch: string) => invoke<MainBack>("move_main_back", { branch }),
  undoMainBack: (plan: MainBack) => invoke<void>("undo_main_back", { plan }),
  /** How many gitignored files the main worktree's .worktreeinclude has addWorktree copy in. */
  worktreeIncludes: () => invoke<number>("worktree_includes"),
  /** Renames a worktree's branch and, with `moveFolder`, its folder to match; returns its path afterwards. */
  renameWorktree: (path: string, branch: string, moveFolder: boolean) => invoke<string>("rename_worktree", { path, branch, moveFolder }),
  /** Keeps a worktree from being pruned, moved or removed; `reason` shows on its row. */
  lockWorktree: (path: string, reason: string | null) => invoke<void>("lock_worktree", { path, reason }),
  unlockWorktree: (path: string) => invoke<void>("unlock_worktree", { path }),
  /** Deletes a linked worktree's folder (its branch stays); `force` drops uncommitted files and overrides a lock. */
  removeWorktree: (path: string, force: boolean) => invoke<void>("remove_worktree", { path, force }),
  /** What removing a worktree deletes that git doesn't count as a change: its ignored files, sized, largest first. */
  worktreeIgnored: (path: string) => invoke<IgnoredFiles>("worktree_ignored", { path }),
  /** Removes merged worktrees, each checked again, and the branches known merged (Undo brings those back). */
  cleanUpWorktrees: (list: CleanUp[]) => invoke<CleanedUp>("clean_up_worktrees", { list }),
  /** Nested repositories are refused unless `allowNested`: git would stage only a gitlink. */
  stage: (paths: string[], allowNested = false) => invoke<void>("stage", { paths, allowNested }),
  /**
   * A staged rename's old path goes too, or its deletion would stay staged. A copy's source
   * (status.renames=copies) is a file of its own, whose staged edits stay.
   */
  unstage: (files: FileChange[]) => invoke<void>("unstage", { paths: files.flatMap((f) => (f.status === "R" && f.oldPath ? [f.path, f.oldPath] : [f.path])) }),
  discard: (paths: string[]) => invoke<void>("discard", { paths }),
  /** Removes the index.lock at `path`, as git's error named it: only this repository's, and only once it's a few seconds old. */
  removeIndexLock: (path: string) => invoke<void>("remove_index_lock", { path }),
  /** Stages, unstages or discards some lines of a diff (lines.rs); a discard is undoable. */
  changeLines: (request: LinesRequest) => invoke<void>("change_lines", { request }),
  /** Files' changes as a patch: unstaged (untracked ones as new files), staged, or a commit's. `paths` take a rename's old path too. */
  changesPatch: (kind: "unstaged" | "staged" | "commit", paths: string[], sha: string | null = null) => invoke<string>("changes_patch", { kind, paths, sha }),
  /** A commit as `git format-patch` writes it. */
  commitPatch: (sha: string) => invoke<string>("commit_patch", { sha }),
  /** A stash's changes, its untracked files too. */
  stashPatch: (sha: string) => invoke<string>("stash_patch", { sha }),
  linesPatch: (request: LinesPatch) => invoke<string>("lines_patch", { request }),
  /** The clipboard's text, read by the app: the webview's own read asks the user first. */
  clipboardText: () => invoke<string>("clipboard_text"),
  /** Puts text on the clipboard after the click that asked for it is over, which the webview can't. */
  clipboardWrite: (text: string) => invoke<void>("clipboard_write", { text }),
  patchPreview: (patch: string) => invoke<PatchPreview>("patch_preview", { patch }),
  /** Applies a patch to the working tree (merging three ways where it must); undoable. Returns the files left with conflict markers. */
  applyPatch: (patch: string) => invoke<string[]>("apply_patch", { patch }),
  /** `path` in the working tree as commit `sha` has it; undoable. */
  restoreFile: (sha: string, path: string) => invoke<void>("restore_file", { sha, path }),
  /** Undoes what commit `sha` did to the file, in the working tree; undoable. Returns the files left with conflicts. */
  revertFile: (sha: string, path: string, oldPath: string | null) => invoke<string[]>("revert_file", { sha, path, oldPath }),
  revertLines: (request: RevertLines) => invoke<string[]>("revert_lines", { request }),
  /** An empty `message` with `amend` keeps the old one (--no-edit). Watched as a network command: hook output is its progress, and Cancel stops it (once git has moved HEAD, the commit stands). */
  commit: (message: string, options: CommitOptions, op?: NetOp) => network<void>("commit", { message, options }, op),
  /** Staged files GitHub would refuse a push of (over 100 MiB), by their staged blobs; LFS files are staged as pointers, so they don't count. */
  largeStaged: () => invoke<LargeFile[]>("large_staged"),
  /** The message git would start with, comment lines stripped: a squash merge's or `cherry-pick -n`'s, else `commit.template`; null for neither. */
  commitTemplate: () => invoke<string | null>("commit_template"),
  /** "Name <email>" of recent authors and co-authors, newest first, not the user. */
  recentAuthors: () => invoke<string[]>("recent_authors"),
  /**
   * Runs the user's agent CLI (`command`, e.g. "claude -p") in the repo with `prompt` and the
   * diff `scope` would commit; returns what it printed. Rejects with SUGGEST_CANCELLED on cancel.
   */
  suggestMessage: (command: string, prompt: string, scope: "staged" | "all" | "amend") => invoke<string>("suggest_message", { command, prompt, scope }),
  /** The same for a pull request from HEAD into `base` (refs/remotes/…): its commits, PR template and diff. */
  suggestPull: (command: string, prompt: string, base: string) => invoke<string>("suggest_pull", { command, prompt, base }),
  /** Stops the run of `kind`, leaving the other's. */
  suggestCancel: (kind: SuggestKind) => invoke<void>("suggest_cancel", { kind }),
  /** Signature status and trailers of one commit (verifying runs gpg/ssh, so one at a time). */
  commitDetails: (sha: string) => invoke<CommitDetails>("commit_details", { sha }),
  /**
   * `force`: --force-with-lease, after a rebase or amend. `remote`: where to publish a branch with no upstream.
   * `tags`: --follow-tags, annotated tags on the pushed commits go too.
   */
  push: (force = false, remote?: string, op?: NetOp, tags = false) => network<void>("push", { force, remote, tags }, op),
  /** After a non-fast-forward push: the remote's extra commits were this branch's own (rebased or amended since). */
  remoteWasOurs: () => invoke<boolean>("remote_was_ours"),
  // The boolean results mean "stopped on conflicts".
  /** `autostash`: uncommitted changes in the way are stashed first and reapplied after. */
  pull: (mode: PullMode, op?: NetOp, autostash = false) => network<boolean>("pull", { mode, autostash }, op),
  /** `how`: fast-forward when possible, always a merge commit, or the branch's changes as one commit. */
  merge: (name: string, how: "ff" | "no-ff" | "squash" = "ff", autostash = false) => invoke<boolean>("merge", { name, how, autostash }),
  /** `updateRefs`: move the branches on the replayed commits along (or not); undefined leaves it to rebase.updateRefs. */
  rebase: (onto: string, autostash = false, updateRefs?: boolean) => invoke<boolean>("rebase", { onto, autostash, updateRefs }),
  /** The branches a rebase onto `onto` would replay commits of. */
  rebaseStacked: (onto: string) => invoke<StackedBranches>("rebase_stacked", { onto }),
  opContinue: () => invoke<boolean>("op_continue"),
  opAbort: () => invoke<void>("op_abort"),
  rebaseSkip: () => invoke<boolean>("rebase_skip"),
  resolveSide: (path: string, side: "ours" | "theirs") => invoke<void>("resolve_side", { path, side }),
  /** A conflicted file's sides: `merged` again in diff3 style (null for a binary or huge side), and whether each ends with a newline. Null when a side deleted it. */
  conflictBase: (path: string) => invoke<{ merged: string | null; oursNewline: boolean; theirsNewline: boolean } | null>("conflict_base", { path }),
  /** The merge and diff tools git's config names (as `--gui` reads it); null where none a window can run. */
  externalTools: () => invoke<{ merge: string | null; diff: string | null }>("external_tools"),
  /** `git mergetool` on the path, settling once the tool is closed. */
  openMergeTool: (path: string) => invoke<void>("open_merge_tool", { path }),
  /** `git difftool` on the path's unstaged changes, or `staged` ones; settles once the tool is closed. */
  openDiffTool: (path: string, staged: boolean) => invoke<void>("open_diff_tool", { path, staged }),
  writeFile: (path: string, content: string) => invoke<void>("write_file", { path, content }),
  createFile: (path: string) => invoke<void>("create_file", { path }),
  createDir: (path: string) => invoke<void>("create_dir", { path }),
  renamePath: (from: string, to: string) => invoke<void>("rename_path", { from, to }),
  /** Moves to the system Trash, so it can be put back. */
  trashPath: (path: string) => invoke<void>("trash_path", { path }),
  revealPath: (path: string) => invoke<void>("reveal_path", { path }),
  /** Known editors, terminals and git apps found on this machine (open_in.rs). */
  openInApps: () => invoke<OpenInApp[]>("open_in_apps"),
  /** A found app's icon as PNG bytes; empty where there's none (off macOS). */
  openInIcon: (app: string) => invoke<ArrayBuffer>("open_in_icon", { app }),
  /** `path` in the worktree ("" for all of it) in an app; editors that can go to `line` do. `project`: the worktree as a project, with `path` open in it. */
  openIn: (app: string, path: string, line: number | undefined, project: boolean) => invoke<void>("open_in", { app, path, line, project }),
  /** The same with the user's own command. */
  openInCustom: (command: string, path: string, line: number | undefined, project: boolean) => invoke<void>("open_in_custom", { command, path, line, project }),
  /** Any saved project's folder, not just the open repo's. */
  revealProject: (path: string) => invoke<void>("reveal_project", { path }),
  projectInfo: (paths: string[]) => invoke<ProjectInfo[]>("project_info", { paths }),
  folderBranch: (path: string) => invoke<string | null>("folder_branch", { path }),
  fetch: (op?: NetOp) => network<void>("fetch", {}, op),
  /** When the repo last fetched (FETCH_HEAD's mtime, Unix seconds); null if never. */
  lastFetch: () => invoke<number | null>("last_fetch"),
  /** Clones into `parent/name` (refused if that holds anything); returns the new repo's path. */
  cloneRepo: (url: string, parent: string, name: string, op?: NetOp) => network<string>("clone_repo", { url, parent, name }, op),
  /** `git init` in a folder that isn't in a repository yet. */
  initRepo: (path: string) => invoke<void>("init_repo", { path }),
  /** Adds the repository git refuses at `path` for its owner to the global safe.directory: the folder git's error names, only while git refuses it. */
  trustFolder: (path: string) => invoke<void>("trust_folder", { path }),
  // History actions. `sha` on undo and `head` on reset are the HEAD the user saw (refused if it moved).
  undoCommit: (sha: string) => invoke<void>("undo_commit", { sha }),
  reset: (sha: string, mode: ResetMode, head: string) => invoke<void>("reset", { sha, mode, head }),
  /** Moving HEAD to `sha` would drop commits the upstream already has (needs a force-push); null: rewriting from the root, so any it has. */
  dropsPushed: (sha: string | null) => invoke<boolean>("drops_pushed", { sha }),
  revert: (sha: string) => invoke<boolean>("revert", { sha }),
  cherryPick: (sha: string) => invoke<boolean>("cherry_pick", { sha }),
  /** Picks `shas`, oldest first, onto HEAD; true = stopped on conflicts. */
  cherryPickMany: (shas: string[]) => invoke<boolean>("cherry_pick_many", { shas }),
  /** Picks onto the branch of another worktree (`path`), running git there; true = stopped on conflicts there. */
  cherryPickInto: (path: string, sha: string) => invoke<boolean>("cherry_pick_into", { path, sha }),
  checkoutCommit: (sha: string) => invoke<void>("checkout_commit", { sha }),
  /** Edits the branch's history (rewrite.rs); `head` as the history showed it; `branches`: the local branches on the commits it replays move along. */
  rewrite: (head: string, edit: HistoryEdit, branches = false) => invoke<RewriteOutcome>("rewrite", { head, edit, branches }),
  /** The branches `edit` would replay commits of. */
  stackedBranches: (edit: HistoryEdit) => invoke<StackedBranches>("stacked_branches", { edit }),
  stashes: () => invoke<Stash[]>("stashes"),
  stashFiles: (sha: string) => invoke<StashFiles>("stash_files", { sha }),
  /** `untracked`: take untracked files along (nested repositories stay); `staged`: only what's staged; `paths`: only those files. */
  stashPush: (message: string, untracked: boolean, staged = false, paths: string[] = []) => invoke<void>("stash_push", { message, untracked, staged, paths }),
  /** A branch where the stash was made, with it applied, then dropped; true = stopped on conflicts. */
  stashBranch: (name: string, sha: string) => invoke<boolean>("stash_branch", { name, sha }),
  /** `pop` also drops it, unless it stopped on conflicts (true). */
  stashApply: (sha: string, pop: boolean) => invoke<boolean>("stash_apply", { sha, pop }),
  stashDrop: (sha: string) => invoke<void>("stash_drop", { sha }),
  /** Stashes the chosen lines of an unstaged diff, the index untouched; the file's old version goes to the Trash (see `tracked`). */
  stashLines: (message: string, request: LinesRequest) => invoke<void>("stash_lines", { message, request }),
  /** One file of a stash into the working tree (`untracked`: from its untracked files); what it replaces goes to the Trash. */
  stashRestoreFile: (sha: string, path: string, untracked: boolean) => invoke<void>("stash_restore_file", { sha, path, untracked }),
  /** Another message for a stash, which moves to the top. */
  stashRename: (sha: string, message: string) => invoke<void>("stash_rename", { sha, message }),
  createBranchAt: (name: string, sha: string) => invoke<void>("create_branch_at", { name, sha }),
  /** With a `message`, an annotated tag. */
  createTag: (name: string, sha: string, message?: string) => invoke<void>("create_tag", { name, sha, message }),
  deleteTag: (name: string) => invoke<void>("delete_tag", { name }),
  // Tags go where `git push` sends the current branch; these return that remote.
  pushTags: (names: string[], op?: NetOp) => network<string>("push_tags", { names }, op),
  deleteRemoteTag: (name: string, op?: NetOp) => network<string>("delete_remote_tag", { name }, op),
  /** The tags that remote has. A network call: use `remoteTags` in lib/repo/remoteTags.ts, which caches it. */
  remoteTags: (op?: NetOp) => network<RemoteTags>("remote_tags", {}, op),
  /** The commit to link the working tree's `path` (or its 1-based `lines`) to on GitHub, and where the lines are in it. Refused, saying why, when it isn't there yet. */
  githubPermalink: (path: string, lines: [number, number] | null) => invoke<{ sha: string; tree: boolean; lines: [number, number] | null }>("github_permalink", { path, lines }),
  journal: () => invoke<Journal>("journal"),
  /** The newest entry's id; a change across an action means it was recorded. */
  journalLast: () => invoke<number | null>("journal_last"),
  /** `id`: the entry meant; refused if it's no longer the next one. */
  undo: (id?: number) => invoke<JournalEntry>("undo", { id }),
  redo: (id?: number) => invoke<JournalEntry>("redo", { id }),
  /** The menu bar's items that changed (menu.rs), and File → Open Recent when it did. */
  setMenu: (items: Record<string, unknown>, recent: unknown) => invoke<void>("set_menu", { items, recent }),
  /** Into the app's error log (errors.rs). */
  logError: (source: string, message: string) => invoke<void>("log_error", { source, message }),
  /** Selects the error log in the file manager. */
  showLogs: () => invoke<void>("show_logs"),
  /** How this install updates (updates.rs): in place, from the Releases page, or not at all. */
  updateMode: () => invoke<"install" | "download" | null>("update_mode"),
  /** Whether the OS shows GitViber's notifications (notifications.rs). */
  notificationPermission: () => invoke<NotifyPermission>("notification_permission"),
  /** Asks the OS, which asks the user the first time; resolves once they answer. */
  requestNotifications: () => invoke<NotifyPermission>("notification_request"),
  /** `target` comes back with a "notification-click" event when the user clicks it. */
  notify: (title: string, body: string, target?: string) => invoke<void>("notification_send", { title, body, target }),
  /** System Settings → Notifications at GitViber (macOS). */
  notificationSettings: () => invoke<void>("notification_settings"),
};
