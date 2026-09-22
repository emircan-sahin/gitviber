import type { Commit, FileChange, Issue, Pull } from "./api";

/** A PR's diff range, as computed locally (merge base → head). */
export interface PullRange {
  number: number;
  base: string;
  head: string;
}

export type Selection =
  | { kind: "unstaged" | "staged" | "conflict"; file: FileChange }
  | { kind: "commit"; commit: Commit; file: FileChange }
  | { kind: "file"; path: string }
  | { kind: "pull"; pull: Pull }
  | { kind: "issue"; issue: Issue }
  | { kind: "pr-file"; range: PullRange; file: FileChange };

/** File path for file-like tabs; for a PR or issue overview, a label. */
export function selectionPath(s: Selection) {
  if (s.kind === "file") return s.path;
  if (s.kind === "pull") return `#${s.pull.number} ${s.pull.title}`;
  if (s.kind === "issue") return `#${s.issue.number} ${s.issue.title}`;
  return s.file.path;
}

/** Identity of what a tab shows; also used to match list rows to the open tab. */
export function selectionKey(s: Selection) {
  if (s.kind === "pull") return `pull:${s.pull.number}`;
  if (s.kind === "issue") return `issue:${s.issue.number}`;
  const scope = s.kind === "commit" ? s.commit.sha : s.kind === "pr-file" ? s.range.number : "";
  return `${s.kind}:${scope}:${selectionPath(s)}`;
}
