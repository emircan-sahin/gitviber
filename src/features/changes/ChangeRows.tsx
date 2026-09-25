import { Check, ChevronDown, FolderGit2, GitMerge, Plus } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { ContextMenu, ContextMenuTrigger } from "@/components/ui/context-menu";
import { Tip } from "@/components/ui/tooltip";
import type { FileChange } from "@/lib/api";
import { type Selection, selectionKey } from "@/lib/repo/selection";
import { toast } from "@/lib/app/toast";
import { cn } from "@/lib/utils";
import { NESTED_EXPLAINED } from "@/lib/git/worktrees";
import { FileIcon } from "@/components/FileIcon";
import { LineCounts, PathLabel, StatusLetter } from "@/components/StatusBadge";
import type { BranchChange } from "./BranchReview";
import type { Change } from "./changeList";
import { RowAction } from "@/components/RowAction";

/** `pinned`: the actions stay visible instead of showing on hover (they act on a selection the user just made). */
export function Section({ title, count, tone, action, pinned, children }: { title: string; count: number; tone?: string; action?: React.ReactNode; pinned?: boolean; children: React.ReactNode }) {
  const [open, setOpen] = useState(true);
  return (
    <div>
      <div className="group sticky top-0 z-10 flex h-7 items-center gap-1 border-b border-border bg-panel pr-1.5 pl-2">
        <button className="flex items-center gap-1 text-[10.5px] font-semibold tracking-[0.08em] text-subtle uppercase hover:text-foreground focus-visible:text-foreground" onClick={() => setOpen(!open)}>
          <ChevronDown className={cn("size-3 transition-transform", !open && "-rotate-90")} />
          <span className={tone}>{title}</span>
          <span className="ml-1 font-mono tracking-normal text-muted-foreground">{count}</span>
        </button>
        <div className={cn("ml-auto flex gap-0.5", !pinned && "opacity-0 group-focus-within:opacity-100 group-hover:opacity-100")}>{action}</div>
      </div>
      {open && <div className="py-0.5">{children}</div>}
    </div>
  );
}

/** How far the review is: files, lines, and how many are marked viewed, over a progress bar. */
export function ReviewSummary({ files, add, del, reviewed }: { files: number; add: number; del: number; reviewed: number }) {
  return (
    <div className="shrink-0 border-b border-border px-3 py-2">
      <div className="flex items-center gap-2 text-[11.5px]">
        <span className="text-muted-foreground">
          <span className="font-semibold text-foreground">{files}</span> {files === 1 ? "file" : "files"}
        </span>
        <span className="font-mono text-[11px]">
          <span className="text-added">+{add}</span> <span className="text-removed">-{del}</span>
        </span>
        <span className="ml-auto text-muted-foreground">
          <span className={cn("font-semibold", reviewed === files ? "text-added" : "text-foreground")}>{reviewed}</span>/{files} reviewed
        </span>
      </div>
      <div className="mt-1.5 h-[3px] overflow-hidden bg-border">
        <div className="h-full bg-added transition-[width] duration-300" style={{ width: `${(reviewed / files) * 100}%` }} />
      </div>
    </div>
  );
}

export function SectionBtn({ onClick, children }: { onClick: () => void; children: React.ReactNode }) {
  return (
    <button onClick={onClick} className="h-5 rounded-sm px-1.5 text-[11px] text-muted-foreground outline-none hover:bg-active focus-visible:bg-active hover:text-foreground focus-visible:text-foreground focus-visible:ring-1 focus-visible:ring-ring">
      {children}
    </button>
  );
}

