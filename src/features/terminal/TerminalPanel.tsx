import { open as pickFolder } from "@tauri-apps/plugin-dialog";
import { ChevronDown, ClipboardPaste, Columns2, Copy, Eraser, FolderGit2, FolderOpen, ListX, Maximize2, Minimize2, Pencil, Plus, Rows2, SquareTerminal, TextSelect, Trash2, X, ZoomIn, ZoomOut } from "lucide-react";
import { FindBox, useFindBox } from "@/components/FindBox";
import { type FindOptions, NO_OPTIONS } from "@/lib/ui/findQuery";
import { Button } from "@/components/ui/button";
import { Fragment, memo, useEffect, useLayoutEffect, useRef, useState } from "react";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuShortcut, ContextMenuTrigger } from "@/components/ui/context-menu";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@/components/ui/resizable";
import { Tip } from "@/components/ui/tooltip";
import { api, type Worktree } from "@/lib/api";
import { COMMANDS, commandIn, runCommand, useCommands, useShortcut } from "@/lib/commands/keybindings";
import { type Layout } from "@/lib/terminal/layout";
import { stepTerminalFont, useSettings } from "@/lib/settings";
import { focusMovedTab, focusTab, tabMove } from "@/lib/ui/useListNav";
import { focusedPanel, focusPanel } from "@/lib/ui/panels";
import {
  activateGroup,
  attachPane,
  clearFocused,
  closeFocused,
  closeGroup,
  clearFind,
  closeOtherGroups,
  copyLastOutput,
  copyPaneSelection,
  dismissRestore,
  endFind,
  findInTerminal,
  focusActive,
  focusTerminalPane,
  focusToward,
  killPane,
  moveGroup,
  openTerminal,
  paneDir,
  paneMenuState,
  paneTakesMouse,
  pasteIntoPane,
  renameGroup,
  renamePane,
  resizeSplit,
  restoreSession,
  selectLastOutput,
  showWorktree,
  splitActive,
  stepPane,
  TERMINAL_COMMANDS,
  type TerminalGroup,
  toggleMaximize,
  toggleZoom,
  togglePanel,
  unmaximize,
  useTerminals,
} from "@/lib/terminal/terminals";
import { cn } from "@/lib/utils";
import { createStore } from "@/lib/store";
import { folderName, parentFolder } from "@/lib/path";
import { NameInput } from "@/components/NameInput";
import { NeedsYouDot } from "@/components/NeedsYouDot";
import { plural } from "@/lib/format";
import { ProjectTile } from "@/features/projects/ProjectList";

/** What the workspace needs even while the panel is hidden: the panel shortcuts, and following the worktree that's open. */
export function useTerminalSetup(root: string) {
  useEffect(() => showWorktree(root), [root]);
  // Another worktree's workspace starts with its code in view.
  useEffect(() => unmaximize, [root]);

  // Maximize and zoom from the palette too, where they open the panel first.
  useCommands({
    "terminal.toggle": () => toggle(root),
    "terminal.new": () => openTerminal(root),
    "terminal.toggleMaximize": () => toggleMaximize(root),
    "terminal.zoomPane": () => toggleZoom(root),
    "terminal.fontZoomIn": () => stepTerminalFont(1),
    "terminal.fontZoomOut": () => stepTerminalFont(-1),
    "terminal.fontZoomReset": () => stepTerminalFont(0),
  });
}

/** Opening focuses the terminal (togglePanel does); hiding it while it has focus leaves focus to the code view. */
function toggle(root: string) {
  const had = document.activeElement !== document.body && focusedPanel() === "terminal";
  togglePanel(root);
  if (had) focusPanel("code");
}

interface Props {
  root: string;
  worktrees: Worktree[];
  /** The saved projects other than this one: a terminal there leaves the window on this one. */
  projects: string[];
}

/** Any folder: a terminal in it, the window staying on this project. */
async function chooseFolder() {
  const dir = await pickFolder({ directory: true, title: "New terminal in folder" });
  if (typeof dir === "string") openTerminal(dir);
}

