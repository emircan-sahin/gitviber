import { Check, ChevronsUpDown, CornerUpLeft, FolderGit2, GitBranch, Lock, SquareTerminal, Trash2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Tip } from "@/components/ui/tooltip";
import { api, type Branch, type Worktree } from "@/lib/api";
import { cn, relativeTime } from "@/lib/utils";
import { folderName, shortPath } from "@/lib/worktrees";
import { RowAction } from "./BranchPicker";

interface Props {
  worktrees: Worktree[];
  /** For each worktree's last commit time. */
  branches: Branch[];
  /** Opens a worktree in this window (the regular open-repo flow). */
  onOpen: (path: string) => void;
  onTerminal: (path: string) => void;
  /** Deletes a linked worktree (asks first). */
  onRemove: (w: Worktree) => void;
}

/**
 * `git worktree list` as a switcher, shown once the repo has more than one worktree.
 * Rows lead with the branch, the name people know a worktree by; the folder comes second.
 * In a linked worktree it names it and offers the way back to the main one.
 */
export function WorktreePicker({ worktrees, branches, onOpen, onTerminal, onRemove }: Props) {
  const [open, setOpen] = useState(false);
  const [list, setList] = useState(worktrees);
  const [index, setIndex] = useState(0);
  // Change counts need a `git status` per worktree: fetched when the menu opens, never before.
  const [counts, setCounts] = useState<Record<string, number>>({});
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => setList(worktrees), [worktrees]);
  // Fresh on open: whether a lock's session is still running is only asked here.
  useEffect(() => {
    if (!open) return;
    let live = true;
    api
      .worktrees()
      .then((fresh) => {
        if (!live) return;
        setList(fresh);
        // A few at a time: a dozen agent worktrees shouldn't mean a dozen `git status` at once.
        const todo = fresh.filter((w) => !w.prunable && !w.bare);
        const next = (): Promise<void> | undefined => {
          const w = todo.shift();
          if (!w || !live) return;
          return api
            .worktreeChanges(w.path)
            .then(
              (n) => void (live && setCounts((c) => ({ ...c, [w.path]: n }))),
              () => {},
            )
            .then(next);
        };
        for (let i = 0; i < 3; i++) next();
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [open]);
  useEffect(() => {
    if (open) setIndex(Math.max(0, list.findIndex((w) => w.current)));
  }, [open]);
  useEffect(() => {
    listRef.current?.querySelector(`[data-option="${index}"]`)?.scrollIntoView({ block: "nearest" });
  }, [index]);

  if (list.length < 2) return null;
  const current = list.find((w) => w.current);
  const main = list.find((w) => w.main && !w.bare);
  const linked = !!current && !current.main;
  const usable = (w: Worktree) => !w.current && !w.prunable && !w.bare;
  const then = (fn: (w: Worktree) => void) => (w: Worktree) => {
    setOpen(false);
    fn(w);
  };
  const pick = then((w) => onOpen(w.path));
  const terminal = then((w) => onTerminal(w.path));
  const remove = then(onRemove);

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") setIndex((i) => Math.min(list.length - 1, i + 1));
    else if (e.key === "ArrowUp") setIndex((i) => Math.max(0, i - 1));
    else if (e.key === "Enter" && list[index] && usable(list[index])) pick(list[index]);
    else return;
    e.preventDefault();
  };

  return (
    <>
      <Popover open={open} onOpenChange={setOpen}>
        <Tip label={linked ? `In worktree ${folderName(current.path)} · switch worktree` : `${list.length} worktrees · switch worktree`}>
          <PopoverTrigger asChild>
            <button
              aria-label={linked ? `Worktree ${folderName(current.path)}, switch worktree` : `Switch worktree (${list.length})`}
              className={cn(
                "flex h-7 max-w-56 min-w-0 shrink-0 items-center gap-1.5 rounded-md px-2 hover:bg-hover data-[state=open]:bg-active",
                linked && "bg-primary/10",
              )}
            >
              <FolderGit2 className={cn("size-3.5 shrink-0", linked ? "text-primary" : "text-subtle")} />
              {linked ? (
                <span className="truncate font-mono text-[12px]">{folderName(current.path)}</span>
              ) : (
                <span className="font-mono text-[11px] text-muted-foreground">{list.length}</span>
              )}
              <ChevronsUpDown className="size-3 shrink-0 text-subtle" />
            </button>
          </PopoverTrigger>
        </Tip>
        <PopoverContent
          align="start"
          className="flex w-96 flex-col overflow-hidden"
          onKeyDown={onKeyDown}
          // Focus the list, not the hot row's first action: that would open its tooltip.
          onOpenAutoFocus={(e) => {
            e.preventDefault();
            listRef.current?.focus();
          }}
        >
          {/* Like a native menu: the highlight leaves with the mouse; ↑↓ bring it back. */}
          <div ref={listRef} tabIndex={-1} onMouseLeave={() => setIndex(-1)} className="max-h-[360px] min-h-0 flex-1 overflow-x-hidden overflow-y-auto p-1 outline-none">
            <div className="px-2 pt-2 pb-1 text-[10.5px] font-semibold tracking-[0.08em] text-subtle uppercase">Worktrees</div>
            {list.map((w, i) => (
              <WorktreeRow
                key={w.path}
                i={i}
                w={w}
                hot={i === index}
                usable={usable(w)}
                main={main?.path ?? w.path}
                time={branches.find((b) => !b.remote && b.name === w.branch)?.timestamp}
                count={counts[w.path]}
                onHover={setIndex}
                onPick={pick}
                onTerminal={terminal}
                onRemove={remove}
              />
            ))}
          </div>
          <div className="shrink-0 border-t border-border px-2.5 py-1.5 text-[10.5px] text-subtle">
            ↑↓ navigate · ↵ open here · hover for actions
            {list.some((w) => w.prunable) && (
              <>
                <br />
                Missing folders stay listed until <span className="font-mono">git worktree prune</span>.
              </>
            )}
          </div>
        </PopoverContent>
      </Popover>
      {linked && main && (
        <Tip label={`Back to the main worktree (${folderName(main.path)})`}>
          <Button variant="ghost" size="icon-sm" aria-label={`Back to the main worktree (${folderName(main.path)})`} onClick={() => onOpen(main.path)}>
            <CornerUpLeft />
          </Button>
        </Tip>
      )}
    </>
  );
}

function WorktreeRow({
  i,
  w,
  hot,
  usable,
  main,
  time,
  count,
  onHover,
  onPick,
  onTerminal,
  onRemove,
}: {
  i: number;
  w: Worktree;
  hot: boolean;
  usable: boolean;
  main: string;
  time: number | undefined;
  count: number | undefined;
  onHover: (i: number) => void;
  onPick: (w: Worktree) => void;
  onTerminal: (w: Worktree) => void;
  onRemove: (w: Worktree) => void;
}) {
  const branch = w.branch ?? (w.bare ? "bare" : `detached @ ${w.head ?? "?"}`);
  const act = (fn: (w: Worktree) => void) => (e: React.MouseEvent) => {
    e.stopPropagation();
    fn(w);
  };
  return (
    <div
      data-option={i}
      role="option"
      aria-selected={hot}
      aria-disabled={!usable}
      onMouseMove={() => onHover(i)}
      onClick={() => usable && onPick(w)}
      className={cn(
        "flex h-9 items-center gap-2 rounded-sm px-2 select-none",
        hot && "bg-primary text-primary-foreground",
        usable ? "cursor-pointer" : "cursor-default",
        (w.prunable || w.bare) && "opacity-50",
      )}
    >
      {w.current ? <Check className="size-3.5 shrink-0" /> : <GitBranch className="size-3.5 shrink-0 opacity-60" />}
      <div className="min-w-0 flex-1">
        <div className="flex min-w-0 items-center gap-1.5">
          <span className={cn("truncate font-mono text-[11.5px]", !w.branch && "opacity-70")}>{branch}</span>
          {w.main && <Chip hot={hot}>main</Chip>}
          {w.inUse ? (
            <Tip label={w.lockReason ?? "Locked by a running process"}>
              <Chip hot={hot} tone="live">
                working
              </Chip>
            </Tip>
          ) : (
            w.locked && (
              <Tip label={`Locked${w.lockReason ? `: ${w.lockReason}` : ""}`}>
                <Lock className="size-3 shrink-0 opacity-60" />
              </Tip>
            )
          )}
        </div>
        <div className={cn("flex min-w-0 items-center gap-1 text-[10.5px]", hot ? "opacity-80" : "text-subtle")}>
          <FolderGit2 className="size-3 shrink-0" />
          <span className="truncate">{w.main ? folderName(w.path) : shortPath(w.path, main)}</span>
        </div>
      </div>
      {/* Mounted on every row, shown on the hot one, like the branch picker's actions. */}
      <span className={cn("shrink-0 gap-0.5", hot && !w.prunable && !w.bare ? "flex" : "hidden")}>
        <RowAction hot={hot} label="Open a terminal here" onClick={act(onTerminal)}>
          <SquareTerminal />
        </RowAction>
        {!w.main && !w.current && (
          <RowAction hot={hot} label="Remove worktree…" onClick={act(onRemove)}>
            <Trash2 />
          </RowAction>
        )}
      </span>
      {(!hot || w.prunable || w.bare) && (
        <span className={cn("flex max-w-24 shrink-0 flex-col items-end text-[10.5px] leading-4", hot ? "opacity-80" : "text-subtle")}>
          <span className="max-w-full truncate">{w.prunable ? "missing" : w.bare ? "" : time ? relativeTime(time) : w.current ? "current" : ""}</span>
          {/* "No changes" flags the worktrees an agent was parked in and never touched. */}
          {count !== undefined && (
            <span className={cn("max-w-full truncate", count === 0 ? "opacity-60" : !hot && "text-foreground")}>
              {count === 0 ? "no changes" : `${count} change${count === 1 ? "" : "s"}`}
            </span>
          )}
        </span>
      )}
    </div>
  );
}

function Chip({ hot, tone, className, ...props }: React.ComponentProps<"span"> & { hot: boolean; tone?: "live" }) {
  return (
    <span
      {...props}
      className={cn(
        "shrink-0 rounded-sm px-1 text-[10px] leading-4",
        hot ? "bg-white/20" : tone === "live" ? "bg-primary/15 text-primary" : "bg-active text-muted-foreground",
        className,
      )}
    />
  );
}
