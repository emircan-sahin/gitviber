import { type Selection, selectionKey } from "./selection";
import { getSettings } from "../settings";
import { isRecord, putRecent, readJson, stringList } from "../storage";
import { folderName, joinPath } from "../path";
import { isNote, type ReviewNote } from "../review/notes";
import { type HueChoice, isHueChoice } from "../git/worktrees";

/** What a worktree's window looked like, so reopening the app picks up where it was. */
interface WorkspaceSnapshot {
  tabs: { key: string; sel: Selection; preview: boolean }[];
  active: string | null;
  listTab: string;
  /** Viewed marks: `kind:path` → the file's content signature when it was marked. */
  viewed: [string, string][];
  /** The full ref the branch is reviewed against, while Changes shows that review ("" before one is picked). */
  review?: string | null;
}

/** A commit message being written in a worktree, kept until it's committed. */
export interface CommitDraft {
  summary: string;
  body: string;
  /** "Name <email>", added as Co-authored-by trailers. */
  coAuthors: string[];
  /** What summary and body started as (a prepared message, commit.template), to tell an untouched draft from the user's. */
  from?: { summary: string; body: string };
}

const KEY = "gitviber.workspaces";
const DRAFTS_KEY = "gitviber.drafts";
const EDITS_KEY = "gitviber.fileEdits";
const NOTES_KEY = "gitviber.reviewNotes";
const WORKTREE_DIRS_KEY = "gitviber.worktreeDirs";
const WORKTREE_RUN_KEY = "gitviber.worktreeRun";
const ISSUE_BRANCHES_KEY = "gitviber.issueBranches";
const ISSUE_RUN_KEY = "gitviber.issueRun";
const PINNED_BRANCHES_KEY = "gitviber.pinnedBranches";
const GITHUB_ACCOUNTS_KEY = "gitviber.githubAccounts";
const COLORS_KEY = "gitviber.worktreeColors";
// Agent worktrees come and go; keep only the most recently used.
const MAX = 30;

const all = (key: string) => readJson(key, {}, isRecord);
/** Stores `value` under `root` (null removes it), keeping the MAX most recently saved roots. */
const put = (key: string, root: string, value: unknown) => putRecent(key, root, value, MAX);

export function loadWorkspace(root: string): WorkspaceSnapshot | null {
  const s = all(KEY)[root] as WorkspaceSnapshot | undefined;
  if (!s || !Array.isArray(s.tabs) || !Array.isArray(s.viewed)) return null;
  // Keys are re-derived: their format changes (PRs went from number to url), and a stale key
  // would stop the list row matching its tab. A tab too malformed to key is dropped.
  const rekey = (sel: Selection) => {
    try {
      return selectionKey(sel);
    } catch {
      return null;
    }
  };
  const tabs = s.tabs.flatMap((t) => {
    const key = typeof t?.key === "string" && typeof t.sel?.kind === "string" ? rekey(t.sel) : null;
    return key ? [{ ...t, old: t.key, key }] : [];
  });
  const active = tabs.find((t) => t.old === s.active)?.key ?? null;
  return { ...s, tabs: tabs.map(({ old: _, ...t }) => t), active };
}

export function saveWorkspace(root: string, snapshot: WorkspaceSnapshot) {
  put(KEY, root, snapshot);
}

export function loadDraft(root: string): CommitDraft | null {
  const d = all(DRAFTS_KEY)[root] as Partial<CommitDraft> | undefined;
  if (!d || typeof d.summary !== "string" || typeof d.body !== "string") return null;
  const coAuthors = Array.isArray(d.coAuthors) ? d.coAuthors.filter((a) => typeof a === "string") : [];
  const from = isRecord(d.from) && typeof d.from.summary === "string" && typeof d.from.body === "string" ? { summary: d.from.summary, body: d.from.body } : undefined;
  return { summary: d.summary, body: d.body, coAuthors, from };
}

/** An empty draft is dropped rather than stored. */
export function saveDraft(root: string, draft: CommitDraft) {
  put(DRAFTS_KEY, root, draft.summary || draft.body || draft.coAuthors.length ? draft : null);
}

/** A file edited in the file view and not saved: its text, and the file's when the edit began. */
export interface FileEdit {
  text: string;
  base: string;
}

export function loadEdits(root: string): Record<string, FileEdit> {
  const saved = all(EDITS_KEY)[root];
  if (!isRecord(saved)) return {};
  const ok = (e: unknown): e is FileEdit => isRecord(e) && typeof e.text === "string" && typeof e.base === "string";
  return Object.fromEntries(Object.entries(saved).filter((entry): entry is [string, FileEdit] => ok(entry[1])));
}

/** False when storage is full: the edits then live only until the app quits. */
export function saveEdits(root: string, edits: Record<string, FileEdit>) {
  return put(EDITS_KEY, root, Object.keys(edits).length ? edits : null);
}

