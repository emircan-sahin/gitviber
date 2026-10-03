import { Check, ChevronRight, ChevronsUpDown, Cloud, FolderGit2, GitBranch, GitBranchPlus, GitCommitHorizontal, GitMerge, GitPullRequestArrow, Link, Pencil, Pin, PinOff, Plus, Search, SquareTerminal, Trash2, Unlink } from "lucide-react";
import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuShortcut, ContextMenuTrigger } from "@/components/ui/context-menu";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Tip } from "@/components/ui/tooltip";
import { api, type Branch, fullName, github, type Worktree } from "@/lib/api";
import { matchesCommand, useCommands, useShortcut } from "@/lib/commands/keybindings";
import { pointerMoved } from "@/lib/ui/pointer";
import { isMenuKey, openRowMenu } from "@/lib/ui/useListNav";
import { cn } from "@/lib/utils";
import { relativeTime } from "@/lib/format";
import { sameRef, sanitizedRefName } from "@/lib/git/refs";
import { mainBackOffer } from "@/lib/git/worktrees";
import { folderName } from "@/lib/path";
import { loadPinnedBranches, savePinnedBranches } from "@/lib/repo/session";
import { RowAction } from "@/components/RowAction";
import { useAsyncValue } from "@/hooks/useAsyncValue";
import { usePickerIndex } from "@/hooks/usePickerIndex";
import { useGitHubAccount } from "@/features/github/shared/useGitHubAccount";

interface Props {
  /** The main worktree: pins are kept per project, across its worktrees. */
  main: string;
  label: string;
  current: string | null;
  branches: Branch[];
  /** What each branch's `worktree` path is: a folder that's gone is pruned, not opened. */
  worktrees: Worktree[];
  onSwitch: (name: string) => void;
  /** Switches to a remote branch's local branch, creating it to track exactly that remote. */
  onSwitchRemote: (branch: Branch) => void;
  onCreate: (name: string) => void;
  onMerge: (name: string, how?: "ff" | "no-ff" | "squash") => void;
  onRebase: (name: string) => void;
  /** Opens a terminal on the branch. */
  onTerminal: (name: string) => void;
  /** Opens the worktree at `path`, for a branch checked out there. */
  onOpenWorktree: (path: string, branch: string) => void;
  /** Checks a held branch's commit out here, detached: nothing is moved. */
  onDetach: (branch: Branch) => void;
  /** Prunes a worktree whose folder is gone, which frees its branch, then switches to it. */
  onPruneHolder: (worktree: Worktree, branch: string) => void;
  /** Hands the default branch back to the main folder (the confirm is the caller's). */
  onMainBack: (branch: string) => void;
  /** Deletes a branch; asks first unless it's a merged local one, here or `upstream`. */
  onDelete: (branch: Branch, upstream: boolean) => void;
  /** Deletes these merged branches together (asks first); `upstream` ones git sees as unmerged. */
  onCleanUp: (merged: string[], upstream: string[]) => void;
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
const optionKey = (o: Option | undefined) => (o ? (o.kind === "create" ? "\0create" : o.branch.name) : null);
const LOCAL = "Local";
const PINNED = "Pinned";
const RECENT = "Recent";
/** As GitHub Desktop's recent branches. */
const RECENT_MAX = 5;

/**
 * Searchable branch switcher: type to filter, ↑/↓ + Enter to switch, or create what you
 * typed. The highlighted row also offers merging it into, or rebasing onto it.
 * A branch checked out in another worktree, which git won't switch to here, opens that worktree
 * (↵, and the row says so); its menu has the other ways: detached here, or a new branch from it.
 */
export function BranchPicker({ main, label, current, branches, worktrees, onSwitch, onSwitchRemote, onCreate, onMerge, onRebase, onTerminal, onOpenWorktree, onDetach, onPruneHolder, onMainBack, onDelete, onCleanUp, onRename, onNewBranch, onSetUpstream, onUnsetUpstream, side = "bottom" }: Props) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const listRef = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  // GitHub branch protection, asked when the menu opens. No GitHub, no answer: then only
  // the remote default is held back, and the confirm is what guards the rest.
  const guarded = useAsyncValue(open ? () => github.protectedBranches().then((names) => new Set(names.map((n) => `origin/${n}`))) : null, [open], new Set<string>());
  useCommands({ "git.switchBranch": () => setOpen(true) });

