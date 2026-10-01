import type { Commit, FileChange, Issue, Pull } from "../api";

/**
 * A PR's diff range, as computed locally (merge base → head), or a branch's in a comparison (no
 * `number`, its name as `label`). Some of a PR's commits have `number` and `label` (their short ids).
 */
interface PullRange {
  number?: number;
  /** The PR's page, for its line comments; none for some of its commits, whose lines aren't the head's. */
  pullUrl?: string;
  label?: string;
  base: string;
  head: string;
}

export type Selection =
  | { kind: "unstaged" | "staged" | "conflict"; file: FileChange }
  // `url`: the commit's GitHub page, when it's there (History knows; a fork's original has it too).
  | { kind: "commit"; commit: Commit; file: FileChange; url?: string }
  | { kind: "file"; path: string }
  | { kind: "pull"; pull: Pull }
  | { kind: "issue"; issue: Issue }
  | { kind: "pr-file"; range: PullRange; file: FileChange }
  // A branch under review: `base` is the merge base (a commit id), `label` the branch it was compared with.
  | { kind: "branch"; base: string; label: string; file: FileChange }
  // Every file of a Changes list in one scroll: uncommitted, staged, or the branch under review.
  | { kind: "changes"; list: ChangeList };

/** The lists Changes shows, which open whole as one stacked diff. */
export type ChangeList = "unstaged" | "staged" | "branch";

const LIST_TITLES: Record<ChangeList, string> = { unstaged: "All Changes", staged: "All Staged Changes", branch: "All Branch Changes" };

/** File path for file-like tabs; for a PR or issue overview, a label. */
export function selectionPath(s: Selection) {
  if (s.kind === "file") return s.path;
  if (s.kind === "pull") return `#${s.pull.number} ${s.pull.title}`;
  if (s.kind === "issue") return `#${s.issue.number} ${s.issue.title}`;
  if (s.kind === "changes") return LIST_TITLES[s.list];
  return s.file.path;
}

/** The tab's file is in the working tree: not a commit's or a pull request's version, nor deleted. */
export function onDisk(s: Selection) {
  if (s.kind === "file") return true;
  return (s.kind === "unstaged" || s.kind === "staged" || s.kind === "conflict" || s.kind === "branch") && s.file.status !== "D";
}

/** Identity of what a tab shows; also used to match list rows to the open tab. */
export function selectionKey(s: Selection) {
  // By url: a fork's #3 and its original's #3 are different threads.
  if (s.kind === "pull") return `pull:${s.pull.url}`;
  if (s.kind === "issue") return `issue:${s.issue.url}`;
  // A PR file by its commits too: a fork's #3 and its original's #3 differ, and so do the PR and one of its commits.
  const scope = s.kind === "commit" ? s.commit.sha : s.kind === "pr-file" ? `${s.range.number ?? ""}@${s.range.base}..${s.range.head}` : s.kind === "branch" ? s.base : "";
  return `${s.kind}:${scope}:${selectionPath(s)}`;
}

/**
 * The row now in the place of `from`, which left a list whose keys were `before`, in order: the next
 * one still there (`after`), else the one before it. None when the list has nothing left.
 */
export function rowInPlace(before: string[], after: { has: (key: string) => boolean }, from: string): string | undefined {
  const at = before.indexOf(from);
  if (at < 0) return undefined;
  return before.slice(at + 1).find((k) => after.has(k)) ?? before.slice(0, at).reverse().find((k) => after.has(k));
}
