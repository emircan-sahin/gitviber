import { Diff, File, FileDiff, History, RotateCcw } from "lucide-react";
import { ContextMenuContent, ContextMenuItem, ContextMenuSeparator } from "@/components/ui/context-menu";
import { api, type Commit, type FileChange } from "@/lib/api";
import type { Selection } from "@/lib/repo/selection";
import { copyLater } from "@/lib/app/clipboard";
import { rewriteFiles } from "@/lib/repo/undo";
import { basename } from "@/lib/path";

/** Right-click actions on a file of a commit: the working tree takes its version, or its change undone (Fork's Restore and Reverse). */
export function CommitFileMenu({ commit: c, file: f, sel, refresh, onOpen }: { commit: Commit; file: FileChange; sel: Selection; refresh: () => unknown; onOpen: (s: Selection, pin?: boolean) => void }) {
  const name = basename(f.path);
  const paths = f.oldPath ? [f.oldPath, f.path] : [f.path];
  return (
    <ContextMenuContent>
      <ContextMenuItem onSelect={() => onOpen(sel, true)}>
        <Diff /> Open Changes
      </ContextMenuItem>
      <ContextMenuItem onSelect={() => onOpen({ kind: "file", path: f.path }, true)}>
        <File /> Open File
      </ContextMenuItem>
      <ContextMenuSeparator />
      {/* Deleted there: nothing to take. */}
      <ContextMenuItem disabled={f.status === "D"} onSelect={() => void rewriteFiles(`Restored ${name} from ${c.shortSha}`, "Restore failed", () => api.restoreFile(c.sha, f.path), refresh)}>
        <History /> Restore This Version
      </ContextMenuItem>
      <ContextMenuItem onSelect={() => void rewriteFiles(`Reverted ${c.shortSha} in ${name}`, "Revert failed", () => api.revertFile(c.sha, f.path, f.oldPath), refresh)}>
        <RotateCcw /> Revert This File's Change
      </ContextMenuItem>
      <ContextMenuSeparator />
      <ContextMenuItem onSelect={() => copyLater(() => api.changesPatch("commit", paths, c.sha), "Patch copied")}>
        <FileDiff /> Copy as Patch
      </ContextMenuItem>
    </ContextMenuContent>
  );
}
