import { Columns2, Eraser, ListX, Pencil, Rows2, SquareTerminal, Trash2, X } from "lucide-react";
import { memo, useState } from "react";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuShortcut, ContextMenuTrigger } from "@/components/ui/context-menu";
import { Tip } from "@/components/ui/tooltip";
import { useShortcut } from "@/lib/commands/keybindings";
import {
  activateGroup,
  clearFocused,
  closeGroup,
  closeOtherGroups,
  renameGroup,
  splitActive,
  type TerminalGroup,
} from "@/lib/terminal/terminals";
import { cn } from "@/lib/utils";
import { folderName } from "@/lib/path";
import { type Hue, hueColor } from "@/lib/git/worktrees";
import { NameInput } from "@/components/NameInput";
import { StatusDot } from "@/components/StatusDot";
import { lookLabel, mostUrgent, paneLook } from "@/lib/terminal/agentLook";

// Memoized: every title a program sets re-renders the panel, and the other tabs keep their group object.
export const GroupTab = memo(function GroupTab({
  group: g,
  active,
  here,
  branch,
  hue,
  alone,
}: {
  group: TerminalGroup;
  active: boolean;
  here: boolean;
  branch: string | null;
  /** Its worktree's color. */
  hue: Hue | null;
  alone: boolean;
}) {
  const [renaming, setRenaming] = useState(false);
  const cwd = g.panes[0].cwd;
  const title = g.panes.find((p) => p.id === g.focused)?.title;
  const where = title ? `${cwd} · ${title}` : cwd;
  // The tab's tooltip says the dot's state too.
  const look = mostUrgent(g.panes.map(paneLook));
  const agent = g.panes.find((p) => look && paneLook(p) === look)?.agent?.name;
  const label = (g.name ? `${g.name} · ${where}` : where) + (look ? ` · ${lookLabel(look, agent)}` : "");
  // "/" has no name of its own.
  const shown = g.name ?? (folderName(cwd) || cwd);
  const [splitKey, splitDownKey, clearKey] = [useShortcut("terminal.split"), useShortcut("terminal.splitDown"), useShortcut("terminal.clear")];
  // Split and Clear act on the open tab's focused pane.
  const inTab = (run: () => void) => () => {
    activateGroup(g.id);
    run();
  };
  const tab = (
    <div
      role="tab"
      aria-selected={active}
      aria-label={[label, branch, g.panes.length > 1 && `${g.panes.length} panes`].filter(Boolean).join(" · ")}
      tabIndex={active ? 0 : -1}
      // A double-click renames: its second click leaves focus for the name field.
      onClick={(e) => activateGroup(g.id, e.detail < 2)}
      onDoubleClick={() => setRenaming(true)}
      onAuxClick={(e) => e.button === 1 && void closeGroup(g.id)}
      className={cn(
        "group relative flex max-w-64 shrink-0 cursor-pointer items-center gap-1.5 border-r border-border pr-1.5 pl-3 text-[12px] outline-none select-none focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset",
        active ? "bg-background text-foreground" : "bg-panel text-muted-foreground hover:bg-hover hover:text-foreground focus:bg-hover focus:text-foreground",
      )}
    >
      {active && <span className="absolute inset-x-0 top-0 h-0.5 bg-primary" />}
      {active && <span className="absolute inset-x-0 -bottom-px h-px bg-background" />}
      {hue && <span className="absolute inset-y-2 left-0 w-0.5 rounded-r-full" style={{ background: hueColor(hue) }} />}
      <SquareTerminal className={cn("size-3.5 shrink-0", here ? "text-primary" : "text-subtle")} style={hue ? { color: hueColor(hue) } : undefined} />
      {renaming ? (
        <NameInput
          initial={shown}
          onDone={(name, refocus) => {
            setRenaming(false);
            // Left as it was, the folder's name isn't a name the user gave.
            if (name !== null && name.trim() !== shown) renameGroup(g.id, name);
            if (refocus) activateGroup(g.id);
          }}
        />
      ) : (
        <span className="truncate">{shown}</span>
      )}
      {branch && <span className="min-w-0 truncate font-mono text-[10.5px] text-subtle">{branch}</span>}
      <StatusDot look={look} />
      {g.panes.length > 1 && <span className="rounded-sm bg-elevated px-1 font-mono text-[10px] leading-4 text-muted-foreground">{g.panes.length}</span>}
      <button
        aria-label="Kill terminal"
        // Off the Tab order: ⌫ on the tab kills it.
        tabIndex={-1}
        onClick={(e) => {
          e.stopPropagation();
          void closeGroup(g.id);
        }}
        className={cn("flex size-5 items-center justify-center rounded-sm text-subtle hover:bg-active focus-visible:bg-active hover:text-foreground focus-visible:text-foreground", !active && "opacity-0 group-focus-within:opacity-100 group-hover:opacity-100")}
      >
        <X className="size-3" />
      </button>
    </div>
  );
  return (
    <ContextMenu>
      {/* The tooltip opens on keyboard focus too; the label says the same for a screen reader. */}
      <Tip label={label}>
        <ContextMenuTrigger asChild>{tab}</ContextMenuTrigger>
      </Tip>
      <ContextMenuContent>
        {/* Focus going back to the tab would end the rename. */}
        <ContextMenuItem keepFocus onSelect={() => setRenaming(true)}>
          <Pencil /> Rename…
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem onSelect={inTab(() => void splitActive("row"))}>
          <Columns2 /> Split Right{active && splitKey && <ContextMenuShortcut>{splitKey}</ContextMenuShortcut>}
        </ContextMenuItem>
        <ContextMenuItem onSelect={inTab(() => void splitActive("col"))}>
          <Rows2 /> Split Down{active && splitDownKey && <ContextMenuShortcut>{splitDownKey}</ContextMenuShortcut>}
        </ContextMenuItem>
        <ContextMenuItem onSelect={inTab(clearFocused)}>
          <Eraser /> Clear{active && clearKey && <ContextMenuShortcut>{clearKey}</ContextMenuShortcut>}
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem onSelect={() => void closeGroup(g.id)}>
          <Trash2 /> Kill Terminal
        </ContextMenuItem>
        <ContextMenuItem disabled={alone} onSelect={() => void closeOtherGroups(g.id)}>
          <ListX /> Kill Others
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
});
