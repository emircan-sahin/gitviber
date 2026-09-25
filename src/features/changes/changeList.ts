import type { FileChange, RepoStatus } from "@/lib/api";
import { gitFailed } from "@/hooks/useGitAction";
import type { Selection } from "@/lib/repo/selection";
import type { RepoData } from "@/lib/repo/useRepo";

export type Change = Selection & { kind: "conflict" | "staged" | "unstaged" };

/** Every reviewable change in display order; J/K walk this list. Nested repos have no diff to review. */
export function changeList(status: RepoStatus): Change[] {
  return [
    ...status.conflicted.map((file) => ({ kind: "conflict" as const, file })),
    ...status.staged.map((file) => ({ kind: "staged" as const, file })),
    ...status.unstaged.filter((f) => !f.nested).map((file) => ({ kind: "unstaged" as const, file })),
  ];
}

const LISTS: Record<Change["kind"], (s: RepoStatus) => FileChange[]> = {
  conflict: (s) => s.conflicted,
  staged: (s) => s.staged,
  unstaged: (s) => s.unstaged,
};
export const isChange = (sel: Selection): sel is Change => sel.kind in LISTS;

export function currentFile(status: RepoStatus | null, sel: Selection): FileChange | undefined {
  if (!status || !isChange(sel)) return undefined;
  return LISTS[sel.kind](status).find((f) => f.path === sel.file.path);
}

/** Where a change tab's file lives now: same list first, else wherever it moved (resolved → staged…). */
export function relocate(status: RepoStatus, sel: Change): Selection | null {
  for (const kind of [sel.kind, "conflict", "staged", "unstaged"] as const) {
    const file = LISTS[kind](status).find((f) => f.path === sel.file.path);
    if (file) return { kind, file };
  }
  return null;
}

/** `status` with only the files `keep` accepts. */
export const filtered = (status: RepoStatus, keep: (f: FileChange) => boolean): RepoStatus => ({
  ...status,
  conflicted: status.conflicted.filter(keep),
  staged: status.staged.filter(keep),
  unstaged: status.unstaged.filter(keep),
});

export function sumLines(files: Pick<FileChange, "additions" | "deletions">[]) {
  return {
    add: files.reduce((n, f) => n + (f.additions ?? 0), 0),
    del: files.reduce((n, f) => n + (f.deletions ?? 0), 0),
  };
}

export function changeTotals(repo: RepoData) {
  const files = repo.status ? changeList(repo.status).map((c) => c.file) : [];
  return { files: files.length, ...sumLines(files) };
}

export async function attempt(title: string, fn: () => Promise<unknown>) {
  try {
    await fn();
    return true;
  } catch (e) {
    gitFailed(title, e);
    return false;
  }
}

export const leftOut = (n: number) => `Left out ${n} nested ${n === 1 ? "repository" : "repositories"}`;
export const paths = (rows: Change[]) => rows.map((r) => r.file.path);
export const files = (n: number) => `${n} ${n === 1 ? "file" : "files"}`;
