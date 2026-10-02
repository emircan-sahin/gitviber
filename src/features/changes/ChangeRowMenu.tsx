import { Archive, ArrowLeftToLine, ArrowRightToLine, Check, Copy, Diff, EyeOff, File, Files, FolderSearch, GitCompareArrows, GitMerge, History, ListTree, Minus, Plus, SquareCheck, Undo2 } from "lucide-react";
import { ContextMenuContent, ContextMenuItem, ContextMenuSeparator } from "@/components/ui/context-menu";
import type { FileChange } from "@/lib/api";
import { IS_MAC, REVEAL_LABEL } from "@/lib/platform";
import type { Selection } from "@/lib/repo/selection";
import { copyFiles, copyLabel, copyText } from "@/lib/app/clipboard";
import { revealPath } from "@/lib/app/openIn";
import { OpenInMenuItem } from "@/features/workspace/OpenIn";
import { diffInTool, mergeInTool, mergingInTool, toolCanOpen, toolName, useExternalTools } from "@/lib/git/externalTools";
import { type Change, files, keptByRestore, paths } from "./changeList";

/** The row's right-click menu, modeled on VS Code's Source Control view. `rows`: what its git actions cover. */
export function ChangeRowMenu({
  sel,
  rows,
  root,
  canStash,
  viewed,
  setViewed,
  onOpen,
  onShowHistory,
  onRevealInExplorer,
  stage,
  unstage,
  markResolved,
  discard,
  ignore,
  resolve,
  stash,
}: {
  sel: Change;
  rows: Change[];
  root: string;
  canStash: boolean;
  viewed: (s: Selection) => boolean;
  setViewed: (s: Selection[], on: boolean) => void;
  onOpen: (s: Selection, pin?: boolean) => void;
  onShowHistory: (path: string) => void;
  onRevealInExplorer: (path: string) => void;
  stage: (rows: Change[]) => void;
  unstage: (rows: Change[]) => void;
  markResolved: (rows: Change[]) => void;
  discard: (list: FileChange[]) => void;
  ignore: (list: FileChange[]) => void;
  resolve: (rows: Change[], side: "ours" | "theirs") => void;
  stash: (paths: string[]) => void;
}) {
  const { file } = sel;
  const onDisk = file.status !== "D";
  const n = rows.length;
  const untracked = rows.filter((r) => r.file.status === "?").map((r) => r.file);
  const isViewed = viewed(sel);
  const list = sel.kind === "conflict" ? null : sel.kind;
  // A deleted file has nothing on disk to copy (its old version copies from the diff view), and a
  // submodule is a folder, not a file.
  const onDiskPaths = [...new Set(rows.filter((r) => r.file.status !== "D" && !r.file.nested).map((r) => r.file.path))];
  // One file at a time, as git opens it. mergetool merges text both sides changed; a deleted side
  // asks on the terminal. difftool has nothing for an untracked file or a submodule.
  const tools = useExternalTools(root);
  const one = n === 1 && toolCanOpen(file.path);
  const mergeTool = one && sel.kind === "conflict" && (file.conflict === "UU" || file.conflict === "AA") ? tools.merge : null;
  const diffTool = one && sel.kind !== "conflict" && file.status !== "?" && !file.nested ? tools.diff : null;
  return (
    <ContextMenuContent>
      <ContextMenuItem onSelect={() => onOpen(sel, true)}>
        {sel.kind === "conflict" ? <GitMerge /> : <Diff />} {sel.kind === "conflict" ? "Open Conflict" : "Open Changes"}
      </ContextMenuItem>
      <ContextMenuItem disabled={!onDisk} onSelect={() => onOpen({ kind: "file", path: file.path }, true)}>
        <File /> Open File
      </ContextMenuItem>
      {list && (
        <ContextMenuItem onSelect={() => onOpen({ kind: "changes", list }, true)}>
          <Files /> {list === "staged" ? "Open All Staged Changes" : "Open All Changes"}
        </ContextMenuItem>
      )}
      <ContextMenuItem disabled={file.status === "?"} onSelect={() => onShowHistory(file.path)}>
        <History /> Show History
      </ContextMenuItem>
      {mergeTool && (
        <ContextMenuItem disabled={mergingInTool(file.path)} onSelect={() => void mergeInTool(mergeTool, file.path)}>
          <GitMerge /> Open in {toolName(mergeTool)}
        </ContextMenuItem>
      )}
      {diffTool && (
        <ContextMenuItem onSelect={() => void diffInTool(diffTool, file.path, sel.kind === "staged")}>
          <GitCompareArrows /> Open in {toolName(diffTool)}
        </ContextMenuItem>
      )}
      <ContextMenuSeparator />
      {sel.kind === "unstaged" && (
        <>
          <ContextMenuItem onSelect={() => stage(rows)}>
            <Plus /> {n > 1 ? `Stage ${n} Files` : "Stage Changes"}
          </ContextMenuItem>
          <ContextMenuItem disabled={rows.every((r) => keptByRestore(r.file))} onSelect={() => discard(rows.map((r) => r.file))}>
            <Undo2 /> {n > 1 ? `Discard ${n} Files…` : "Discard Changes"}
          </ContextMenuItem>
          {untracked.length > 0 && (
            <ContextMenuItem onSelect={() => ignore(untracked)}>
              <EyeOff /> {n > 1 ? `Add ${untracked.length} to .gitignore` : "Add to .gitignore"}
            </ContextMenuItem>
          )}
        </>
      )}
      {sel.kind === "staged" && (
        <ContextMenuItem onSelect={() => unstage(rows)}>
          <Minus /> {n > 1 ? `Unstage ${n} Files` : "Unstage Changes"}
        </ContextMenuItem>
      )}
      {(sel.kind === "unstaged" || sel.kind === "staged") && canStash && (
        <ContextMenuItem onSelect={() => stash(rows.filter((r) => !r.file.nested).map((r) => r.file.path))}>
          <Archive /> {n > 1 ? `Stash ${n} Files…` : "Stash Changes…"}
        </ContextMenuItem>
      )}
      {sel.kind === "conflict" && (
        <>
          <ContextMenuItem onSelect={() => markResolved(rows)}>
            <Check /> {n > 1 ? `Mark ${n} as Resolved` : "Mark as Resolved"}
          </ContextMenuItem>
          <ContextMenuItem onSelect={() => resolve(rows, "ours")}>
            <ArrowLeftToLine /> {n > 1 ? `Take Current Version of ${n} Files` : "Take Current Version"}
          </ContextMenuItem>
          <ContextMenuItem onSelect={() => resolve(rows, "theirs")}>
            <ArrowRightToLine /> {n > 1 ? `Take Incoming Version of ${n} Files` : "Take Incoming Version"}
          </ContextMenuItem>
        </>
      )}
      {sel.kind === "unstaged" && (
        <ContextMenuItem onSelect={() => setViewed(rows, !isViewed)}>
          <SquareCheck /> {`Mark ${n > 1 ? `${n} ` : ""}as ${isViewed ? "Not Viewed" : "Viewed"}`}
        </ContextMenuItem>
      )}
      <ContextMenuSeparator />
      <ContextMenuItem disabled={!onDisk} keepFocus onSelect={() => onRevealInExplorer(file.path)}>
        <ListTree /> Reveal in Explorer View
      </ContextMenuItem>
      <ContextMenuItem disabled={!onDisk} onSelect={() => revealPath(file.path)}>
        <FolderSearch /> {REVEAL_LABEL}
      </ContextMenuItem>
      <OpenInMenuItem path={file.path} disabled={!onDisk} />
      <ContextMenuSeparator />
      {IS_MAC && onDiskPaths.length > 0 && (
        <ContextMenuItem onSelect={() => copyFiles(onDiskPaths.map((p) => `${root}/${p}`))}>
          <Copy /> {copyLabel(onDiskPaths)}
        </ContextMenuItem>
      )}
      <ContextMenuItem onSelect={() => copyText(paths(rows).map((p) => `${root}/${p}`).join("\n"), n > 1 ? `${n} paths copied` : "Path copied")}>
        <Copy /> {n > 1 ? "Copy Paths" : "Copy Path"}
      </ContextMenuItem>
      <ContextMenuItem onSelect={() => copyText(paths(rows).join("\n"), n > 1 ? `${n} relative paths copied` : "Relative path copied")}>
        <Copy /> {n > 1 ? "Copy Relative Paths" : "Copy Relative Path"}
      </ContextMenuItem>
    </ContextMenuContent>
  );
}