// Tabs outside this repo's worktrees: each one's branch, by tab id, read once as it first shows.
// Nothing watches that repo, so a checkout there leaves it stale (a new tab reads it again).
const outsideBranches = new Map<number, string | null>();

export function TerminalPanel({ root, worktrees, projects }: Props) {
  const { groups, active, maximized, zoomed: zoomOn } = useTerminals();
  const group = groups.find((g) => g.id === active) ?? null;
  // Only a tab with panes to hide shows one alone.
  const zoomed = zoomOn && !!group && group.panes.length > 1;
  const zoomKey = useShortcut("terminal.zoomPane");
  const [, branchRead] = useState(0);
  const { terminalInactiveDim } = useSettings();
  // The panel's keys (onKeyDown), in the palette too for whoever doesn't know them; maximize is useTerminalSetup's.
  useCommands({
    "terminal.split": () => void splitActive("row"),
    "terminal.splitDown": () => void splitActive("col"),
    "terminal.clear": () => void clearFocused(),
    "terminal.close": () => void closeFocused(),
    "terminal.prevPane": () => stepPane(-1),
    "terminal.nextPane": () => stepPane(1),
    "terminal.focusLeft": () => focusToward("left"),
    "terminal.focusRight": () => focusToward("right"),
    "terminal.focusUp": () => focusToward("up"),
    "terminal.focusDown": () => focusToward("down"),
    // Only a pane with a header on screen: a split, not zoomed.
    "terminal.renamePane": () => group && group.panes.length > 1 && !zoomed && renamingPane.set(group.focused),
  });
  const branchOf = (g: TerminalGroup) => {
    const w = worktrees.find((x) => x.path === g.panes[0].cwd);
    return w ? w.branch : (outsideBranches.get(g.id) ?? null);
  };
  // Worktrees not listed yet: every tab would look outside.
  useEffect(() => {
    if (!worktrees.length) return;
    for (const id of outsideBranches.keys()) if (!groups.some((g) => g.id === id)) outsideBranches.delete(id);
    for (const g of groups) {
      const cwd = g.panes[0].cwd;
      if (outsideBranches.has(g.id) || worktrees.some((w) => w.path === cwd)) continue;
      outsideBranches.set(g.id, null);
      api.folderBranch(cwd).then(
        (b) => {
          if (!b || !outsideBranches.has(g.id)) return;
          outsideBranches.set(g.id, b);
          branchRead((n) => n + 1);
        },
        () => {},
      );
    }
  }, [groups, worktrees]);

  // Shortcuts while a terminal has focus. Stopping them here keeps ⌘W from closing a file tab.
  const onKeyDown = (e: React.KeyboardEvent) => {
    const id = commandIn(TERMINAL_COMMANDS, e.nativeEvent);
    // Find's box keeps ↵ (it took it: next match) and ⇧⌘← / ⇧⌘→ (select to the line's ends).
    if (!id || e.defaultPrevented || (e.target instanceof HTMLInputElement && e.shiftKey && e.key.startsWith("Arrow"))) return;
    e.preventDefault();
    e.stopPropagation();
    // Held down, ⌘↵ would flicker and ⌘D open a shell per repeat.
    if (!(e.repeat && COMMANDS.some((c) => c.id === id && "noRepeat" in c))) runCommand(id);
  };

  const others = worktrees.filter((w) => w.path !== root && !w.bare && !w.prunable);

  // The tabs: ←/→ (Home/End) switch terminals and stay on the tabs, ⌥←/⌥→ reorder them
  // (tab.moveLeft / tab.moveRight), ↵ or Space goes into the terminal, ⌫ kills it.
  const onTabKey = (e: React.KeyboardEvent<HTMLDivElement>) => {
    const el = e.target instanceof HTMLElement && e.target.getAttribute("role") === "tab" ? e.target : null;
    const shift = tabMove(e);
    if (!el || (!shift && (e.altKey || e.metaKey || e.ctrlKey || e.shiftKey))) return;
    const els = [...e.currentTarget.querySelectorAll<HTMLElement>('[role="tab"]')];
    const i = els.indexOf(el);
    const to = ({ ArrowLeft: i - 1, ArrowRight: i + 1, Home: 0, End: els.length - 1 } as Record<string, number>)[e.key];
    if (shift) {
      if (!groups[i + shift]) return;
      moveGroup(groups[i].id, shift);
      focusMovedTab(el);
    } else if (to !== undefined) {
      const at = Math.max(0, Math.min(els.length - 1, to));
      activateGroup(groups[at].id, false);
      focusTab(els[at]);
    } else if (e.key === "Enter" || e.key === " ") activateGroup(groups[i].id);
    else if (e.key === "Backspace" || e.key === "Delete") {
      const next = els[i + 1] ?? els[i - 1];
      void closeGroup(groups[i].id).then((killed) => killed && next?.focus());
    } else return;
    e.preventDefault();
  };
  // ⌘1–⌘9 or the next tab, pressed on the tabs: focus goes to the tab they opened.
  const tablist = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (tablist.current?.contains(document.activeElement)) tablist.current.querySelector<HTMLElement>('[aria-selected="true"]')?.focus();
  }, [active]);

  return (
    <div className="flex h-full flex-col bg-background" onKeyDown={onKeyDown}>
      <div className="flex h-9 shrink-0 items-stretch border-b border-border bg-panel">
        <div ref={tablist} role="tablist" aria-label="Terminals" onKeyDown={onTabKey} data-scrollbar="none" className="flex min-w-0 flex-1 items-stretch overflow-x-auto overflow-y-hidden">
          {groups.map((g) => (
            <GroupTab key={g.id} group={g} active={g.id === active} here={g.panes[0].cwd === root} branch={branchOf(g)} alone={groups.length === 1} />
          ))}
        </div>
        <div className="flex shrink-0 items-center gap-0.5 px-1.5">
          <Tip label={`New terminal in ${folderName(root)}`} shortcut={useShortcut("terminal.new")}>
            <Button variant="ghost" size="icon-sm" onClick={() => openTerminal(root)}>
              <Plus />
            </Button>
          </Tip>
          <DropdownMenu>
            <Tip label="New terminal in another folder">
              <DropdownMenuTrigger asChild>
                <Button variant="ghost" size="icon-sm" className="w-4">
                  <ChevronDown className="size-3" />
                </Button>
              </DropdownMenuTrigger>
            </Tip>
            <DropdownMenuContent align="end" className="w-72">
              {others.length > 0 && (
                <>
                  <DropdownMenuLabel>New terminal in worktree</DropdownMenuLabel>
                  {others.map((w) => (
                    <DropdownMenuItem key={w.path} onSelect={() => openTerminal(w.path)}>
                      <FolderGit2 />
                      <span className="truncate">{folderName(w.path)}</span>
                      <span className="ml-auto truncate font-mono text-[11px] text-subtle">{w.branch ?? "detached"}</span>
                    </DropdownMenuItem>
                  ))}
                  <DropdownMenuSeparator />
                </>
              )}
              {projects.length > 0 && (
                <>
                  <DropdownMenuLabel>New terminal in project</DropdownMenuLabel>
                  {projects.map((p) => (
                    <DropdownMenuItem key={p} title={p} onSelect={() => openTerminal(p)}>
                      <ProjectTile name={folderName(p)} />
                      <span className="truncate">{folderName(p)}</span>
                      {/* Where it is, as a worktree shows its branch: same-named projects tell apart. */}
                      <span className="ml-auto truncate text-[11px] text-subtle">{folderName(parentFolder(p))}</span>
                    </DropdownMenuItem>
                  ))}
                  <DropdownMenuSeparator />
                </>
              )}
              <DropdownMenuItem onSelect={() => void chooseFolder()}>
                <FolderOpen /> Choose Folder…
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
          <Tip label="Split right" shortcut={useShortcut("terminal.split")}>
            <Button variant="ghost" size="icon-sm" onClick={() => void splitActive("row")} disabled={!group}>
              <Columns2 />
            </Button>
          </Tip>
          <Tip label="Split down" shortcut={useShortcut("terminal.splitDown")}>
            <Button variant="ghost" size="icon-sm" onClick={() => void splitActive("col")} disabled={!group}>
              <Rows2 />
            </Button>
          </Tip>
          <Tip label="Kill terminal" shortcut={useShortcut("terminal.close")}>
            <Button variant="ghost" size="icon-sm" onClick={() => void closeFocused()} disabled={!group}>
              <Trash2 />
            </Button>
          </Tip>
          <div className="mx-0.5 h-4 w-px bg-border-strong" />
          {group && group.panes.length > 1 && (
            <Tip label={zoomed ? "Show all panes" : "Zoom pane"} shortcut={zoomKey}>
              <Button variant="ghost" size="icon-sm" aria-pressed={zoomed} onClick={() => toggleZoom(root)}>
                {zoomed ? <ZoomOut /> : <ZoomIn />}
              </Button>
            </Tip>
          )}
          <Tip label={maximized ? "Exit maximized terminal" : "Maximize terminal"} shortcut={useShortcut("terminal.toggleMaximize")}>
            <Button variant="ghost" size="icon-sm" aria-pressed={maximized} onClick={() => toggleMaximize(root)}>
              {maximized ? <Minimize2 /> : <Maximize2 />}
            </Button>
          </Tip>
          <Tip label="Hide terminal" shortcut={useShortcut("terminal.toggle")}>
            <Button variant="ghost" size="icon-sm" onClick={() => toggle(root)}>
              <ChevronDown />
            </Button>
          </Tip>
        </div>
      </div>
      <div className="relative min-h-0 flex-1">
        <TerminalFind />
        {group && <LayoutView key={shape(zoomed ? group.focused : group.layout)} group={group} node={zoomed ? group.focused : group.layout} focused={group.focused} dim={group.panes.length > 1 ? terminalInactiveDim / 100 : 0} />}
      </div>
    </div>
  );
}

