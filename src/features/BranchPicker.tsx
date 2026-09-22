import { Check, ChevronsUpDown, Cloud, GitBranch, GitMerge, GitPullRequestArrow, Plus, Search, SquareTerminal, Trash2 } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Tip, Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { type Branch, github } from "@/lib/api";
import { cn, relativeTime } from "@/lib/utils";

interface Props {
  label: string;
  current: string | null;
  branches: Branch[];
  onSwitch: (name: string) => void;
  onCreate: (name: string) => void;
  onMerge: (name: string) => void;
  onRebase: (name: string) => void;
  /** Opens a terminal on the branch. */
  onTerminal: (name: string) => void;
  /** Deletes a branch; asks first unless it's a merged local one. */
  onDelete: (branch: Branch) => void;
  /** Deletes these merged branches together (asks first). */
  onCleanUp: (names: string[]) => void;
  /** Where the list opens relative to the trigger. */
  side?: "top" | "bottom";
}

type Option = { kind: "create"; name: string } | { kind: "branch"; branch: Branch };

/** "origin/feature" → "feature": `git switch feature` then creates a tracking branch. */
const localName = (b: Branch) => (b.remote ? b.name.slice(b.name.indexOf("/") + 1) : b.name);

/**
 * Searchable branch switcher: type to filter, ↑/↓ + Enter to switch, or create what you
 * typed. The highlighted row also offers merging it into, or rebasing onto it.
 * Branches checked out in another worktree live in the worktree picker instead.
 */
