import type { Worktree } from "../../lib/api/types.ts";
import { folderName } from "../../lib/path.ts";

/** The open project is entered through a linked worktree: its row then opens the main folder, as a row of another project would. */
export interface Away {
  folder: string;
  mainBranch: string | null;
}

/** Set while the window is in `root` and not in the project's main folder. */
export function awayOf(root: string, main: string, worktrees: Worktree[]): Away | undefined {
  if (root === main) return undefined;
  return { folder: folderName(root), mainBranch: worktrees.find((w) => w.main)?.branch ?? null };
}
