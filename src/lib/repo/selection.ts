import type { Commit, FileChange, Issue, Pull, Target } from "../api";
import type { DeviceChoice } from "../browser/devices.ts";
import { pageLabel } from "../browser/url.ts";

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

/** A side of a comparison: a full ref (refs/heads/…, refs/remotes/…, refs/tags/…) or a commit id, and what to call it. */
export interface ComparePoint {
  ref: string;
  label: string;
}

/** A stored tab's point, which a build that wrote it may have shaped otherwise: the screen reads both sides' labels. */
export const isComparePoint = (p: unknown): p is ComparePoint => typeof p === "object" && p !== null && typeof (p as ComparePoint).ref === "string" && typeof (p as ComparePoint).label === "string";

export type Selection =
  | { kind: "unstaged" | "staged" | "conflict"; file: FileChange }
  // `url`: the commit's GitHub page, when it's there (History knows; a fork's original has it too).
  | { kind: "commit"; commit: Commit; file: FileChange; url?: string }
  | { kind: "file"; path: string }
  | { kind: "pull"; pull: Pull }
  | { kind: "issue"; issue: Issue }
  | { kind: "pr-file"; range: PullRange; file: FileChange }
  // A branch under review: `base` is the merge base (a commit id), `label` the branch it was compared with.
  // `fixed`: History's comparison of the working tree with one commit, whose base stays as the review moves.
  | { kind: "branch"; base: string; label: string; file: FileChange; fixed?: true }
  // Every file of a Changes list in one scroll: uncommitted, staged, or the branch under review.
  | { kind: "changes"; list: ChangeList }
  // Or of a commit (`url` as for its files), or of a range: a comparison's, a PR's.
  | { kind: "changes"; list: "commit"; commit: Commit; url?: string }
  | { kind: "changes"; list: "range"; range: PullRange }
  // The Compare screen: `head` against `base`, from their merge base (what a PR shows) or as they stand.
  | { kind: "compare"; base: ComparePoint; head: ComparePoint; mergeBase: boolean }
  // Two working-tree files side by side (the explorer's Compare Selected): `file.oldPath` against `file.path`.
  | { kind: "files"; file: FileChange }
  // A file in an Obsidian vault (`vault`: its folder), outside the repo; `path` is vault-relative.
  | { kind: "vault"; vault: string; path: string }
  // An agent's guided review (features/review/GuideView) of a commit, of HEAD's branch since
  // `base` (a full ref, `label` its short name), of a pull request (`target`: the repo it's on), or
  // of the worktree's uncommitted changes.
  | { kind: "guide"; of: "commit"; commit: Commit }
  | { kind: "guide"; of: "branch"; base: string; label: string }
  | { kind: "guide"; of: "pull"; pull: Pull; target: Target }
  | { kind: "guide"; of: "changes" }
  // A web page (features/browser): `id` names the tab's native view, whatever it loads; no `file`,
  // so nothing takes it for a repo file. `title` is the page's last, for a tab not loaded yet;
  // `device`, the one it shows its page as (device mode).
  | { kind: "browser"; id: string; url: string; title?: string; device?: DeviceChoice };

export type GuideSelection = Extract<Selection, { kind: "guide" }>;

/** A whole list of files in one scroll. */
export type ChangesSelection = Extract<Selection, { kind: "changes" }>;

/** The explorer's comparison of two working-tree files, `left` the old side. */
export const filesSelection = (left: string, right: string): Selection => ({
  kind: "files",
  file: { path: right, oldPath: left, status: "M", additions: null, deletions: null, oid: null, indexOid: null, conflict: null, mode: null, submodule: null, nested: null },
});

/** The lists Changes shows, which open whole as one stacked diff. */
export type ChangeList = "unstaged" | "staged" | "branch";

const LIST_TITLES: Record<ChangeList, string> = { unstaged: "All Changes", staged: "All Staged Changes", branch: "All Branch Changes" };