export function Row({
  sel,
  active,
  selected,
  dim,
  tabStop,
  viewed,
  checkLabel,
  onClick,
  onOpen,
  onHover,
  onToggleViewed,
  menu,
  children,
}: {
  sel: Change | BranchChange;
  active: boolean;
  selected: boolean;
  /** Selected while focus is elsewhere: shown fainter, like VS Code's inactive selection. The open row keeps its color. */
  dim: boolean;
  tabStop: boolean;
  viewed: boolean;
  checkLabel: string;
  onClick: (e: React.MouseEvent) => void;
  onOpen: (s: Selection, pin?: boolean) => void;
  onHover: (s: Selection) => void;
  onToggleViewed: () => void;
  /** Built only once the menu is first opened: thousands of rows each building theirs made the list slow. */
  menu: () => React.ReactNode;
  children?: React.ReactNode;
}) {
  const file = sel.file;
  const ref = useRef<HTMLDivElement>(null);
  const [menuOpened, setMenuOpened] = useState(false);
  // J/K can move the selection off-screen; follow it. ↑/↓ from a row also moves focus to it.
  // (scroll-mt-7 keeps a row scrolled to the top clear of its section's sticky h-7 header.)
  useEffect(() => {
    if (!active) return;
    ref.current?.scrollIntoView({ block: "nearest" });
    if (document.activeElement instanceof HTMLElement && document.activeElement.dataset.row !== undefined) ref.current?.focus();
  }, [active]);
  return (
    <ContextMenu onOpenChange={(open) => open && setMenuOpened(true)}>
      <ContextMenuTrigger asChild>
        <div
          ref={ref}
          role="button"
          tabIndex={tabStop ? 0 : -1}
          data-row={selectionKey(sel)}
          aria-current={active || undefined}
          onClick={onClick}
          onDoubleClick={() => onOpen(sel, true)}
          onMouseEnter={() => onHover(sel)}
          className={cn(
            "group/row relative flex h-[26px] scroll-mt-7 cursor-pointer items-center gap-2 pr-2 pl-2 text-[12px] outline-none focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset",
            selected ? (dim ? "bg-active" : "bg-primary/15") : "hover:bg-hover focus:bg-hover data-[state=open]:bg-hover",
          )}
        >
          {active && <span className="absolute inset-y-0 left-0 w-0.5 bg-primary" />}
          {sel.kind === "conflict" ? (
            <GitMerge className="size-3.5 shrink-0 text-conflict" />
          ) : (
          <Tip label={checkLabel}>
            <button
              role="checkbox"
              tabIndex={tabStop ? undefined : -1}
              aria-checked={viewed}
              aria-label={sel.kind === "staged" ? "Staged" : "Viewed"}
              onClick={(e) => {
                e.stopPropagation();
                onToggleViewed();
              }}
              className={cn(
                "flex size-3.5 shrink-0 items-center justify-center rounded-[3px] border outline-none focus-visible:ring-1 focus-visible:ring-ring",
                viewed ? "border-added-fill bg-added-fill text-on-status" : "border-border-strong hover:border-muted-foreground",
              )}
            >
              {viewed && <Check className="size-2.5" strokeWidth={3} />}
            </button>
          </Tip>
          )}
          <FileIcon path={file.path} />
          <PathLabel path={file.path} className={cn("flex-1", viewed && "opacity-45")} />
          {/* Shown on the active row too, so Tab can reach them without a mouse. */}
          <LineCounts file={file} className={active ? "hidden" : "group-focus-within/row:hidden group-hover/row:hidden"} />
          <div className={cn("items-center", active ? "flex" : "hidden group-focus-within/row:flex group-hover/row:flex")} onClick={(e) => e.stopPropagation()}>
            {children}
          </div>
          <StatusLetter status={file.status} />
        </div>
      </ContextMenuTrigger>
      {menuOpened && menu()}
    </ContextMenu>
  );
}

/**
 * An untracked folder that is another repository (not one of ours: worktrees stay out of
 * status). Git lists it, so we do too, but it has no diff here and can't be staged.
 */
export function NestedRow({ file }: { file: FileChange }) {
  return (
    <div className="group/row relative flex h-[26px] cursor-default items-center gap-2 pr-2 pl-2 text-[12px]">
      <span className="size-3.5 shrink-0" />
      <FolderGit2 className="size-4 shrink-0 text-subtle" />
      <PathLabel path={file.path.replace(/\/$/, "")} className="flex-1" />
      <span className="max-w-32 shrink-0 truncate rounded-sm bg-elevated px-1 font-mono text-[10.5px] leading-4 text-muted-foreground group-focus-within/row:hidden group-hover/row:hidden">
        nested repo
      </span>
      <div className="hidden items-center group-focus-within/row:flex group-hover/row:flex">
        <RowAction label="Can't stage a separate git repository" onClick={() => toast("info", "Not stageable", NESTED_EXPLAINED)}>
          <Plus className="opacity-40" />
        </RowAction>
      </div>
      <StatusLetter status={file.status} />
    </div>
  );
}

export function AllCaughtUp() {
  return (
    <div className="flex flex-col items-center gap-1.5 px-6 pt-20 text-center">
      <Check className="mb-1 size-6 text-border-strong" />
      <div className="text-[12.5px] text-muted-foreground">Working tree clean</div>
      <div className="text-[11.5px] leading-relaxed text-subtle">Anything your agent writes shows up here instantly.</div>
    </div>
  );
}
