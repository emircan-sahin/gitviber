import type { FileChange, Nested } from "./api";

export const folderName = (path: string) => path.replace(/\/+$/, "").split("/").pop() ?? path;

/** Where a worktree sits, as short as it can be said: inside the main one, or beside it. */
export function shortPath(path: string, main: string) {
  if (path === main) return ".";
  if (path.startsWith(`${main}/`)) return path.slice(main.length + 1);
  const parent = main.slice(0, main.lastIndexOf("/"));
  if (parent && path.startsWith(`${parent}/`)) return `../${path.slice(parent.length + 1)}`;
  return path;
}

/** What a nested entry in Changes is: a worktree's branch, or just "nested repo". */
export function nestedLabel(n: Nested) {
  if (!n.worktree) return "nested repo";
  return n.branch ?? "detached";
}

/**
 * Files "stage all" / "commit all" may hand to git. Nested repositories are left out:
 * git would add each as a gitlink (a pointer to its commit), not its files.
 */
export function stageable(files: FileChange[]) {
  const paths = files.filter((f) => !f.nested).map((f) => f.path);
  return { paths, skipped: files.length - paths.length };
}

export const NESTED_EXPLAINED =
  "It's a separate git repository. git add would record only a pointer to its current commit (an embedded repo), not its files. Commit inside it instead.";
