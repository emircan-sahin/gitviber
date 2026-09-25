import { Check, ChevronsUpDown, Copy, CornerUpLeft, Eraser, FolderGit2, FolderOpen, GitBranch, GitMerge, Lock, LockOpen, Pencil, SquareTerminal, Trash2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuShortcut, ContextMenuTrigger } from "@/components/ui/context-menu";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Tip } from "@/components/ui/tooltip";
import { api, type Branch, type Worktree, type WorktreeState } from "@/lib/api";
import { REVEAL_LABEL } from "@/lib/platform";
import { matchesCommand, useCommands, useShortcut } from "@/lib/commands/keybindings";
import { pointerMoved } from "@/lib/ui/pointer";
import { isMenuKey, openRowMenu } from "@/lib/ui/useListNav";
import { cn } from "@/lib/utils";
import { plural, relativeTime } from "@/lib/format";
import { shortPath } from "@/lib/git/worktrees";
import { folderName } from "@/lib/path";
import { copyText } from "@/lib/app/clipboard";
import { revealProject } from "@/lib/app/openIn";
import { RowAction } from "@/components/RowAction";
import { useWorktreeDialog } from "./WorktreeDialogs";
import { usePickerIndex } from "@/hooks/usePickerIndex";

interface Props {
  worktrees: Worktree[];
  /** For each worktree's last commit time. */
  branches: Branch[];
  /** Opens a worktree in this window (the regular open-repo flow). */
  onOpen: (path: string) => void;
  onTerminal: (path: string) => void;
  /** Merges a branch into the current one. Git allows it while another worktree has it out. */
  onMerge: (branch: string) => void;
  /** Deletes a linked worktree (asks first), or prunes one whose folder is gone. */
  onRemove: (w: Worktree) => void;
  /** Renames a worktree's branch, and its folder with it. */
  onRename: (w: Worktree) => void;
  /** Asks for a reason, then locks it. */
  onLock: (w: Worktree) => void;
  onUnlock: (w: Worktree) => void;
  onNew: () => void;
}

/**
 * `git worktree list` as a switcher. Always shown, even with only the main worktree, so
 * the feature is found at all; then it says how to make one.
 * Rows lead with the branch, the name people know a worktree by; the folder comes second.
 * In a linked worktree it names it and offers the way back to the main one.
 */
