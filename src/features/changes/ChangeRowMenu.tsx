import { Archive, ArrowLeftToLine, ArrowRightToLine, Check, Copy, Diff, EyeOff, File, FolderSearch, GitMerge, History, ListTree, Minus, Plus, SquareCheck, Undo2 } from "lucide-react";
import { useRef } from "react";
import { ContextMenuContent, ContextMenuItem, ContextMenuSeparator } from "@/components/ui/context-menu";
import type { FileChange } from "@/lib/api";
import { IS_MAC, REVEAL_LABEL } from "@/lib/platform";
import type { Selection } from "@/lib/repo/selection";
import { copyFiles, copyLabel, copyText } from "@/lib/app/clipboard";
import { revealPath } from "@/lib/app/openIn";
import { OpenInMenuItem } from "@/features/workspace/OpenIn";
import { type Change, paths } from "./changeList";

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
  discard: (list: FileChange[]) => void;
  ignore: (list: FileChange[]) => void;
  resolve: (rows: Change[], side: "ours" | "theirs") => void;
  stash: (paths: string[]) => void;
}) {
  // Set by "Reveal in Explorer", so the closing menu doesn't pull focus back from the tree.
  const keepFocus = useRef(false);

  const { file } = sel;
  const onDisk = file.status !== "D";
  const n = rows.length;
  const untracked = rows.filter((r) => r.file.status === "?").map((r) => r.file);
  const isViewed = viewed(sel);
  // A deleted file has nothing on disk to copy; its old version copies from the diff view.
  const onDiskPaths = [...new Set(rows.filter((r) => r.file.status !== "D").map((r) => r.file.path))];
  return (
    <ContextMenuContent
      onCloseAutoFocus={(e) => {
        if (keepFocus.current) e.preventDefault();
        keepFocus.current = false;
      }}
    >
      <ContextMenuItem onSelect={() => onOpen(sel, true)}>
        {sel.kind === "conflict" ? <GitMerge /> : <Diff />} {sel.kind === "conflict" ? "Open Conflict" : "Open Changes"}
      </ContextMenuItem>
      <ContextMenuItem disabled={!onDisk} onSelect={() => onOpen({ kind: "file", path: file.path }, true)}>
        <File /> Open File
      </ContextMenuItem>
      <ContextMenuItem disabled={file.status === "?"} onSelect={() => onShowHistory(file.path)}>
        <History /> Show History
      </ContextMenuItem>
      <ContextMenuSeparator />
      {sel.kind === "unstaged" && (
        <>
          <ContextMenuItem onSelect={() => stage(rows)}>
            <Plus /> {n > 1 ? `Stage ${n} Files` : "Stage Changes"}
          </ContextMenuItem>
          <ContextMenuItem onSelect={() => discard(rows.map((r) => r.file))}>
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
          <ContextMenuItem onSelect={() => stage(rows)}>
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
      <ContextMenuItem
        disabled={!onDisk}
        onSelect={() => {
          keepFocus.current = true;
          onRevealInExplorer(file.path);
        }}
      >
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
