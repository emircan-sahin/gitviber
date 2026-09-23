import type { FileChange } from "./api";

// A Windows path (C:\… or \\server\…) may mix both separators; elsewhere "\" can be part of a name.
const separator = (path: string) => (/^([a-z]:[\\/]|\\\\)/i.test(path) ? /[\\/]/ : /\//);

export const folderName = (path: string) => {
  const sep = separator(path);
  return path.replace(new RegExp(`${sep.source}+$`), "").split(sep).pop() ?? path;
};

/** `path` is somewhere inside the folder `dir`. */
export const isInside = (path: string, dir: string) => path.startsWith(dir) && separator(dir).test(path.charAt(dir.length));

/** Where a worktree sits, as short as it can be said: inside the main one, or beside it. */
export function shortPath(path: string, main: string) {
  if (path === main) return ".";
  if (path.startsWith(`${main}/`)) return path.slice(main.length + 1);
  const parent = main.slice(0, main.lastIndexOf("/"));
  if (parent && path.startsWith(`${parent}/`)) return `../${path.slice(parent.length + 1)}`;
  return path;
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
  "It's a separate git repository. git add would record only a pointer to its current commit (an embedded repo), not its files. Commit inside it instead. If it's a worktree that broke when the repo moved, git worktree repair reconnects it.";