export function WorktreePicker({ worktrees, branches, onOpen, onTerminal, onMerge, onRemove, onRename, onLock, onUnlock, onNew }: Props) {
  const [open, setOpen] = useState(false);
  const [list, setList] = useState(worktrees);
  const { index, setIndex, move } = usePickerIndex(list.length);
  // A `git status` and two rev-lists per worktree: fetched when the menu opens, never before.
  const [states, setStates] = useState<Record<string, WorktreeState>>({});
  const listRef = useRef<HTMLDivElement>(null);
  const afterClose = useRef<(() => void) | null>(null);
  const dialog = useWorktreeDialog();
  useEffect(() => {
    if (dialog) setOpen(false);
  }, [dialog]);
  const renameKey = useShortcut("worktree.rename");
  useCommands({ "git.switchWorktree": () => setOpen(true) });

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
            .worktreeState(w.path)
            .then(
              (s) => void (live && setStates((c) => ({ ...c, [w.path]: s }))),
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

  if (!list.length) return null;
  const extra = list.filter((w) => !w.main).length;
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
  const merge = then((w) => w.branch && onMerge(w.branch));
  // Dialogs open once the picker is gone: closing hands the focus back to its trigger, which
  // would take it from the dialog and show the trigger's tooltip over it.
  const thenDialog = (fn: (w: Worktree) => void) => (w: Worktree) => {
    afterClose.current = () => fn(w);
    setOpen(false);
  };
  const remove = then(onRemove);
  const rename = thenDialog(onRename);
  const lock = thenDialog((w) => (w.locked ? onUnlock(w) : onLock(w)));
  // revealProject, not revealPath: that one only reaches inside the open worktree.
  const actions = { pick, terminal, merge, rename, lock, remove, reveal: then((w) => void revealProject(w.path)), copy: then((w) => void copyText(w.path, "Path copied")) };

  // The hot row's actions, which the mouse finds on the row.
  const hot = list[index];
  const can = {
    terminal: !!hot && !hot.prunable && !hot.bare,
    merge: !!hot && !!current?.branch && !!hot.branch && !hot.current && !!states[hot.path]?.commits,
    rename: !!hot && !!hot.branch && !hot.prunable,
    remove: !!hot && !hot.main && !hot.current,
  };
  const onKeyDown = (e: React.KeyboardEvent) => {
    const n = e.nativeEvent;
    if (matchesCommand("worktree.openTerminal", n) && can.terminal) terminal(hot);
    else if (matchesCommand("worktree.merge", n) && can.merge) merge(hot);
    else if (matchesCommand("worktree.rename", n) && can.rename) rename(hot);
    else if (matchesCommand("worktree.remove", n) && can.remove) remove(hot);
    else if (isMenuKey(e)) {
      const row = listRef.current?.querySelector<HTMLElement>(`[data-option="${index}"]`);
      if (row) openRowMenu(row);
    } else if (e.metaKey || e.ctrlKey || e.altKey) return;
    else if (e.key === "ArrowDown") move(1);
    else if (e.key === "ArrowUp") move(-1);
    else if (e.key === "Enter" && hot && usable(hot)) pick(hot);
    else return;
    e.preventDefault();
  };

  return (
    <>
      {/* Never over one of its own dialogs, whatever the order things closed in. */}
      <Popover open={open && !dialog} onOpenChange={setOpen}>
        <Tip label={linked ? `In worktree ${folderName(current.path)} · switch worktree` : extra === 0 ? "Worktrees" : `${plural(extra, "worktree")} besides the main one · switch worktree`}>
          <PopoverTrigger asChild>
            <button
              aria-label={linked ? `Worktree ${folderName(current.path)}, switch worktree` : extra === 0 ? "Worktrees" : `Switch worktree (${extra} besides the main one)`}
              className={cn(
                "flex h-7 max-w-56 min-w-0 shrink-0 items-center gap-1.5 rounded-md px-2 hover:bg-hover focus-visible:bg-hover data-[state=open]:bg-active",
                linked && "bg-primary/10",
              )}
            >
              <FolderGit2 className={cn("size-3.5 shrink-0", linked ? "text-primary" : "text-subtle")} />
              {linked ? (
                <span className="truncate font-mono text-[12px]">{folderName(current.path)}</span>
              ) : (
                // Named, so it's found even before there are any. Counted as people count them:
                // the extra checkouts, not the main folder git also lists.
                <span className="text-[12px]">
                  Worktrees{extra > 0 && <span className="text-muted-foreground"> ({extra})</span>}
                </span>
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
          onCloseAutoFocus={(e) => {
            const run = afterClose.current;
            if (!run) return;
            afterClose.current = null;
            e.preventDefault();
            run();
          }}
        >
          {/* Like a native menu: the highlight leaves with the mouse; ↑↓ bring it back. */}
          <div ref={listRef} tabIndex={-1} onMouseLeave={(e) => pointerMoved(e) && setIndex(-1)} className="max-h-[360px] min-h-0 flex-1 overflow-x-hidden overflow-y-auto p-1 outline-none">
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
                state={states[w.path]}
                into={current?.branch ?? null}
                renameKey={renameKey}
                onHover={setIndex}
                actions={actions}
                // Back to the list, not the row, so ↑↓ and the row keys keep working.
                onMenuClosed={() => listRef.current?.isConnected && listRef.current.focus()}
              />
            ))}
            {list.length === 1 && (
              <div className="px-2 py-3 text-center text-[11.5px] leading-relaxed text-subtle">
                No other worktrees yet. A worktree checks out another branch in its own folder, side by side with this one. Make one with New worktree below.
              </div>
            )}
          </div>
          <div className="flex shrink-0 items-start gap-2 border-t border-border px-2.5 py-1.5 text-[10.5px] text-subtle">
            <span className="min-w-0 flex-1">
              ↑↓ navigate · ↵ open here{can.terminal && " · T terminal"}
              {can.merge && " · M merge"}
              {can.rename && renameKey && ` · ${renameKey} rename`}
              {can.remove && (hot.prunable ? " · ⌫ prune" : " · ⌫ remove")}
              {hot && " · ⇧F10 or right-click for more"}
              {/* The lock's reason is otherwise only in a tooltip, out of the keyboard's reach. */}
              {hot?.locked && (
                <>
                  <br />
                  {hot.inUse ? "In use" : "Locked"}
                  {hot.lockReason ? `: ${hot.lockReason}` : ""}
                </>
              )}
            </span>
            <Tip label="A new branch in its own folder">
              <button
                onClick={() => {
                  afterClose.current = onNew;
                  setOpen(false);
                }}
                className="shrink-0 rounded-sm px-1.5 py-0.5 hover:bg-hover focus-visible:bg-hover hover:text-foreground focus-visible:text-foreground"
              >
                New worktree…
              </button>
            </Tip>
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

type RowActions = Record<"pick" | "terminal" | "merge" | "rename" | "lock" | "remove" | "reveal" | "copy", (w: Worktree) => void>;

function WorktreeRow({
  i,
  w,
  hot,
  usable,
  main,
  time,
  state,
  into,
  renameKey,
  onHover,
  actions: a,
  onMenuClosed,
}: {
  i: number;
  w: Worktree;
  hot: boolean;
  usable: boolean;
  main: string;
  time: number | undefined;
  state: WorktreeState | undefined;
  /** The current worktree's branch; null when detached. */
  into: string | null;
  renameKey: string | undefined;
  onHover: (i: number) => void;
  actions: RowActions;
  onMenuClosed: () => void;
}) {
  const branch = w.branch ?? (w.bare ? "bare" : `detached @ ${w.head ?? "?"}`);
  const act = (fn: (w: Worktree) => void) => (e: React.MouseEvent) => {
    e.stopPropagation();
    fn(w);
  };
  const onDisk = !w.prunable && !w.bare;
  const can = {
    merge: !!into && !!w.branch && !w.current && !!state?.commits,
    rename: !!w.branch && !w.prunable,
    // git won't lock the main worktree.
    lock: !w.main && !w.bare,
    remove: !w.main && !w.current,
  };
  const mergeLabel = `Merge into ${into}${state?.uncommitted ? ` · its ${state.uncommitted} uncommitted ${state.uncommitted === 1 ? "change stays" : "changes stay"} behind` : ""}`;
  const row = (
    <div
      data-option={i}
      role="option"
      aria-selected={hot}
      aria-disabled={!usable}
      onMouseMove={(e) => pointerMoved(e) && onHover(i)}
      onClick={() => usable && a.pick(w)}
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
          {/* A worktree away from the project has a long path; rtl cuts its start, not its folder (LRMs: see the New worktree dialog). */}
          <span dir="rtl" className="truncate text-left">
            {`\u200e${w.main ? folderName(w.path) : shortPath(w.path, main)}\u200e`}
          </span>
        </div>
      </div>
      {/* Mounted on every row, shown on the hot one, like the branch picker's actions. */}
      <span className={cn("shrink-0 gap-0.5", hot && !w.bare ? "flex" : "hidden")}>
        {onDisk && (
          <RowAction variant="picker" hot={hot} label="Open a terminal here" onClick={act(a.terminal)}>
            <SquareTerminal />
          </RowAction>
        )}
        {can.merge && (
          <RowAction variant="picker" hot={hot} label={mergeLabel} onClick={act(a.merge)}>
            <GitMerge />
          </RowAction>
        )}
        {can.rename && (
          <RowAction variant="picker" hot={hot} label="Rename…" onClick={act(a.rename)}>
            <Pencil />
          </RowAction>
        )}
        {can.lock && (
          <RowAction variant="picker" hot={hot} label={w.locked ? `Unlock${w.lockReason ? ` (${w.lockReason})` : ""}` : "Lock: keep it from being pruned, moved or removed…"} onClick={act(a.lock)}>
            {w.locked ? <LockOpen /> : <Lock />}
          </RowAction>
        )}
        {can.remove && (
          <RowAction variant="picker" hot={hot} label={w.prunable ? "Prune: its folder is gone, drop it from the list" : "Remove worktree…"} onClick={act(a.remove)}>
            {w.prunable ? <Eraser /> : <Trash2 />}
          </RowAction>
        )}
      </span>
      {(!hot || w.bare) && (
        <span className={cn("flex max-w-36 shrink-0 flex-col items-end text-[10.5px] leading-4", hot ? "opacity-80" : "text-subtle")}>
          <span className="max-w-full truncate">{w.prunable ? "missing" : w.bare ? "" : time ? relativeTime(time) : w.current ? "current" : ""}</span>
          {state && <StateLabel state={state} hot={hot} />}
        </span>
      )}
    </div>
  );
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{row}</ContextMenuTrigger>
      <ContextMenuContent
        onCloseAutoFocus={(e) => {
          e.preventDefault();
          onMenuClosed();
        }}
      >
        {usable && (
          <ContextMenuItem onSelect={() => a.pick(w)}>
            <FolderGit2 /> Open here
          </ContextMenuItem>
        )}
        {onDisk && (
          <ContextMenuItem onSelect={() => a.terminal(w)}>
            <SquareTerminal /> Open a terminal here
          </ContextMenuItem>
        )}
        {can.merge && (
          <ContextMenuItem onSelect={() => a.merge(w)}>
            <GitMerge /> Merge into {into}
          </ContextMenuItem>
        )}
        {can.rename && (
          <ContextMenuItem onSelect={() => a.rename(w)}>
            <Pencil /> Rename…{renameKey && <ContextMenuShortcut>{renameKey}</ContextMenuShortcut>}
          </ContextMenuItem>
        )}
        {can.lock && (
          <ContextMenuItem onSelect={() => a.lock(w)}>
            {w.locked ? <LockOpen /> : <Lock />} {w.locked ? "Unlock" : "Lock…"}
          </ContextMenuItem>
        )}
        {(usable || onDisk || can.merge || can.rename || can.lock) && <ContextMenuSeparator />}
        {onDisk && (
          <ContextMenuItem onSelect={() => a.reveal(w)}>
            <FolderOpen /> {REVEAL_LABEL}
          </ContextMenuItem>
        )}
        <ContextMenuItem onSelect={() => a.copy(w)}>
          <Copy /> Copy path
        </ContextMenuItem>
        {can.remove && (
          <>
            <ContextMenuSeparator />
            <ContextMenuItem className={w.prunable ? undefined : "text-destructive"} onSelect={() => a.remove(w)}>
              {w.prunable ? (
                <>
                  <Eraser /> Prune
                </>
              ) : (
                <>
                  <Trash2 /> Remove worktree…
                </>
              )}
            </ContextMenuItem>
          </>
        )}
      </ContextMenuContent>
    </ContextMenu>
  );
}

/** Uncommitted files and unmerged commits side by side; "merged" or "no changes" only when neither. */
function StateLabel({ state: s, hot }: { state: WorktreeState; hot: boolean }) {
  const parts: [string, string][] = [];
  if (s.uncommitted) parts.push([plural(s.uncommitted, "change"), "text-removed"]);
  if (s.commits) parts.push([plural(s.commits, "commit"), "text-added"]);
  if (!parts.length) parts.push(s.merged ? ["merged", "text-renamed"] : ["no changes", "text-subtle"]);
  return (
    <span className="max-w-full truncate">
      {parts.map(([text, tone], i) => (
        <span key={text} className={cn(!hot && tone)}>
          {i > 0 && " · "}
          {text}
        </span>
      ))}
    </span>
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
