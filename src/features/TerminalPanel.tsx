import { ChevronDown, Columns2, FolderGit2, Plus, SquareTerminal, Trash2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Fragment, useEffect, useLayoutEffect, useRef } from "react";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@/components/ui/resizable";
import { Tip } from "@/components/ui/tooltip";
import type { Worktree } from "@/lib/api";
import { useCommands } from "@/lib/keybindings";
import {
  activateGroup,
  attachPane,
  clearFocused,
  closeFocused,
  closeGroup,
  dismissRestore,
  openTerminal,
  restoreSession,
  showWorktree,
  splitActive,
  type TerminalGroup,
  togglePanel,
  useTerminals,
} from "@/lib/terminals";
import { cn } from "@/lib/utils";
import { folderName } from "@/lib/worktrees";

/** What the workspace needs even while the panel is hidden: the panel shortcuts, and following the worktree that's open. */
export function useTerminalSetup(root: string) {
  useEffect(() => showWorktree(root), [root]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // ⌃` as in VS Code, ⌘J as its panel toggle.
      if ((e.ctrlKey && e.code === "Backquote") || (e.metaKey && !e.shiftKey && !e.altKey && e.code === "KeyJ")) {
        e.preventDefault();
        togglePanel(root);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [root]);

  useCommands({ "terminal.toggle": () => togglePanel(root), "terminal.new": () => openTerminal(root) });
}

interface Props {
  root: string;
  worktrees: Worktree[];
}

export function TerminalPanel({ root, worktrees }: Props) {
  const { groups, active } = useTerminals();
  const group = groups.find((g) => g.id === active) ?? null;
  const branchOf = (cwd: string) => worktrees.find((w) => w.path === cwd)?.branch ?? null;

  // Shortcuts while a terminal has focus. Stopping them here keeps ⌘W from closing a file tab.
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (!e.metaKey || e.altKey || e.ctrlKey) return;
    if (e.code === "KeyD" && !e.shiftKey) splitActive();
    else if (e.code === "KeyW") closeFocused();
    else if (e.code === "KeyK") clearFocused();
    else if (e.code === "KeyT") openTerminal(root);
    else return;
    e.preventDefault();
    e.stopPropagation();
  };

  const others = worktrees.filter((w) => w.path !== root && !w.bare && !w.prunable);

  return (
    <div className="flex h-full flex-col bg-background" onKeyDown={onKeyDown}>
      <div className="flex h-9 shrink-0 items-stretch border-b border-border bg-panel">
        <div role="tablist" data-scrollbar="none" className="flex min-w-0 flex-1 items-stretch overflow-x-auto overflow-y-hidden">
          {groups.map((g) => (
            <GroupTab key={g.id} group={g} active={g.id === active} here={g.panes[0].cwd === root} branch={branchOf(g.panes[0].cwd)} />
          ))}
        </div>
        <div className="flex shrink-0 items-center gap-0.5 px-1.5">
          <Tip label={`New terminal in ${folderName(root)}`} shortcut="⌘T">
            <Button variant="ghost" size="icon-sm" onClick={() => openTerminal(root)}>
              <Plus />
            </Button>
          </Tip>
          {others.length > 0 && (
            <DropdownMenu>
              <Tip label="New terminal in another worktree">
                <DropdownMenuTrigger asChild>
                  <Button variant="ghost" size="icon-sm" className="w-4">
                    <ChevronDown className="size-3" />
                  </Button>
                </DropdownMenuTrigger>
              </Tip>
              <DropdownMenuContent align="end" className="w-72">
                <DropdownMenuLabel>New terminal in worktree</DropdownMenuLabel>
                {others.map((w) => (
                  <DropdownMenuItem key={w.path} onSelect={() => openTerminal(w.path)}>
                    <FolderGit2 />
                    <span className="truncate">{folderName(w.path)}</span>
                    <span className="ml-auto truncate font-mono text-[11px] text-subtle">{w.branch ?? "detached"}</span>
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          )}
          <Tip label="Split terminal" shortcut="⌘D">
            <Button variant="ghost" size="icon-sm" onClick={splitActive} disabled={!group}>
              <Columns2 />
            </Button>
          </Tip>
          <Tip label="Kill terminal" shortcut="⌘W">
            <Button variant="ghost" size="icon-sm" onClick={closeFocused} disabled={!group}>
              <Trash2 />
            </Button>
          </Tip>
          <div className="mx-0.5 h-4 w-px bg-border-strong" />
          <Tip label="Hide terminal" shortcut="⌃`">
            <Button variant="ghost" size="icon-sm" onClick={() => togglePanel(root)}>
              <ChevronDown />
            </Button>
          </Tip>
        </div>
      </div>
      <div className="min-h-0 flex-1">
        {group && (
          // Re-keyed on the pane list so a split or close lays the panes out evenly again.
          <ResizablePanelGroup key={group.panes.map((p) => p.id).join()} orientation="horizontal">
            {group.panes.map((p, i) => (
              <Fragment key={p.id}>
                {i > 0 && <ResizableHandle className="bg-border" />}
                <ResizablePanel id={`pane-${p.id}`} minSize={160}>
                  <PaneView id={p.id} />
                </ResizablePanel>
              </Fragment>
            ))}
          </ResizablePanelGroup>
        )}
      </div>
    </div>
  );
}

function GroupTab({ group: g, active, here, branch }: { group: TerminalGroup; active: boolean; here: boolean; branch: string | null }) {
  const cwd = g.panes[0].cwd;
  const title = g.panes.find((p) => p.id === g.focused)?.title;
  return (
    <Tip label={title ? `${cwd} · ${title}` : cwd}>
      <div
        role="tab"
        aria-selected={active}
        onClick={() => activateGroup(g.id)}
        onAuxClick={(e) => e.button === 1 && closeGroup(g.id)}
        className={cn(
          "group relative flex max-w-64 shrink-0 cursor-pointer items-center gap-1.5 border-r border-border pr-1.5 pl-3 text-[12px] select-none",
          active ? "bg-background text-foreground" : "bg-panel text-muted-foreground hover:bg-hover hover:text-foreground",
        )}
      >
        {active && <span className="absolute inset-x-0 top-0 h-px bg-primary" />}
        {active && <span className="absolute inset-x-0 -bottom-px h-px bg-background" />}
        <SquareTerminal className={cn("size-3.5 shrink-0", here ? "text-primary" : "text-subtle")} />
        <span className="truncate">{folderName(cwd)}</span>
        {branch && <span className="min-w-0 truncate font-mono text-[10.5px] text-subtle">{branch}</span>}
        {g.panes.length > 1 && <span className="rounded-sm bg-elevated px-1 font-mono text-[10px] leading-4 text-muted-foreground">{g.panes.length}</span>}
        <button
          aria-label="Kill terminal"
          onClick={(e) => {
            e.stopPropagation();
            closeGroup(g.id);
          }}
          className={cn("flex size-5 items-center justify-center rounded-sm text-subtle hover:bg-active hover:text-foreground", !active && "opacity-0 group-hover:opacity-100")}
        >
          <X className="size-3" />
        </button>
      </div>
    </Tip>
  );
}

/** Offers last run's terminals. Asked, not automatic: each one starts a shell. */
export function TerminalRestoreOffer() {
  const { restorable } = useTerminals();
  if (!restorable) return null;
  const cwds = restorable.groups.flatMap((g) => g.panes.map((p) => p.cwd));
  const folders = [...new Set(cwds.map(folderName))];
  return (
    <div className="pointer-events-auto fixed right-4 bottom-10 z-50 flex w-96 gap-3 rounded-md border border-border-strong bg-elevated p-3 shadow-lg shadow-black/50 animate-in fade-in-0 slide-in-from-bottom-2">
      <SquareTerminal className="mt-0.5 size-4 shrink-0 text-primary" />
      <div className="min-w-0 flex-1">
        <div className="text-[12px] font-medium">
          Restore {cwds.length} terminal{cwds.length === 1 ? "" : "s"} from last session?
        </div>
        <div className="mt-1 truncate text-[11.5px] text-muted-foreground">{folders.join(", ")}</div>
        <div className="mt-2.5 flex gap-2">
          <Button size="sm" onClick={restoreSession}>
            Restore
          </Button>
          <Button size="sm" variant="secondary" onClick={dismissRestore}>
            Dismiss
          </Button>
        </div>
      </div>
    </div>
  );
}

function PaneView({ id }: { id: number }) {
  const ref = useRef<HTMLDivElement>(null);
  // Layout effect: the pane is in place before paint, so focusing it next frame works.
  useLayoutEffect(() => attachPane(id, ref.current!), [id]);
  // Inset from the edges like the code view's text; the scrollbar keeps the right edge.
  return <div ref={ref} className="h-full w-full pt-2 pb-1 pl-3" />;
}
