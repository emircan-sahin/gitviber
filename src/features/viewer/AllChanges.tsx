// A whole Changes list as one stacked diff (StackedFiles). Also a commit's files, or a range's (a
// comparison, a PR): they never change, so they're read once.
import { ChevronsDownUp, ChevronsUpDown, Files } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef } from "react";
import { Button } from "@/components/ui/button";
import { Tip } from "@/components/ui/tooltip";
import { LineCounts } from "@/components/StatusBadge";
import type { FileChange, RepoStatus } from "@/lib/api";
import { codeWantsFocus } from "@/lib/ui/panels";
import { type ChangesSelection, type Selection, selectionKey, selectionPath } from "@/lib/repo/selection";
import { cn } from "@/lib/utils";
import { sumLines } from "@/features/changes/changeList";
import type { BranchChange } from "@/features/changes/BranchReview";
import { scrolls } from "./stackedMemo";
import { useFixedFiles } from "./fixedFiles";
import { blocksIn, type ListFile, topBlock, useStackedFiles } from "./StackedFiles";

interface Props {
  changes: ChangesSelection;
  status: RepoStatus | null;
  /** The branch review's files as last loaded; null while there's none. */
  branchRows: BranchChange[] | null;
  revision: number;
  viewed: (sel: Selection) => boolean;
  toggleViewed: (sel: Selection) => void;
  onOpen: (s: Selection) => void;
}

function filesOf(changes: ChangesSelection, status: RepoStatus | null, branchRows: BranchChange[] | null, fixed: FileChange[] | null): ListFile[] | null {
  const { list } = changes;
  if (list === "commit") return fixed && fixed.map((file) => ({ kind: "commit", commit: changes.commit, file, url: changes.url }));
  if (list === "range") return fixed && fixed.map((file) => ({ kind: "pr-file", range: changes.range, file }));
  if (list === "branch") return branchRows;
  if (!status) return null;
  // Nested repos have no diff, as in the list.
  return list === "staged" ? status.staged.map((file) => ({ kind: "staged", file })) : status.unstaged.filter((f) => !f.nested).map((file) => ({ kind: "unstaged", file }));
}

export function AllChanges({ changes, status, branchRows, revision, viewed, toggleViewed, onOpen }: Props) {
  const { list } = changes;
  const fixed = useFixedFiles(changes);
  const files = useMemo(() => filesOf(changes, status, branchRows, fixed.files), [changes, status, branchRows, fixed.files]);
  const root = status?.root ?? "";
  // A PR's or comparison's files take no notes, as in their own tab; a commit's are marked as on it.
  const notes = list === "range" ? null : { at: list === "commit" ? `commit ${changes.commit.shortSha}` : undefined };
  const { scroller, block, setAll } = useStackedFiles({ root, status, revision, viewed, toggleViewed, notes: files?.length ? notes : null, onOpen });

  // Back at the file that was at the top, as far into it, once the files are there; each keeps its
  // height from before, so it's the same place.
  const scrollKey = `${root}\0${selectionKey(changes)}`;
  const restored = useRef(false);
  useLayoutEffect(() => {
    const el = scroller.current;
    if (restored.current || !el || !files?.length) return;
    restored.current = true;
    const at = scrolls.get(scrollKey);
    const block = at && blocksIn(el).find((b) => b.dataset.file === at.file);
    if (block) el.scrollTop = block.offsetTop + at.offset;
  });
  useLayoutEffect(() => {
    const el = scroller.current;
    return () => {
      const block = blocksIn(el)[topBlock(el)];
      if (el && block && restored.current) scrolls.set(scrollKey, { file: block.dataset.file!, offset: el.scrollTop - block.offsetTop });
    };
  }, [scrollKey, scroller]);

  // Opened from the code view (a tab switch, quick open): it takes the keys, as a file would.
  useEffect(() => {
    if (codeWantsFocus()) scroller.current?.focus();
  }, []);

  const { add, del } = sumLines((files ?? []).map((f) => f.file));
  const reviewed = (files ?? []).filter(viewed).length;
  const empty = !files
    ? (fixed.error ?? (list === "branch" ? "Review the branch from Changes to see its files here." : "Loading…"))
    : files.length
      ? null
      : list === "staged"
        ? "Nothing staged."
        : list === "commit"
          ? "This commit changes no files."
          : "No changes.";

  return (
    <>
      <div className="flex h-9 shrink-0 items-center gap-2 border-b border-border pr-2 pl-3 text-[12px]">
        <Files className="size-4 shrink-0 text-subtle" />
        <span className={cn("font-medium", list === "commit" ? "shrink-0" : "min-w-0 truncate")}>{selectionPath(changes)}</span>
        {list === "commit" && <span className="min-w-0 truncate text-muted-foreground">{changes.commit.subject}</span>}
        {!!files?.length && (
          <>
            <span className="shrink-0 text-muted-foreground">{files.length === 1 ? "1 file" : `${files.length} files`}</span>
            <LineCounts file={{ additions: add, deletions: del }} />
            {list !== "staged" && (
              <span className="shrink-0 text-muted-foreground">
                <span className={cn("font-semibold", reviewed === files.length ? "text-added" : "text-foreground")}>{reviewed}</span>/{files.length} reviewed
              </span>
            )}
            <div className="ml-auto flex shrink-0 items-center gap-1">
              <Tip label="Expand all files">
                <Button variant="ghost" size="icon-sm" aria-label="Expand all files" onClick={() => setAll(files, true)}>
                  <ChevronsUpDown />
                </Button>
              </Tip>
              <Tip label="Collapse all files">
                <Button variant="ghost" size="icon-sm" aria-label="Collapse all files" onClick={() => setAll(files, false)}>
                  <ChevronsDownUp />
                </Button>
              </Tip>
            </div>
          </>
        )}
      </div>
      <div ref={scroller} data-code-scroll tabIndex={0} className="relative min-h-0 flex-1 overflow-y-auto outline-none">
        {empty ? (
          <div className="flex h-full items-center justify-center p-6 text-[12.5px] text-muted-foreground">{empty}</div>
        ) : (
          files!.map((f) => block(f))
        )}
      </div>
    </>
  );
}