/** The files of a range, and the dots between its ends as git writes them. */
export const rangeLabel = (range: PullRange) => range.label ?? `${range.base.slice(0, 7)}..${range.head.slice(0, 7)}`;

/** A comparison's name: `base...head` from the merge base, `base..head` as they stand. */
export const compareLabel = (s: { base: ComparePoint; head: ComparePoint; mergeBase: boolean }) => `${s.base.label}${s.mergeBase ? "..." : ".."}${s.head.label}`;

/** A PR or comparison range's identity: a fork's #3 and its original's #3 differ, and so do the PR and one of its commits. */
const rangeScope = (range: PullRange) => `${range.number ?? ""}@${range.base}..${range.head}`;

/** File path for file-like tabs; for a PR or issue overview, a label. */
export function selectionPath(s: Selection) {
  if (s.kind === "file" || s.kind === "vault") return s.path;
  if (s.kind === "pull") return `#${s.pull.number} ${s.pull.title}`;
  if (s.kind === "issue") return `#${s.issue.number} ${s.issue.title}`;
  if (s.kind === "changes") return s.list === "commit" ? `Commit ${s.commit.shortSha}` : s.list === "range" ? `All Changes · ${rangeLabel(s.range)}` : LIST_TITLES[s.list];
  if (s.kind === "compare") return "Compare";
  if (s.kind === "guide") return s.of === "commit" ? `Explain ${s.commit.shortSha}` : `Guided Review · ${s.of === "pull" ? `#${s.pull.number}` : s.of === "changes" ? "Uncommitted" : s.label}`;
  if (s.kind === "browser") return pageLabel(s.url);
  return s.file.path;
}

/** The tab's file is in the working tree: not a commit's or a pull request's version, nor deleted. */
export function onDisk(s: Selection) {
  if (s.kind === "file") return true;
  return (s.kind === "unstaged" || s.kind === "staged" || s.kind === "conflict" || s.kind === "branch") && s.file.status !== "D";
}

/**
 * What unsaved edits to the tab's file are kept under (lib/editor/edits): a repo file by its
 * path, a vault's by its tab's key. Null for anything that can't be typed into.
 */
export function editPath(s: Selection) {
  // An unstaged diff's new side is the file on disk, typed into as the file view is.
  if (s.kind === "file" || s.kind === "unstaged") return selectionPath(s);
  return s.kind === "vault" ? selectionKey(s) : null;
}

/** A vault note's path inside its vault, taken out of its edit key (`editPath`). */
export const vaultEditFile = (vault: string, key: string) => key.slice(editPath({ kind: "vault", vault, path: "" })!.length);

/** Identity of what a tab shows; also used to match list rows to the open tab. */
export function selectionKey(s: Selection) {
  // By url: a fork's #3 and its original's #3 are different threads.
  if (s.kind === "pull") return `pull:${s.pull.url}`;
  if (s.kind === "issue") return `issue:${s.issue.url}`;
  if (s.kind === "vault") return `vault:${s.vault}:${s.path}`;
  // The same tab as its page moves on.
  if (s.kind === "browser") return `browser::${s.id}`;
  // A PR file by its commits too, as its range's list is.
  const scope =
    s.kind === "commit"
      ? s.commit.sha
      : s.kind === "pr-file"
        ? rangeScope(s.range)
        : s.kind === "changes"
          ? s.list === "commit"
            ? s.commit.sha
            : s.list === "range"
              ? rangeScope(s.range)
              : ""
          : s.kind === "branch"
            ? s.base
            : s.kind === "files"
              ? (s.file.oldPath ?? "")
              : s.kind === "guide"
                ? s.of === "commit"
                  ? s.commit.sha
                  : s.of === "pull"
                    ? s.pull.url
                    : s.of === "changes"
                      ? "changes"
                      : s.base
                : "";
  return `${s.kind}:${scope}:${selectionPath(s)}`;
}