export function loadNotes(root: string): ReviewNote[] {
  const saved = all(NOTES_KEY)[root];
  return Array.isArray(saved) ? saved.filter(isNote) : [];
}

/** False when storage is full: the notes then live only until the app quits. */
export function saveNotes(root: string, notes: ReviewNote[]) {
  return put(NOTES_KEY, root, notes.length ? notes : null);
}

/** A worktree's folder moved: its layout, unsent commit message, unsaved files, review notes and color, kept by path, go along. */
export function moveRoot(from: string, to: string) {
  for (const key of [KEY, DRAFTS_KEY, EDITS_KEY, NOTES_KEY, COLORS_KEY]) {
    const saved = all(key)[from];
    if (saved === undefined) continue;
    put(key, from, null);
    put(key, to, saved);
  }
}

/** The colors picked for worktrees, by path (lib/git/worktreeColors); the rest take their name's. */
export function loadWorktreeColors(): Record<string, HueChoice> {
  return Object.fromEntries(Object.entries(all(COLORS_KEY)).filter((e): e is [string, HueChoice] => isHueChoice(e[1])));
}

export function saveWorktreeColor(path: string, choice: HueChoice) {
  put(COLORS_KEY, path, choice);
}

/** The folder the project `main` puts new worktrees in, when it isn't the default one beside it. */
export function loadWorktreeDir(main: string): string | null {
  const d = all(WORKTREE_DIRS_KEY)[main];
  return typeof d === "string" ? d : null;
}

/** null goes back to the default folder. */
export function saveWorktreeDir(main: string, dir: string | null) {
  put(WORKTREE_DIRS_KEY, main, dir);
}

/**
 * The command the project `main` last ran in a new worktree's terminal. `forIssue`: one started
 * from an issue, kept apart as its {issue} means nothing elsewhere; the plain one until there is one.
 */
export function loadWorktreeRun(main: string, forIssue = false): string {
  const r = forIssue ? (all(ISSUE_RUN_KEY)[main] ?? all(WORKTREE_RUN_KEY)[main]) : all(WORKTREE_RUN_KEY)[main];
  return typeof r === "string" ? r : "";
}

/** "" forgets it. */
export function saveWorktreeRun(main: string, run: string, forIssue = false) {
  put(forIssue ? ISSUE_RUN_KEY : WORKTREE_RUN_KEY, main, run || null);
}

/**
 * The issue (its url) `branch` was started for from the issue view, by origin's owner/name: a PR
 * from it closes that issue. Kept here, never in the repo's config.
 */
export function loadBranchIssue(repo: string, branch: string): string | null {
  const r = all(ISSUE_BRANCHES_KEY)[repo.toLowerCase()];
  const url = isRecord(r) ? r[branch] : null;
  return typeof url === "string" ? url : null;
}

export function saveBranchIssue(repo: string, branch: string, url: string) {
  const r = all(ISSUE_BRANCHES_KEY)[repo.toLowerCase()];
  const { [branch]: _, ...rest } = isRecord(r) ? r : {};
  put(ISSUE_BRANCHES_KEY, repo.toLowerCase(), Object.fromEntries([...Object.entries(rest), [branch, url]].slice(-MAX)));
}

/** The branches pinned to the top of the project `main`'s branch picker, in pin order. */
export const loadPinnedBranches = (main: string): string[] => stringList(all(PINNED_BRANCHES_KEY)[main]);

export function savePinnedBranches(main: string, names: string[]) {
  put(PINNED_BRANCHES_KEY, main, names.length ? names : null);
}

/** A branch renamed in the app keeps its pin. */
export function renamePinnedBranch(main: string, from: string, to: string) {
  const pins = loadPinnedBranches(main);
  if (pins.includes(from)) savePinnedBranches(main, pins.map((p) => (p === from ? to : p)));
}

/**
 * The gh account the project `main` uses for GitHub, when it isn't gh's active one. Kept here,
 * never in the repo's config; the backend is told as the project opens (lib/github/account).
 */
export function loadGitHubAccount(main: string): string | null {
  const login = all(GITHUB_ACCOUNTS_KEY)[main];
  return typeof login === "string" ? login : null;
}

/** null goes back to gh's active account. */
export function saveGitHubAccount(main: string, login: string | null) {
  put(GITHUB_ACCOUNTS_KEY, main, login);
}

/** The project's own subfolder of the worktree folder set in Settings, or null while that's off. */
export function sharedWorktreeDir(main: string): string | null {
  const root = getSettings().worktreeRoot;
  return root ? joinPath(root, folderName(main)) : null;
}

/** Where `main`'s next worktree goes; null is `<project>.worktrees` beside it. */
export const worktreeDir = (main: string) => loadWorktreeDir(main) ?? sharedWorktreeDir(main);
