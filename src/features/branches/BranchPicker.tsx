import { Check, ChevronRight, ChevronsUpDown, Cloud, GitBranch, GitBranchPlus, GitMerge, GitPullRequestArrow, Link, Pencil, Plus, Search, SquareTerminal, Trash2, Unlink } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuShortcut, ContextMenuTrigger } from "@/components/ui/context-menu";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Tip } from "@/components/ui/tooltip";
import { type Branch, fullName, github } from "@/lib/api";
import { matchesCommand, useCommands, useShortcut } from "@/lib/commands/keybindings";
import { pointerMoved } from "@/lib/ui/pointer";
import { isMenuKey, openRowMenu } from "@/lib/ui/useListNav";
import { cn } from "@/lib/utils";
import { relativeTime } from "@/lib/format";
import { RowAction } from "@/components/RowAction";
import { useAsyncValue } from "@/hooks/useAsyncValue";
import { usePickerIndex } from "@/hooks/usePickerIndex";
import { useGitHubAccount } from "@/features/github/shared/useGitHubAccount";

interface Props {
  label: string;
  current: string | null;
  branches: Branch[];
  onSwitch: (name: string) => void;
  /** Switches to a remote branch's local branch, creating it to track exactly that remote. */
  onSwitchRemote: (branch: Branch) => void;
  onCreate: (name: string) => void;
  onMerge: (name: string, how?: "ff" | "no-ff" | "squash") => void;
  onRebase: (name: string) => void;
  /** Opens a terminal on the branch. */
  onTerminal: (name: string) => void;
  /** Deletes a branch; asks first unless it's a merged local one. */
  onDelete: (branch: Branch) => void;
  /** Deletes these merged branches together (asks first). */
  onCleanUp: (names: string[]) => void;
  onRename: (branch: Branch) => void;
  /** A new branch at `base`: a full ref (refs/heads/…, refs/remotes/…), or HEAD. */
  onNewBranch: (base: string) => void;
  onSetUpstream: (branch: Branch) => void;
  onUnsetUpstream: (branch: Branch) => void;
  /** Where the list opens relative to the trigger. */
  side?: "top" | "bottom";
}

type Option = { kind: "create"; name: string } | { kind: "branch"; branch: Branch };

/** "origin/feature" → "feature": `git switch feature` then creates a tracking branch. */
const localName = (b: Branch) => (b.remote ? b.name.slice(b.name.indexOf("/") + 1) : b.name);
const remoteOf = (b: Branch) => b.name.slice(0, b.name.indexOf("/"));
const LOCAL = "Local";

/**
 * Searchable branch switcher: type to filter, ↑/↓ + Enter to switch, or create what you
 * typed. The highlighted row also offers merging it into, or rebasing onto it.
 * Branches checked out in another worktree live in the worktree picker instead.
 */
