import { Check, ChevronsUpDown, Cloud, FolderGit2, GitBranch, GitMerge, GitPullRequestArrow, Plus, Search, SquareTerminal } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Tip } from "@/components/ui/tooltip";
import type { Branch } from "@/lib/api";
import { cn, relativeTime } from "@/lib/utils";
import { folderName } from "@/lib/worktrees";

interface Props {
  label: string;
  current: string | null;
  branches: Branch[];
  onSwitch: (name: string) => void;
  /** For a branch checked out in another worktree, which git won't switch to here. */
  onOpenWorktree: (path: string) => void;
  onCreate: (name: string) => void;
  onMerge: (name: string) => void;
  onRebase: (name: string) => void;
  /** Opens a terminal on the branch; `worktree` is where it's checked out, if anywhere else. */
  onTerminal: (name: string, worktree: string | null) => void;
  /** Where the list opens relative to the trigger. */
  side?: "top" | "bottom";
}

type Option = { kind: "create"; name: string } | { kind: "branch"; branch: Branch };

/** "origin/feature" → "feature": `git switch feature` then creates a tracking branch. */
const localName = (b: Branch) => (b.remote ? b.name.slice(b.name.indexOf("/") + 1) : b.name);

/**
 * Searchable branch switcher: type to filter, ↑/↓ + Enter to switch, or create what you
 * typed. The highlighted row also offers merging it into, or rebasing onto it.
 */
export function BranchPicker({ label, current, branches, onSwitch, onOpenWorktree, onCreate, onMerge, onRebase, onTerminal, side = "bottom" }: Props) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);

  const options = useMemo<Option[]>(() => {
    const q = query.trim().toLowerCase();
    const match = (b: Branch) => b.name.toLowerCase().includes(q);
    // Current first, then local by recency (backend order), then remote.
    const local = branches.filter((b) => !b.remote && match(b)).sort((a, b) => Number(b.current) - Number(a.current));
    const remote = branches.filter((b) => b.remote && match(b));
    const found = [...local, ...remote].map((branch) => ({ kind: "branch" as const, branch }));
    // "feature" matches origin/feature too: switching to it creates the tracking branch.
    const exact = branches.some((b) => b.name === query.trim() || localName(b) === query.trim());
    return q && !exact ? [...found, { kind: "create", name: query.trim() }] : found;
  }, [branches, query]);

  useEffect(() => setIndex(0), [query, open]);
  useEffect(() => {
    listRef.current?.querySelector(`[data-option="${index}"]`)?.scrollIntoView({ block: "nearest" });
  }, [index]);

  const close = () => {
    setOpen(false);
    setQuery("");
  };

  // Switching to origin/x means switching to x, so a remote row follows its local branch.
  const elsewhere = (b: Branch) => b.worktree ?? branches.find((l) => !l.remote && l.name === localName(b))?.worktree ?? null;

  const terminalTip = (b: Branch) => {
    const worktree = elsewhere(b);
    if (b.current) return "Open a terminal here";
    return worktree ? `Open a terminal in ${folderName(worktree)}` : "Open a terminal in a new worktree";
  };

  const choose = (o: Option | undefined) => {
    if (!o) return;
    const worktree = o.kind === "branch" ? elsewhere(o.branch) : null;
    if (o.kind === "create") onCreate(o.name);
    else if (worktree) onOpenWorktree(worktree);
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
          {options.length === 0 && <div className="px-2 py-3 text-center text-[12px] text-subtle">No branches</div>}
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
                      ) : o.branch.worktree ? (
                        <FolderGit2 className="size-3.5 shrink-0 opacity-60" />
                      ) : (
                        <GitBranch className="size-3.5 shrink-0 opacity-60" />
                      )}
                      <span className="truncate font-mono text-[11.5px]">{o.branch.name}</span>
                      {hot ? (
                        <span className="ml-auto flex shrink-0 gap-0.5">
                          <Tip label={terminalTip(o.branch)}>
                            <button
                              onClick={(e) => {
                                e.stopPropagation();
                                onTerminal(localName(o.branch), o.branch.current ? null : elsewhere(o.branch));
                                close();
                              }}
                              className="flex h-5 items-center gap-1 rounded-sm bg-white/15 px-1.5 text-[11px] hover:bg-white/25"
                            >
                              <SquareTerminal className="size-3" /> Terminal
                            </button>
                          </Tip>
                          {!o.branch.current && current && (
                            <>
                              <Tip label={`Merge into ${current}`}>
                                <button onClick={act(onMerge, o.branch.name)} className="flex h-5 items-center gap-1 rounded-sm bg-white/15 px-1.5 text-[11px] hover:bg-white/25">
                                  <GitMerge className="size-3" /> Merge
                                </button>
                              </Tip>
                              <Tip label={`Rebase ${current} onto it`}>
                                <button onClick={act(onRebase, o.branch.name)} className="flex h-5 items-center gap-1 rounded-sm bg-white/15 px-1.5 text-[11px] hover:bg-white/25">
                                  <GitPullRequestArrow className="size-3" /> Rebase
                                </button>
                              </Tip>
                            </>
                          )}
                        </span>
                      ) : (
                        <span className={cn("ml-auto max-w-40 shrink-0 truncate text-[10.5px]", hot ? "opacity-80" : "text-subtle")}>
                          {o.branch.current ? "current" : o.branch.worktree ? `in ${folderName(o.branch.worktree)}` : relativeTime(o.branch.timestamp)}
                        </span>
                      )}
                    </>
                  )}
                </div>
              </div>
            );
          })}
        </div>
        <div className="shrink-0 border-t border-border px-2.5 py-1.5 text-[10.5px] text-subtle">↑↓ navigate · ↵ switch · hover a branch to merge, rebase or open a terminal</div>
      </PopoverContent>
    </Popover>
  );
}
