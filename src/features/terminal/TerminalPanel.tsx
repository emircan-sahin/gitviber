import { open as pickFolder } from "@tauri-apps/plugin-dialog";
import { ChevronDown, Columns2, FolderGit2, FolderOpen, History, Maximize2, Minimize2, Plus, Rows2, Trash2, ZoomIn, ZoomOut } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Tip } from "@/components/ui/tooltip";
import { api, type Worktree } from "@/lib/api";
import { COMMANDS, commandIn, runCommand, useCommands, useShortcut } from "@/lib/commands/keybindings";
import { stepTerminalFont, useSettings } from "@/lib/settings";
import { focusMovedTab, focusTab, tabMove } from "@/lib/ui/useListNav";
import { focusedPanel, focusPanel } from "@/lib/ui/panels";
import {
  activateGroup,
  clearFocused,
  closeFocused,
  closeGroup,
  focusToward,
  moveGroup,
  openTerminal,
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
import { folderName, parentFolder } from "@/lib/path";
import { worktreeHue } from "@/lib/git/worktrees";
import { useWorktreeColors } from "@/lib/git/worktreeColors";
import { ProjectTile } from "@/features/projects/ProjectList";
import { showConversations } from "@/features/palette/CommandPalette";
import { GroupTab } from "./GroupTab";
import { LayoutView, renamingPane, shape } from "./PaneLayout";
import { TerminalFind } from "./TerminalFind";

/** What the workspace needs even while the panel is hidden: the panel shortcuts, and following the worktree that's open. */
export function useTerminalSetup(root: string) {
  useEffect(() => showWorktree(root), [root]);
  // Another worktree's workspace starts with its code in view.
  useEffect(() => unmaximize, [root]);

  // Maximize and zoom from the palette too, where they open the panel first.
  useCommands({
    "terminal.toggle": () => toggle(root),
    "terminal.new": () => openTerminal(root),
    "terminal.resumeConversation": () => showConversations(root),
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
  const colors = useWorktreeColors();
  // This repo's worktrees take their name's color; a tab elsewhere only a color picked for its folder.
  const hueOf = (g: TerminalGroup) => {
    const cwd = g.panes[0].cwd;
    return worktreeHue(worktrees.find((x) => x.path === cwd) ?? { path: cwd, main: true }, colors);
  };
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
            <GroupTab key={g.id} group={g} active={g.id === active} here={g.panes[0].cwd === root} branch={branchOf(g)} hue={hueOf(g)} alone={groups.length === 1} />
          ))}
        </div>
        <div className="flex shrink-0 items-center gap-0.5 px-1.5">
          <Tip label={`New terminal in ${folderName(root)}`} shortcut={useShortcut("terminal.new")}>
            <Button variant="ghost" size="icon-sm" onClick={() => openTerminal(root)}>
              <Plus />
            </Button>
          </Tip>
          <DropdownMenu>
            <Tip label="New terminal in another folder, or resume a conversation">
              <DropdownMenuTrigger asChild>
                <Button variant="ghost" size="icon-sm" className="w-5">
                  <ChevronDown className="size-3" />
                </Button>
              </DropdownMenuTrigger>
            </Tip>
            <DropdownMenuContent align="end" className="w-72">
              <DropdownMenuItem onSelect={() => showConversations(root)}>
                <History /> Resume a Conversation…
              </DropdownMenuItem>
              <DropdownMenuSeparator />
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