  // The worktree a branch is checked out in, other than this one. Switching to origin/x means
  // switching to x, so a remote row goes with its local branch.
  const heldIn = useMemo(() => {
    const held = new Map(branches.filter((b) => !b.remote && !b.current && b.worktree).map((b) => [b.name, b.worktree!]));
    return (b: Branch) => (b.current ? null : (b.remote ? held.get(localName(b)) : b.worktree) ?? null);
  }, [branches]);

  const holderOf = (b: Branch) => worktrees.find((w) => w.path === heldIn(b));
  const mainBack = useMemo(() => mainBackOffer(worktrees, branches), [worktrees, branches]);

  // Read at once, so the first open's rows don't move; again on each open (a rename moved one).
  const [pins, setPins] = useState(() => loadPinnedBranches(main));
  useEffect(() => {
    if (!open) return;
    // A pin of a branch deleted or renamed outside the app goes, rather than pin a new branch
    // that takes its name. Not while the list is still empty (loading).
    const saved = loadPinnedBranches(main);
    const kept = branches.length ? saved.filter((name) => branches.some((b) => b.name === name)) : saved;
    if (kept.length !== saved.length) savePinnedBranches(main, kept);
    setPins(kept);
  }, [open, main]);
  const togglePin = (name: string) => {
    const next = pins.includes(name) ? pins.filter((p) => p !== name) : [...pins, name];
    setPins(next);
    savePinnedBranches(main, next);
  };
  // Asked on each open: a checkout in the terminal moves it.
  const visited = useAsyncValue(open ? () => api.recentBranches() : null, [open], [] as string[]);