export function BranchPicker({ label, current, branches, onSwitch, onSwitchRemote, onCreate, onMerge, onRebase, onTerminal, onDelete, onCleanUp, onRename, onNewBranch, onSetUpstream, onUnsetUpstream, side = "bottom" }: Props) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const listRef = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  // GitHub branch protection, asked when the menu opens. No GitHub, no answer: then only
  // the remote default is held back, and the confirm is what guards the rest.
  const guarded = useAsyncValue(open ? () => github.protectedBranches().then((names) => new Set(names.map((n) => `origin/${n}`))) : null, [open], new Set<string>());
  useCommands({ "git.switchBranch": () => setOpen(true) });

  // Switching to origin/x means switching to x, so a remote row goes with its local branch.
  const elsewhere = useMemo(() => {
    const held = new Set(branches.filter((b) => !b.remote && b.worktree).map((b) => b.name));
    return (b: Branch) => !b.current && (!!b.worktree || (b.remote && held.has(localName(b))));
  }, [branches]);

  // Local, then one group per remote (origin first), each by recency. All open until closed.
  const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set());
  const q = query.trim().toLowerCase();
  const groups = useMemo(() => {
    const match = (b: Branch) => !elsewhere(b) && b.name.toLowerCase().includes(q);
    const byGroup = new Map<string, Branch[]>([[LOCAL, []]]);
    // Current first among local, then backend order (recency).
    const found = branches.filter(match).sort((a, b) => Number(b.current) - Number(a.current));
    const remotes = [...new Set(found.filter((b) => b.remote).map(remoteOf))].sort((a, b) => Number(b === "origin") - Number(a === "origin") || a.localeCompare(b));
    for (const r of remotes) byGroup.set(r, []);
    for (const b of found) byGroup.get(b.remote ? remoteOf(b) : LOCAL)!.push(b);
    return [...byGroup].filter(([, list]) => list.length).map(([name, list]) => ({ name, list }));
  }, [branches, elsewhere, q]);
  // Searching shows every match, whatever is collapsed.
  const isOpen = (group: string) => !!q || !collapsed.has(group);
  const toggleGroup = (group: string) =>
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (!next.delete(group)) next.add(group);
      return next;
    });

  const options = useMemo<Option[]>(() => {
    const found = groups.filter((g) => isOpen(g.name)).flatMap((g) => g.list.map((branch) => ({ kind: "branch" as const, branch })));
    // "feature" matches origin/feature too: switching to it creates the tracking branch.
    const exact = branches.some((b) => b.name === query.trim() || localName(b) === query.trim());
    return q && !exact ? [...found, { kind: "create", name: query.trim() }] : found;
  }, [groups, branches, query, q, collapsed]);
  const { index, setIndex, move } = usePickerIndex(options.length);

  // Which GitHub repository each remote is, so branches on one you can't push to (a fork's
  // original) offer no delete.
  const remoteRepos = useAsyncValue(open ? () => github.remotes().then((list) => new Map(list.map((r) => [r.name, r.repo]))) : null, [open], new Map<string, string | null>());
  const { account } = useGitHubAccount(open);
  const accessOf = (remote: string) => {
    const repo = remoteRepos.get(remote)?.toLowerCase();
    return repo ? ([account?.origin, account?.parent].find((a) => a && fullName(a.repo).toLowerCase() === repo) ?? null) : null;
  };

  useEffect(() => setIndex(0), [query, open]);

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
    else if (o.branch.remote) onSwitchRemote(o.branch);
    else if (!o.branch.current) onSwitch(o.branch.name);
    else return;
    close();
  };

  const renameKey = useShortcut("git.renameBranch");
  const rename = (o: Option | undefined) => {
    if (o?.kind !== "branch" || o.branch.remote) return;
    onRename(o.branch);
    close();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (matchesCommand("git.renameBranch", e.nativeEvent)) rename(options[index]);
    else if (e.key === "ArrowDown") move(1);
    else if (e.key === "ArrowUp") move(-1);
    else if (e.key === "Enter") choose(options[index]);
    else if (isMenuKey(e)) {
      const row = listRef.current?.querySelector<HTMLElement>(`[data-option="${index}"]`);
      if (row) openRowMenu(row);
    } else return;
    e.preventDefault();
  };

  const act = (fn: (name: string) => void, name: string) => (e: React.MouseEvent) => {
    e.stopPropagation();
    fn(name);
    close();
  };

  // Where each open group's rows start in `options`, which ↑↓ and Enter walk.
  const starts = new Map<string, number>();
  let n = 0;
  for (const g of groups) {
    if (!isOpen(g.name)) continue;
    starts.set(g.name, n);
    n += g.list.length;
  }

  const menuAct = (fn: () => void) => () => {
    fn();
    close();
  };

  /** Right-click actions on a branch row. */
  const branchMenu = (b: Branch) => (
    // Not to the row, which can't take it: back to the search box while the picker is open. The
    // dialogs these open take the focus themselves.
    <ContextMenuContent
      onCloseAutoFocus={(e) => {
        e.preventDefault();
        if (input.current?.isConnected) input.current.focus();
      }}
    >
      {!b.current && current && (
        <>
          <ContextMenuItem onSelect={menuAct(() => onMerge(b.name))}>
            <GitMerge /> Merge into {current}
          </ContextMenuItem>
          <ContextMenuItem onSelect={menuAct(() => onMerge(b.name, "no-ff"))}>
            <GitMerge /> Merge into {current} (No Fast-forward)
          </ContextMenuItem>
          <ContextMenuItem onSelect={menuAct(() => onMerge(b.name, "squash"))}>
            <GitMerge /> Squash and Merge into {current}
          </ContextMenuItem>
          <ContextMenuSeparator />
        </>
      )}
      {!b.remote && (
        <ContextMenuItem onSelect={menuAct(() => onRename(b))}>
          <Pencil /> Rename…{renameKey && <ContextMenuShortcut>{renameKey}</ContextMenuShortcut>}
        </ContextMenuItem>
      )}
      <ContextMenuItem onSelect={menuAct(() => onNewBranch(`refs/${b.remote ? "remotes" : "heads"}/${b.name}`))}>
        <GitBranchPlus /> New branch from {b.name}…
      </ContextMenuItem>
      {!b.remote && (
        <>
          <ContextMenuSeparator />
          <ContextMenuItem onSelect={menuAct(() => onSetUpstream(b))}>
            <Link /> {b.upstream ? "Change upstream…" : "Set upstream…"}
          </ContextMenuItem>
          <ContextMenuItem disabled={!b.upstream} onSelect={menuAct(() => onUnsetUpstream(b))}>
            <Unlink /> Unset upstream{b.upstream && <span className="ml-auto pl-4 font-mono text-[11px] opacity-70">{b.upstream}</span>}
          </ContextMenuItem>
        </>
      )}
    </ContextMenuContent>
  );

  const optionRow = (o: Option, i: number) => {
    const hot = i === index;
    const row = (
      <div
        data-option={i}
        role="option"
        aria-selected={hot}
        onMouseMove={(e) => pointerMoved(e) && setIndex(i)}
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
              <RowAction variant="picker" hot={hot} label={o.branch.current ? "Open a terminal here" : "Open a terminal in a new worktree"} onClick={act(onTerminal, localName(o.branch))}>
                <SquareTerminal />
              </RowAction>
              {!o.branch.current && current && (
                <>
                  <RowAction variant="picker" hot={hot} label={`Merge into ${current}`} onClick={act(onMerge, o.branch.name)}>
                    <GitMerge />
                  </RowAction>
                  <RowAction variant="picker" hot={hot} label={`Rebase ${current} onto it`} onClick={act(onRebase, o.branch.name)}>
                    <GitPullRequestArrow />
                  </RowAction>
                </>
              )}
              {/* Where you can't push, GitHub would refuse the delete anyway. */}
              {!o.branch.current && !o.branch.remoteDefault && !guarded.has(o.branch.name) && !(o.branch.remote && accessOf(remoteOf(o.branch))?.push === false) && (
                <RowAction variant="picker" hot={hot} label={o.branch.remote ? "Delete from the remote…" : o.branch.merged ? "Delete branch (merged)" : "Delete branch…"} onClick={act(() => onDelete(o.branch), o.branch.name)}>
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
    );
    return (
      <div key={o.kind === "create" ? "\0create" : o.branch.name}>
        {o.kind === "create" ? (
          row
        ) : (
          <ContextMenu>
            <ContextMenuTrigger asChild>{row}</ContextMenuTrigger>
            {branchMenu(o.branch)}
          </ContextMenu>
        )}
      </div>
    );
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button className="flex h-7 max-w-72 min-w-0 items-center gap-1.5 rounded-md px-2 text-left hover:bg-hover focus-visible:bg-hover data-[state=open]:bg-active">
          <GitBranch className="size-3.5 shrink-0 text-primary" />
          <span className="truncate font-mono text-[12px]">{label}</span>
          <ChevronsUpDown className="size-3 shrink-0 text-subtle" />
        </button>
      </PopoverTrigger>
      <PopoverContent side={side} align="start" className="flex w-96 flex-col overflow-hidden">
        <div className="flex h-9 shrink-0 items-center gap-2 border-b border-border px-2.5">
          <Search className="size-3.5 shrink-0 text-subtle" />
          <input
            ref={input}
            autoFocus
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onKeyDown}
            placeholder="Find or create a branch…"
            className="h-full min-w-0 flex-1 bg-transparent font-mono text-[12px] outline-none placeholder:font-sans placeholder:text-subtle"
          />
        </div>
        {/* Like a native menu: the highlight leaves with the mouse; ↑↓ bring it back. */}
        <div ref={listRef} onMouseLeave={(e) => pointerMoved(e) && setIndex(-1)} className="max-h-[360px] min-h-0 flex-1 overflow-x-hidden overflow-y-auto p-1">
          {groups.length === 0 && options.length === 0 && (
            <div className="px-2 py-3 text-center text-[12px] text-subtle">
              {branches.some(elsewhere) ? "No branches here · ones checked out in other worktrees are in the worktree menu" : "No branches"}
            </div>
          )}
          {groups.map((g) => (
            <div key={g.name}>
              <button
                // Keep the focus in the search box.
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => toggleGroup(g.name)}
                disabled={!!q}
                aria-expanded={isOpen(g.name)}
                className="flex w-full items-center gap-1 px-1 pt-2 pb-1 text-left text-[10.5px] font-semibold tracking-[0.08em] text-subtle uppercase hover:text-foreground focus-visible:text-foreground disabled:hover:text-subtle"
              >
                <ChevronRight className={cn("size-3 shrink-0 transition-transform", isOpen(g.name) && "rotate-90")} />
                {g.name}
                <span className="font-normal tracking-normal">{g.list.length}</span>
              </button>
              {isOpen(g.name) && g.list.map((branch, j) => optionRow({ kind: "branch", branch }, starts.get(g.name)! + j))}
            </div>
          ))}
          {options.at(-1)?.kind === "create" && optionRow(options.at(-1)!, options.length - 1)}
        </div>
        <div className="flex shrink-0 items-center gap-2 border-t border-border px-2.5 py-1.5 text-[10.5px] text-subtle">
          <span className="min-w-0 flex-1 truncate">↑↓ navigate · ↵ switch · {renameKey ? `${renameKey} rename · ` : ""}⇧F10 or right-click for more</span>
          <Tip label={`New branch from ${current ?? "HEAD"}, or from any branch or tag`}>
            <button
              onClick={() => {
                onNewBranch(current ? `refs/heads/${current}` : "HEAD");
                close();
              }}
              className="shrink-0 rounded-sm px-1.5 py-0.5 hover:bg-hover focus-visible:bg-hover hover:text-foreground focus-visible:text-foreground"
            >
              New branch…
            </button>
          </Tip>
          {stale.length > 0 && (
            <Tip label={`Delete the ${stale.length} local branches already merged into ${current ?? "HEAD"}`}>
              <button
                onClick={() => {
                  onCleanUp(stale);
                  close();
                }}
                className="shrink-0 rounded-sm px-1.5 py-0.5 hover:bg-hover focus-visible:bg-hover hover:text-foreground focus-visible:text-foreground"
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