// Memoized: every title a program sets re-renders the panel, and the other tabs keep their group object.
const GroupTab = memo(function GroupTab({ group: g, active, here, branch, alone }: { group: TerminalGroup; active: boolean; here: boolean; branch: string | null; alone: boolean }) {
  const [renaming, setRenaming] = useState(false);
  const cwd = g.panes[0].cwd;
  const title = g.panes.find((p) => p.id === g.focused)?.title;
  const where = title ? `${cwd} · ${title}` : cwd;
  const calling = g.panes.some((p) => p.needsYou);
  const label = (g.name ? `${g.name} · ${where}` : where) + (calling ? " · needs you" : "");
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
      aria-label={branch ? `${label} · ${branch}` : label}
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
      <SquareTerminal className={cn("size-3.5 shrink-0", here ? "text-primary" : "text-subtle")} />
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
      {calling && <NeedsYouDot />}
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

/** Find (⌘F with focus in the terminal): the focused pane's text, its scrollback included. */
function TerminalFind() {
  const box = useFindBox("terminal");
  const [query, setQuery] = useState("");
  const [options, setOptions] = useState(NO_OPTIONS);
  const [at, setAt] = useState<{ index: number; total: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const search = (q: string, o: FindOptions, step: 0 | 1 | -1) => setError(findInTerminal(q, o, step, setAt));
  // Opened again: the last query's matches come back.
  useEffect(() => {
    if (box.asked && query) search(query, options, 0);
    // Only when Find asks.
  }, [box.asked]);
  // Another tab or pane: the box searches that one (the last one's marks go with it).
  const { groups, active } = useTerminals();
  const pane = groups.find((g) => g.id === active)?.focused;
  useEffect(() => {
    if (box.open && query) search(query, options, 0);
    else clearFind();
    // Only when the pane changes.
  }, [pane]);
  // The panel hidden: no marks left behind, nothing reporting to this box.
  useEffect(() => clearFind, []);
  if (!box.open) return null;
  return (
    <div className="absolute top-2 right-5 z-10">
      <FindBox
        query={query}
        onQuery={(q) => {
          setQuery(q);
          search(q, options, 0);
        }}
        options={options}
        onOptions={(o) => {
          setOptions(o);
          search(query, o, 0);
        }}
        at={at}
        error={error}
        onStep={(dir) => search(query, options, dir)}
        onClose={() => {
          box.close();
          endFind();
        }}
        focus={box.asked}
      />
    </div>
  );
}

/** Offers last run's terminals. Asked, not automatic: each one starts a shell. */
export function TerminalRestoreOffer() {
  const { restorable } = useTerminals();
  if (!restorable) return null;
  const cwds = restorable.groups.flatMap((g) => g.panes.map((p) => p.cwd));
  const folders = [...new Set(cwds.map((c) => folderName(c) || c))];
  return (
    <div className="pointer-events-auto fixed right-4 bottom-10 z-50 flex w-96 gap-3 rounded-md border border-border-strong bg-elevated p-3 shadow-lg shadow-black/50 animate-in fade-in-0 slide-in-from-bottom-2">
      <SquareTerminal className="mt-0.5 size-4 shrink-0 text-primary" />
      <div className="min-w-0 flex-1">
        <div className="text-[12px] font-medium">
          Restore {plural(cwds.length, "terminal")} from last session?
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

/** A split's structure without its sizes: what a tab re-lays out on (a split or close), not a drag. */
const shape = (l: Layout): string => (typeof l === "number" ? String(l) : `${l.dir}(${l.children.map(shape).join()})`);

type PaneInfo = TerminalGroup["panes"][number];

/**
 * A tab's panes as split; a dragged divider's sizes are kept in the tab's layout, by the split's
 * `path` from the top. `dim`: how far the panes other than the focused one fade.
 */
function LayoutView({ group, node, focused, dim, path = [] }: { group: TerminalGroup; node: Layout; focused: number; dim: number; path?: number[] }) {
  if (typeof node === "number") {
    // A pane in a split gets a header, as in cmux: what runs in each, and which one has the keys.
    const pane = path.length ? group.panes.find((p) => p.id === node) : undefined;
    return <PaneView id={node} dim={node === focused ? 0 : dim} header={pane && { pane, focused: node === focused }} />;
  }
  const id = (i: number) => `pane-${[...path, i].join("-")}`;
  const row = node.dir === "row";
  return (
    <ResizablePanelGroup
      orientation={row ? "horizontal" : "vertical"}
      defaultLayout={Object.fromEntries(node.sizes.map((size, i) => [id(i), size]))}
      onLayoutChanged={(layout, { isUserInteraction }) => isUserInteraction && resizeSplit(group.id, path, node.children.map((_, i) => layout[id(i)]))}
    >
      {node.children.map((c, i) => (
        <Fragment key={id(i)}>
          {/* The library focuses a divider as it's grabbed, from a few px either side of it too, where the
              pointer's release lands on the pane: the keys go back to the pane on any release. */}
          {/* Stronger than --border, which all but vanishes against a dark terminal. */}
          {i > 0 && <ResizableHandle className="bg-foreground/20" onFocus={() => window.addEventListener("pointerup", focusActive, { capture: true, once: true })} />}
          <ResizablePanel id={id(i)} minSize={row ? 160 : 80}>
            <LayoutView group={group} node={c} focused={focused} dim={dim} path={[...path, i]} />
          </ResizablePanel>
        </Fragment>
      ))}
    </ResizablePanelGroup>
  );
}

/** `dim`: how far it fades into the panel's background, while another pane of its tab has focus. */
function PaneView({ id, dim, header }: { id: number; dim: number; header?: { pane: PaneInfo; focused: boolean } }) {
  const ref = useRef<HTMLDivElement>(null);
  // Layout effect: the pane is in place before paint, so focusing it next frame works.
  useLayoutEffect(() => attachPane(id, ref.current!), [id]);
  // Read as the menu opens: the selection and the last command change under it.
  const [can, setCan] = useState(() => paneMenuState(id));
  return (
    <div className="flex h-full flex-col">
      {header && <PaneHeader pane={header.pane} focused={header.focused} />}
      <ContextMenu onOpenChange={(open) => open && setCan(paneMenuState(id))}>
        <ContextMenuTrigger asChild>
          {/* Inset from the edges like the code view's text; the scrollbar keeps the right edge, command marks the left. */}
          <div ref={ref} className="min-h-0 w-full flex-1 pt-2 pb-1 pl-3 transition-opacity duration-150" style={{ opacity: 1 - dim }} onContextMenu={(e) => paneTakesMouse(id) && e.preventDefault()} />
        </ContextMenuTrigger>
        <ContextMenuContent
          onCloseAutoFocus={(e) => {
            e.preventDefault();
            focusActive();
          }}
        >
          <ContextMenuItem disabled={!can.selection} onSelect={() => copyPaneSelection(id)}>
            <Copy /> Copy
          </ContextMenuItem>
          <ContextMenuItem disabled={!can.paste} onSelect={() => pasteIntoPane(id)}>
            <ClipboardPaste /> Paste
          </ContextMenuItem>
          <ContextMenuSeparator />
          {/* Shell integration marks where it starts and ends (Settings → Terminal). */}
          <ContextMenuItem disabled={!can.output} onSelect={() => copyLastOutput(id)}>
            <Copy /> Copy Last Command Output
          </ContextMenuItem>
          <ContextMenuItem disabled={!can.output} onSelect={() => selectLastOutput(id)}>
            <TextSelect /> Select Last Command Output
          </ContextMenuItem>
        </ContextMenuContent>
      </ContextMenu>
    </div>
  );
}

/** The split pane whose header shows its name field (⌘R, or a double-click on the header). */
const renamingPane = createStore<number | null>(null);

/** A split pane's title bar: the user's name for it, else the program's title, else its folder; and its own split and kill. */
function PaneHeader({ pane, focused }: { pane: PaneInfo; focused: boolean }) {
  const id = pane.id;
  const renaming = renamingPane.use() === id;
  const title = pane.name ?? (pane.title || folderName(paneDir(id) ?? pane.cwd));
  const action = (label: string, Icon: typeof X, run: () => void) => (
    <Tip label={label}>
      <button
        aria-label={label}
        tabIndex={-1}
        onClick={(e) => {
          e.stopPropagation();
          // A double-click's second click would split again (splitActive is async, the first not done yet).
          if (e.detail < 2) run();
        }}
        className="flex size-5 items-center justify-center rounded-sm text-subtle hover:bg-active hover:text-foreground"
      >
        <Icon className="size-3" />
      </button>
    </Tip>
  );
  return (
    <div
      // The keys stay with the terminal: a click here gives them to this pane.
      onMouseDown={(e) => !(e.target instanceof HTMLInputElement) && e.preventDefault()}
      onClick={() => focusTerminalPane(id)}
      // Not on its buttons: a double-click on Split is still a split, not a rename.
      onDoubleClick={(e) => !(e.target as Element).closest("button") && renamingPane.set(id)}
      className="group/pane relative flex h-6 shrink-0 cursor-default items-center gap-1.5 border-b border-border bg-panel pr-1 pl-2.5 text-[11.5px] select-none"
    >
      {focused && <span className="absolute inset-x-0 top-0 h-0.5 bg-primary" />}
      <SquareTerminal className={cn("size-3 shrink-0", focused ? "text-primary" : "text-subtle")} />
      {renaming ? (
        <NameInput
          initial={title}
          onDone={(name, refocus) => {
            renamingPane.set(null);
            // Left as it was, the title isn't a name the user gave.
            if (name !== null && name.trim() !== title) renamePane(id, name);
            if (refocus) focusTerminalPane(id);
          }}
        />
      ) : (
        <span className={cn("min-w-0 truncate", focused ? "text-foreground" : "text-muted-foreground")}>{title}</span>
      )}
      {pane.needsYou && <NeedsYouDot />}
      <div className={cn("ml-auto flex shrink-0 items-center", !focused && "opacity-0 group-hover/pane:opacity-100")}>
        {action("Split right", Columns2, () => void splitActive("row", id))}
        {action("Split down", Rows2, () => void splitActive("col", id))}
        {action("Kill pane", X, () => void killPane(id))}
      </div>
    </div>
  );
}