  // Pinned, recently checked out, the rest of local, then one group per remote (origin first),
  // each by recency. A branch shows in one group only. All open until closed.
  const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set());
  const q = query.trim().toLowerCase();
  const groups = useMemo(() => {
    // "fix login" finds fix-login, which is what it would be created as.
    const typed = sanitizedRefName(q);
    const match = (b: Branch) => (b.name.toLowerCase().includes(q) || (!!typed && b.name.toLowerCase().includes(typed)));
    const local = new Map(branches.filter((b) => !b.remote).map((b) => [b.name, b]));
    const pinnedList = pins.filter((name) => branches.some((b) => b.name === name));
    const pinned = new Set(pinnedList);
    const recentList = visited.filter((name) => local.has(name) && !local.get(name)!.current && !pinned.has(name)).slice(0, RECENT_MAX);
    const recent = new Set(recentList);
    const byGroup = new Map<string, Branch[]>([
      [PINNED, []],
      [RECENT, []],
      [LOCAL, []],
    ]);
    // Current first among local, then backend order (recency).
    const found = branches.filter(match).sort((a, b) => Number(b.current) - Number(a.current));
    const remotes = [...new Set(found.filter((b) => b.remote).map(remoteOf))].sort((a, b) => Number(b === "origin") - Number(a === "origin") || a.localeCompare(b));
    for (const r of remotes) byGroup.set(r, []);
    for (const b of found) byGroup.get(pinned.has(b.name) ? PINNED : recent.has(b.name) ? RECENT : b.remote ? remoteOf(b) : LOCAL)!.push(b);
    // Pins in the order they were pinned, recent ones newest first.
    const order = (names: string[]) => {
      const at = new Map(names.map((n, i) => [n, i]));
      return (a: Branch, b: Branch) => at.get(a.name)! - at.get(b.name)!;
    };
    byGroup.get(PINNED)!.sort(order(pinnedList));
    byGroup.get(RECENT)!.sort(order(recentList));
    return [...byGroup].filter(([, list]) => list.length).map(([name, list]) => ({ name, list }));
  }, [branches, q, pins, visited]);
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
    // What's typed is created as git takes it ("fix login" → fix-login), never over a branch.
    const name = sanitizedRefName(query.trim());
    const exact = branches.some((b) => [query.trim(), name].includes(b.name) || localName(b) === name || (!b.remote && sameRef(b.name, name)));
    return name && !exact ? [...found, { kind: "create", name }] : found;
  }, [groups, branches, query, q, collapsed]);
  const { index, setIndex, move } = usePickerIndex(options.length);
  // Rows that move under the highlight (the Recent group arriving after the picker opened) keep
  // it on the same branch, so a quick ↓ and Enter switches to the one that was highlighted.
  const shown = useRef({ options, index });
  useLayoutEffect(() => {
    const was = shown.current;
    shown.current = { options, index };
    if (was.options === options || was.index !== index) return;
    const key = optionKey(was.options[index]);
    const at = key === null ? -1 : options.findIndex((o) => optionKey(o) === key);
    if (at >= 0 && at !== index) {
      shown.current = { options, index: at };
      setIndex(at);
    }
  }, [options, index]);
  const listId = useId();
  const hotOption = options[index];
  const hotHeld = hotOption?.kind === "branch" && heldIn(hotOption.branch) ? hotOption.branch : null;

  // Which GitHub repository each remote is, so branches on one you can't push to (a fork's
  // original) offer no delete.
  const remoteRepos = useAsyncValue(open ? () => github.remotes().then((list) => new Map(list.map((r) => [r.name, r.repo]))) : null, [open], new Map<string, string | null>());
  const { account } = useGitHubAccount(open);
  const accessOf = (remote: string) => {
    const repo = remoteRepos.get(remote)?.toLowerCase();
    return repo ? ([account?.origin, account?.parent].find((a) => a && fullName(a.repo).toLowerCase() === repo) ?? null) : null;
  };

  // Opening starts on the current branch, where Enter does nothing; a search, on its first match.
  useEffect(() => setIndex(q ? 0 : Math.max(0, options.findIndex((o) => o.kind === "branch" && o.branch.current))), [query, open]);

  useEffect(() => {
    listRef.current?.querySelector(`[data-option="${index}"]`)?.scrollIntoView({ block: "nearest" });
  }, [index]);

  const close = () => {
    setOpen(false);
    setQuery("");
  };

  // Squash- or rebase-merged, then deleted on the remote. Asked on each open, as it reads their
  // diffs; null until known, so Clean up never counts from an older open, nor twice.
  const [landed, setLanded] = useState<Set<string> | null>(null);
  useEffect(() => {
    setLanded(null);
    if (!open) return;
    let live = true;
    api.mergedUpstream().then(
      (names) => live && setLanded(new Set(names)),
      () => live && setLanded(new Set()),
    );
    return () => {
      live = false;
    };
  }, [open]);
  const upstream = (b: Branch) => !b.remote && !b.merged && !!landed?.has(b.name);
  // Merged and held by no worktree, this one included: deleting them loses nothing.
  const stale = branches.filter((b) => b.merged && !b.worktree).map((b) => b.name);
  const squashed = branches.filter((b) => upstream(b) && !b.current && !b.worktree).map((b) => b.name);
  const cleanable = stale.length + squashed.length;

  const openHolder = (b: Branch) => {
    const holder = holderOf(b);
    if (holder?.prunable) onPruneHolder(holder, localName(b));
    else onOpenWorktree(heldIn(b)!, localName(b));
  };
  /** What ↵ does on a held branch's row. */
  const heldHint = (b: Branch) => (holderOf(b)?.prunable ? `${folderName(heldIn(b)!)} is gone` : `opens ${folderName(heldIn(b)!)}`);

  const choose = (o: Option | undefined) => {
    if (!o) return;
    if (o.kind === "create") onCreate(o.name);
    else if (heldIn(o.branch)) openHolder(o.branch);
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
      {heldIn(b) && (
        <>
          <ContextMenuItem onSelect={menuAct(() => openHolder(b))}>
            <FolderGit2 /> {holderOf(b)?.prunable ? `Prune ${folderName(heldIn(b)!)}, its folder is gone…` : `Open worktree ${folderName(heldIn(b)!)}`}
            <ContextMenuShortcut>↵</ContextMenuShortcut>
          </ContextMenuItem>
          {!holderOf(b)?.prunable && (
            <ContextMenuItem onSelect={menuAct(() => onDetach(b))}>
              <GitCommitHorizontal /> Check out here, detached
            </ContextMenuItem>
          )}
          {mainBack?.branch === localName(b) && (
            <ContextMenuItem onSelect={menuAct(() => onMainBack(mainBack.branch))}>
              <FolderGit2 /> Move {mainBack.branch} back to {folderName(mainBack.main.path)}…
            </ContextMenuItem>
          )}
          <ContextMenuSeparator />
        </>
      )}
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
      <ContextMenuItem onSelect={() => togglePin(b.name)}>
        {pins.includes(b.name) ? (
          <>
            <PinOff /> Unpin
          </>
        ) : (
          <>
            <Pin /> Pin to top
          </>
        )}
      </ContextMenuItem>
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
        id={`${listId}-${i}`}
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
            ) : heldIn(o.branch) ? (
              <FolderGit2 className="size-3.5 shrink-0 opacity-60" />
            ) : o.branch.remote ? (
              <Cloud className="size-3.5 shrink-0 opacity-60" />
            ) : (
              <GitBranch className="size-3.5 shrink-0 opacity-60" />
            )}
            <span className="truncate font-mono text-[11.5px]">{o.branch.name}</span>
            {/* Hot or not, a row that opens a worktree says which: a plain one switches this checkout. */}
            <span className="ml-auto flex min-w-0 shrink-0 items-center gap-1.5">
              {/* Mounted on every row, shown on the hot one: a tooltip whose button unmounts
                  as the highlight moves gets stuck open or shows the previous label. */}
              <span className={cn("shrink-0 gap-0.5", hot ? "flex" : "hidden")}>
                {!heldIn(o.branch) && (
                  <RowAction variant="picker" hot={hot} label={o.branch.current ? "Open a terminal here" : "Open a terminal in a new worktree"} onClick={act(onTerminal, localName(o.branch))}>
                    <SquareTerminal />
                  </RowAction>
                )}
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
                {!o.branch.current && !heldIn(o.branch) && !o.branch.remoteDefault && !guarded.has(o.branch.name) && !(o.branch.remote && accessOf(remoteOf(o.branch))?.push === false) && (
                  <RowAction variant="picker" hot={hot} label={o.branch.remote ? "Delete from the remote…" : o.branch.merged || upstream(o.branch) ? "Delete branch (merged)" : "Delete branch…"} onClick={act(() => onDelete(o.branch, upstream(o.branch)), o.branch.name)}>
                    <Trash2 />
                  </RowAction>
                )}
              </span>
              {heldIn(o.branch) ? (
                <Tip label={`${localName(o.branch)} is checked out in ${heldIn(o.branch)}. git keeps a branch in one worktree, so ↵ opens that one. Right-click for other ways.`}>
                  <span className={cn("max-w-40 truncate text-[10.5px]", hot ? "opacity-80" : "text-subtle")}>{heldHint(o.branch)}</span>
                </Tip>
              ) : (
                !hot && (
                  <span className="max-w-40 truncate text-[10.5px] text-subtle">
                    {o.branch.current
                      ? "current"
                      : o.branch.merged
                        ? `merged · ${relativeTime(o.branch.timestamp)}`
                        : upstream(o.branch)
                          ? `merged upstream · ${relativeTime(o.branch.timestamp)}`
                          : relativeTime(o.branch.timestamp)}
                  </span>
                )
              )}
            </span>
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
            role="combobox"
            aria-expanded
            aria-controls={listId}
            aria-activedescendant={index >= 0 ? `${listId}-${index}` : undefined}
            aria-autocomplete="list"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onKeyDown}
            placeholder="Find or create a branch…"
            className="h-full min-w-0 flex-1 bg-transparent font-mono text-[12px] outline-none placeholder:font-sans placeholder:text-subtle"
          />
        </div>
        {/* Like a native menu: the highlight leaves with the mouse; ↑↓ bring it back. */}
        <div ref={listRef} id={listId} role="listbox" aria-label="Branches" onMouseLeave={(e) => pointerMoved(e) && setIndex(-1)} className="max-h-[360px] min-h-0 flex-1 overflow-x-hidden overflow-y-auto p-1">
          {groups.length === 0 && options.length === 0 && (
            <div className="px-2 py-3 text-center text-[12px] text-subtle">
              No branches
            </div>
          )}
          {groups.map((g) => (
            <div key={g.name} role="group" aria-label={g.name}>
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
          <span className="min-w-0 flex-1 truncate">↑↓ navigate · ↵ {hotHeld ? (holderOf(hotHeld)?.prunable ? "prune, then switch" : heldHint(hotHeld)) : "switch"} · {renameKey ? `${renameKey} rename · ` : ""}⇧F10 or right-click for more</span>
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
          {landed && cleanable > 0 && (
            <Tip label={`Delete the ${cleanable} local branches ${squashed.length ? `merged into ${current ?? "HEAD"} or upstream` : `already merged into ${current ?? "HEAD"}`}`}>
              <button
                onClick={() => {
                  onCleanUp(stale, squashed);
                  close();
                }}
                className="shrink-0 rounded-sm px-1.5 py-0.5 hover:bg-hover focus-visible:bg-hover hover:text-foreground focus-visible:text-foreground"
              >
                Clean up {cleanable} merged
              </button>
            </Tip>
          )}
        </div>
      </PopoverContent>
    </Popover>
  );
}