/** A tree folder's right-click menu: its git actions cover `rows`, every file under it. */
export function FolderRowMenu({
  kind,
  path,
  rows,
  root,
  canStash,
  viewed,
  setViewed,
  onRevealInExplorer,
  stage,
  unstage,
  markResolved,
  discard,
  resolve,
  stash,
}: {
  kind: Change["kind"];
  path: string;
  rows: Change[];
  root: string;
  canStash: boolean;
  viewed: (s: Selection) => boolean;
  setViewed: (s: Selection[], on: boolean) => void;
  onRevealInExplorer: (path: string) => void;
  stage: (rows: Change[]) => void;
  unstage: (rows: Change[]) => void;
  markResolved: (rows: Change[]) => void;
  discard: (list: FileChange[]) => void;
  resolve: (rows: Change[], side: "ours" | "theirs") => void;
  stash: (paths: string[]) => void;
}) {
  const n = rows.length;
  const allViewed = rows.every(viewed);
  return (
    <ContextMenuContent>
      {kind === "unstaged" && (
        <>
          <ContextMenuItem disabled={!n} onSelect={() => stage(rows)}>
            <Plus /> Stage {files(n)}
          </ContextMenuItem>
          <ContextMenuItem disabled={rows.every((r) => keptByRestore(r.file))} onSelect={() => discard(rows.map((r) => r.file))}>
            <Undo2 /> Discard {files(n)}…
          </ContextMenuItem>
        </>
      )}
      {kind === "staged" && (
        <ContextMenuItem onSelect={() => unstage(rows)}>
          <Minus /> Unstage {files(n)}
        </ContextMenuItem>
      )}
      {kind !== "conflict" && canStash && (
        <ContextMenuItem disabled={!n} onSelect={() => stash(paths(rows))}>
          <Archive /> Stash {files(n)}…
        </ContextMenuItem>
      )}
      {kind === "conflict" && (
        <>
          <ContextMenuItem onSelect={() => markResolved(rows)}>
            <Check /> Mark {files(n)} as Resolved
          </ContextMenuItem>
          <ContextMenuItem onSelect={() => resolve(rows, "ours")}>
            <ArrowLeftToLine /> Take Current Version of {files(n)}
          </ContextMenuItem>
          <ContextMenuItem onSelect={() => resolve(rows, "theirs")}>
            <ArrowRightToLine /> Take Incoming Version of {files(n)}
          </ContextMenuItem>
        </>
      )}
      {kind === "unstaged" && n > 0 && (
        <ContextMenuItem onSelect={() => setViewed(rows, !allViewed)}>
          <SquareCheck /> Mark {files(n)} as {allViewed ? "Not Viewed" : "Viewed"}
        </ContextMenuItem>
      )}
      <ContextMenuSeparator />
      <ContextMenuItem keepFocus onSelect={() => onRevealInExplorer(path)}>
        <ListTree /> Reveal in Explorer View
      </ContextMenuItem>
      <ContextMenuItem onSelect={() => revealPath(path)}>
        <FolderSearch /> {REVEAL_LABEL}
      </ContextMenuItem>
      <ContextMenuSeparator />
      <ContextMenuItem onSelect={() => copyText(`${root}/${path}`, "Path copied")}>
        <Copy /> Copy Path
      </ContextMenuItem>
      <ContextMenuItem onSelect={() => copyText(path, "Relative path copied")}>
        <Copy /> Copy Relative Path
      </ContextMenuItem>
    </ContextMenuContent>
  );
}
