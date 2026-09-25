import type { Commit, FileChange, Issue, Pull } from "../api";

/** A PR's diff range, as computed locally (merge base → head), or a branch's in a comparison (no `number`, its name as `label`). */
interface PullRange {
  number?: number;
  /** The PR's page, for its line comments. */
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
  | { kind: "branch"; base: string; label: string; file: FileChange };

/** File path for file-like tabs; for a PR or issue overview, a label. */
export function selectionPath(s: Selection) {
  if (s.kind === "file") return s.path;
  if (s.kind === "pull") return `#${s.pull.number} ${s.pull.title}`;
  if (s.kind === "issue") return `#${s.issue.number} ${s.issue.title}`;
  return s.file.path;
}

/** Identity of what a tab shows; also used to match list rows to the open tab. */
export function selectionKey(s: Selection) {
  // By url: a fork's #3 and its original's #3 are different threads.
  if (s.kind === "pull") return `pull:${s.pull.url}`;
  if (s.kind === "issue") return `issue:${s.issue.url}`;
  // A PR file by its head commit too: a fork's #3 and its original's #3 differ.
  const scope = s.kind === "commit" ? s.commit.sha : s.kind === "pr-file" ? `${s.range.number ?? `${s.range.base}..`}@${s.range.head}` : s.kind === "branch" ? s.base : "";
  return `${s.kind}:${scope}:${selectionPath(s)}`;
}