export function BranchPicker({ label, current, branches, onSwitch, onCreate, onMerge, onRebase, onTerminal, onDelete, onCleanUp, side = "bottom" }: Props) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);
  // GitHub branch protection, asked when the menu opens. No GitHub, no answer: then only
  // the remote default is held back, and the confirm is what guards the rest.
  const [guarded, setGuarded] = useState<Set<string>>(new Set());

  // Switching to origin/x means switching to x, so a remote row goes with its local branch.
  const elsewhere = useMemo(() => {
    const held = new Set(branches.filter((b) => !b.remote && b.worktree).map((b) => b.name));
    return (b: Branch) => !b.current && (!!b.worktree || (b.remote && held.has(localName(b))));
  }, [branches]);

  const options = useMemo<Option[]>(() => {
    const q = query.trim().toLowerCase();
    const match = (b: Branch) => !elsewhere(b) && b.name.toLowerCase().includes(q);
    // Current first, then local by recency (backend order), then remote.
    const local = branches.filter((b) => !b.remote && match(b)).sort((a, b) => Number(b.current) - Number(a.current));
    const remote = branches.filter((b) => b.remote && match(b));
    const found = [...local, ...remote].map((branch) => ({ kind: "branch" as const, branch }));
    // "feature" matches origin/feature too: switching to it creates the tracking branch.
    const exact = branches.some((b) => b.name === query.trim() || localName(b) === query.trim());
    return q && !exact ? [...found, { kind: "create", name: query.trim() }] : found;
  }, [branches, elsewhere, query]);

  useEffect(() => setIndex(0), [query, open]);
  useEffect(() => {
    if (!open) return;
    let live = true;
    github
      .protectedBranches()
      .then((names) => live && setGuarded(new Set(names.map((n) => `origin/${n}`))))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [open]);

  useEffect(() => {
    listRef.current?.querySelector(`[data-option="${index}"]`)?.scrollIntoView({ block: "nearest" });
  }, [index]);

  const close = () => {
    setOpen(false);
    setQuery("");
  };

  // Merged into HEAD and held by no worktree: deleting them loses nothing.
  const stale = branches.filter((b) => b.merged && !b.worktree).map((b) => b.name);

  const choose = (o: Option | undefined) => {
    if (!o) return;
    if (o.kind === "create") onCreate(o.name);
    else if (!o.branch.current) onSwitch(localName(o.branch));
    else return;
    close();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") setIndex((i) => Math.min(options.length - 1, i + 1));
    else if (e.key === "ArrowUp") setIndex((i) => Math.max(0, i - 1));
    else if (e.key === "Enter") choose(options[index]);
    else return;
    e.preventDefault();
  };

  const act = (fn: (name: string) => void, name: string) => (e: React.MouseEvent) => {
    e.stopPropagation();
    fn(name);
    close();
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button className="flex h-7 max-w-72 min-w-0 items-center gap-1.5 rounded-md px-2 text-left hover:bg-hover data-[state=open]:bg-active">
          <GitBranch className="size-3.5 shrink-0 text-primary" />
          <span className="truncate font-mono text-[12px]">{label}</span>
          <ChevronsUpDown className="size-3 shrink-0 text-subtle" />
        </button>
      </PopoverTrigger>
      <PopoverContent side={side} align="start" className="flex w-96 flex-col overflow-hidden">
        <div className="flex h-9 shrink-0 items-center gap-2 border-b border-border px-2.5">
          <Search className="size-3.5 shrink-0 text-subtle" />
          <input
            autoFocus
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onKeyDown}
            placeholder="Find or create a branch…"
            className="h-full min-w-0 flex-1 bg-transparent font-mono text-[12px] outline-none placeholder:font-sans placeholder:text-subtle"
          />
        </div>
        <div ref={listRef} className="max-h-[360px] min-h-0 flex-1 overflow-x-hidden overflow-y-auto p-1">
          {options.length === 0 && (
            <div className="px-2 py-3 text-center text-[12px] text-subtle">
              {branches.some(elsewhere) ? "No branches here · ones checked out in other worktrees are in the worktree menu" : "No branches"}
            </div>
          )}
          {options.map((o, i) => {
            const prev = options[i - 1];
            const header =
              o.kind === "branch" && (!prev || prev.kind === "create" || prev.branch.remote !== o.branch.remote) ? (o.branch.remote ? "Remote" : "Local") : null;
            const hot = i === index;
            return (
              <div key={o.kind === "create" ? "\0create" : o.branch.name}>
                {header && <div className="px-2 pt-2 pb-1 text-[10.5px] font-semibold tracking-[0.08em] text-subtle uppercase">{header}</div>}
                <div
                  data-option={i}
                  role="option"
                  aria-selected={hot}
                  onMouseMove={() => setIndex(i)}
                  onClick={() => choose(o)}
                  className={cn(
                    "flex h-7 items-center gap-2 rounded-sm px-2 text-[12px]",
                    hot && "bg-primary text-primary-foreground",
                    o.kind === "branch" && o.branch.current ? "cursor-default" : "cursor-pointer",
                  )}
                >
                  {o.kind === "create" ? (
                    <>
                      <Plus className="size-3.5 shrink-0" />
                      <span className="truncate">
                        Create branch <span className="font-mono font-semibold">{o.name}</span>
                      </span>
                    </>
                  ) : (
                    <>
                      {o.branch.current ? (
                        <Check className="size-3.5 shrink-0" />
                      ) : o.branch.remote ? (
                        <Cloud className="size-3.5 shrink-0 opacity-60" />
                      ) : (
                        <GitBranch className="size-3.5 shrink-0 opacity-60" />
                      )}
                      <span className="truncate font-mono text-[11.5px]">{o.branch.name}</span>
                      {/* Mounted on every row, shown on the hot one: a tooltip whose button unmounts
                          as the highlight moves gets stuck open or shows the previous label. */}
                      <span className={cn("ml-auto shrink-0 gap-0.5", hot ? "flex" : "hidden")}>
                        <RowAction hot={hot} label={o.branch.current ? "Open a terminal here" : "Open a terminal in a new worktree"} onClick={act(onTerminal, localName(o.branch))}>
                          <SquareTerminal />
                        </RowAction>
                        {!o.branch.current && current && (
                          <>
                            <RowAction hot={hot} label={`Merge into ${current}`} onClick={act(onMerge, o.branch.name)}>
                              <GitMerge />
                            </RowAction>
                            <RowAction hot={hot} label={`Rebase ${current} onto it`} onClick={act(onRebase, o.branch.name)}>
                              <GitPullRequestArrow />
                            </RowAction>
                          </>
                        )}
                        {!o.branch.current && !o.branch.remoteDefault && !guarded.has(o.branch.name) && (
                          <RowAction hot={hot} label={o.branch.remote ? "Delete from the remote…" : o.branch.merged ? "Delete branch (merged)" : "Delete branch…"} onClick={act(() => onDelete(o.branch), o.branch.name)}>
                            <Trash2 />
                          </RowAction>
                        )}
                      </span>
                      {!hot && (
                        <span className="ml-auto max-w-40 shrink-0 truncate text-[10.5px] text-subtle">
                          {o.branch.current ? "current" : o.branch.merged ? `merged · ${relativeTime(o.branch.timestamp)}` : relativeTime(o.branch.timestamp)}
                        </span>
                      )}
                    </>
                  )}
                </div>
              </div>
            );
          })}
        </div>
        <div className="flex shrink-0 items-center gap-2 border-t border-border px-2.5 py-1.5 text-[10.5px] text-subtle">
          <span className="min-w-0 flex-1 truncate">↑↓ navigate · ↵ switch · hover for actions</span>
          {stale.length > 0 && (
            <Tip label={`Delete the ${stale.length} local branches already merged into ${current ?? "HEAD"}`}>
              <button
                onClick={() => {
                  onCleanUp(stale);
                  close();
                }}
                className="shrink-0 rounded-sm px-1.5 py-0.5 hover:bg-hover hover:text-foreground"
              >
                Clean up {stale.length} merged
              </button>
            </Tip>
          )}
        </div>
      </PopoverContent>
    </Popover>
  );
}

/** Icon button with a tooltip that closes as soon as the pointer leaves it. */
export function RowAction({ label, hot, onClick, children }: { label: string; hot: boolean; onClick: (e: React.MouseEvent) => void; children: React.ReactNode }) {
  return (
    <Tooltip disableHoverableContent>
      <TooltipTrigger asChild>
        <button aria-label={label} onClick={onClick} className="flex size-5 items-center justify-center rounded-sm bg-white/15 hover:bg-white/25 [&_svg]:size-3">
          {children}
        </button>
      </TooltipTrigger>
      {/* Arrow keys can move the highlight off a hovered button without a pointerleave. */}
      {hot && <TooltipContent>{label}</TooltipContent>}
    </Tooltip>
  );
}
