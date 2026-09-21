import { Check, ChevronsUpDown, CornerUpLeft, FolderGit2, Lock } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Tip } from "@/components/ui/tooltip";
import { api, type Worktree } from "@/lib/api";
import { cn } from "@/lib/utils";
import { folderName, shortPath } from "@/lib/worktrees";

interface Props {
  worktrees: Worktree[];
  /** Opens a worktree in this window (the regular open-repo flow). */
  onOpen: (path: string) => void;
}

/**
 * `git worktree list` as a switcher, shown once the repo has more than one worktree.
 * In a linked worktree it names it and offers the way back to the main one.
 */
export function WorktreePicker({ worktrees, onOpen }: Props) {
  const [open, setOpen] = useState(false);
  const [list, setList] = useState(worktrees);
  // Change counts need a `git status` per worktree: fetched when the menu opens, never before.
  const [counts, setCounts] = useState<Record<string, number>>({});

  useEffect(() => setList(worktrees), [worktrees]);
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

  if (list.length < 2) return null;
  const current = list.find((w) => w.current);
  const main = list.find((w) => w.main && !w.bare);
  const linked = !!current && !current.main;
  const pick = (w: Worktree) => {
    setOpen(false);
    onOpen(w.path);
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
        <PopoverContent align="start" className="flex w-96 flex-col overflow-hidden">
          <div className="px-3 pt-2.5 pb-1 text-[10.5px] font-semibold tracking-[0.08em] text-subtle uppercase">Worktrees</div>
          <div className="max-h-[360px] min-h-0 overflow-x-hidden overflow-y-auto p-1">
            {list.map((w) => (
              <WorktreeRow key={w.path} w={w} main={main?.path ?? w.path} count={counts[w.path]} onPick={pick} />
            ))}
          </div>
          {list.some((w) => w.prunable) && (
            <div className="shrink-0 border-t border-border px-2.5 py-1.5 text-[10.5px] text-subtle">
              Missing folders stay listed until <span className="font-mono">git worktree prune</span>.
            </div>
          )}
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

function WorktreeRow({ w, main, count, onPick }: { w: Worktree; main: string; count: number | undefined; onPick: (w: Worktree) => void }) {
  const usable = !w.current && !w.prunable && !w.bare;
  const branch = w.branch ?? (w.bare ? "bare" : `detached @ ${w.head ?? "?"}`);
  return (
    <div
      role="button"
      aria-disabled={!usable}
      tabIndex={usable ? 0 : -1}
      onClick={() => usable && onPick(w)}
      onKeyDown={(e) => {
        if (usable && (e.key === "Enter" || e.key === " ")) {
          e.preventDefault();
          onPick(w);
        }
      }}
      className={cn(
        "flex h-9 items-center gap-2.5 rounded-sm px-2 select-none",
        w.current ? "cursor-default bg-active" : usable ? "cursor-pointer hover:bg-hover" : "cursor-default opacity-50",
      )}
    >
      {w.current ? <Check className="size-3.5 shrink-0 text-primary" /> : <FolderGit2 className="size-3.5 shrink-0 text-subtle" />}
      <div className="min-w-0 flex-1">
        <div className="flex min-w-0 items-baseline gap-2">
          <span className="truncate text-[12.5px] font-medium">{folderName(w.path)}</span>
          <span className="min-w-0 truncate font-mono text-[11px] text-muted-foreground">{branch}</span>
        </div>
        <div className="truncate text-[10.5px] text-subtle">
          {w.main ? "main worktree" : shortPath(w.path, main)}
        </div>
      </div>
      {w.locked && (
        <Tip label="Locked (git worktree lock)">
          <Lock className="size-3 shrink-0 text-subtle" />
        </Tip>
      )}
      <span className="shrink-0 text-[10.5px] text-subtle">
        {w.prunable ? "missing" : w.bare ? "" : count === undefined ? "…" : count === 0 ? "clean" : `${count} changed`}
      </span>
    </div>
  );
}
