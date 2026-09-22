import { arrayMove } from "@dnd-kit/sortable";
import { ask } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  ArrowDownToLine,
  ArrowUpFromLine,
  Check,
  ChevronDown,
  ChevronsUpDown,
  CloudOff,
  GitMerge,
  GitPullRequestArrow,
  Loader2,
  PanelLeft,
  PanelLeftDashed,
  PanelRight,
  PanelRightDashed,
  Plus,
  RefreshCw,
  Settings2,
  SquareTerminal,
  UploadCloud,
  X,
} from "lucide-react";
import { useEffect, useState } from "react";
import { Wordmark } from "@/components/Logo";
import { SortableList, useSortableItem } from "@/components/Sortable";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Tip } from "@/components/ui/tooltip";
import { api, errorMessage, type Worktree } from "@/lib/api";
import { useShortcut } from "@/lib/keybindings";
import { openTerminal, togglePanel, useTerminals } from "@/lib/terminals";
import { toast } from "@/lib/toast";
import type { RepoData } from "@/lib/useRepo";
import { cn } from "@/lib/utils";
import { folderName } from "@/lib/worktrees";
import { BranchPicker } from "./BranchPicker";
import { openSettings } from "./SettingsDialog";
import { WorktreePicker } from "./WorktreePicker";

interface Props {
  repo: RepoData;
  root: string;
  /** The main worktree: the project this window belongs to, even inside a linked worktree. */
  main: string;
  recent: string[];
  onOpenRepo: (path?: string) => void;
  onForgetRepo: (path: string) => void;
  onReorderRepos: (list: string[]) => void;
}

interface LayoutProps {
  leftOpen: boolean;
  rightOpen: boolean;
  onToggleLeft: () => void;
  onToggleRight: () => void;
}

export function changeTotals(repo: RepoData) {
  // Nested repos (agent worktrees) have no diff here, so they aren't changes to review.
  const files = repo.status ? [...repo.status.staged, ...repo.status.unstaged.filter((f) => !f.nested), ...repo.status.conflicted] : [];
  return {
    files: files.length,
    add: files.reduce((n, f) => n + (f.additions ?? 0), 0),
    del: files.reduce((n, f) => n + (f.deletions ?? 0), 0),
  };
}

/** titlebar.rs emits "fullscreen" as each full-screen transition starts. */
function useFullscreen() {
  const [fullscreen, setFullscreen] = useState(false);
  useEffect(() => {
    // getCurrentWindow() throws outside Tauri (the browser-only dev fixture).
    let win;
    try {
      win = getCurrentWindow();
    } catch {
      return;
    }
    // A reload while in full screen gets no event, so ask once.
    win.isFullscreen().then(setFullscreen, () => {});
    const unlisten = win.listen<boolean>("fullscreen", (e) => setFullscreen(e.payload));
    return () => void unlisten.then((f) => f());
  }, []);
  return fullscreen;
}

/**
 * The window's title bar: project / branch breadcrumb on the left (after the traffic
 * lights), sync actions and settings on the right. Empty space drags the window.
 * Its 40px height matches the native title bar (a compact toolbar, see titlebar.rs), whose
 * traffic lights end at 66pt; the 86px left inset clears them. Full screen moves them out
 * of the window, so the inset goes too. The traffic lights ignore page zoom, so both are
 * divided by --ui-scale to stay in points (the height only grows: at 150% a 40pt bar can't
 * fit its buttons).
 */
export function TopBar({ repo, root, main, recent, onOpenRepo, onForgetRepo, onReorderRepos, leftOpen, rightOpen, onToggleLeft, onToggleRight }: Props & LayoutProps) {
  const { status, branches, worktrees } = repo;
  const [busy, setBusy] = useState<string | null>(null);
  const terminalOpen = useTerminals().open;
  const fullscreen = useFullscreen();

  // Operations that can stop on conflicts resolve to true; that's a state to handle, not an error.
  const run = async (label: string, fn: () => Promise<void | boolean>, done?: string) => {
    setBusy(label);
    try {
      const stopped = await fn();
      if (stopped) toast("info", `${label} stopped on conflicts`, "Resolve them in Changes, then continue.");
      else if (done) toast("success", done);
    } catch (e) {
      toast("error", `${label} failed`, errorMessage(e));
    } finally {
      setBusy(null);
      await repo.refresh();
    }
  };

  const branchName = status?.branch ?? (status?.head ? `detached @ ${status.head}` : "…");

  // git refuses to check out a branch another worktree has; going to that worktree is the way.
  const openWorktree = async (path: string) => {
    // Ask git now: the list loaded with the branches may predate an agent removing its folder.
    const fresh = await api.worktrees().catch(() => worktrees);
    if (fresh.find((w) => w.path === path)?.prunable) {
      toast("error", "That worktree's folder is gone", `${path} no longer exists but still holds the branch. git worktree prune releases it.`);
    } else onOpenRepo(path);
  };

  // A terminal on a branch runs where it's checked out; one that isn't gets its own worktree
  // rather than a checkout here, which would pull the files out from under this window.
  const branchTerminal = async (name: string, worktree: string | null) => {
    if (name === status?.branch) return openTerminal(root);
    if (worktree) return openTerminal(worktree);
    const where = `${folderName(main)}.worktrees/${name.replaceAll("/", "-")}`;
    const ok = await ask(`${name} isn't checked out anywhere. Create a worktree for it at ${where}, next to this project, and open a terminal there?`, {
      title: "Open terminal on branch",
      okLabel: "Create worktree",
    });
    if (ok) await run("Create worktree", async () => openTerminal(await api.addWorktree(name)), `${name} checked out in ${where}`);
  };

  // A fresh count decides force: git refuses a dirty or locked worktree otherwise, and the
  // warning must say what gets lost. If counting fails, git's own refusal is the fallback.
  const removeWorktree = async (w: Worktree) => {
    const name = folderName(w.path);
    const changed = w.prunable ? 0 : await api.worktreeChanges(w.path).catch(() => 0);
    const branch = w.branch ? ` The branch ${w.branch} stays.` : "";
    const lost = changed ? ` Its ${changed} uncommitted ${changed === 1 ? "change" : "changes"} will be lost.` : "";
    const lock = w.locked ? " It's locked; this overrides the lock." : "";
    const ok = await ask(
      w.prunable ? `${name}'s folder is already gone. Remove it from the worktree list?${branch}` : `Delete worktree ${name} and its folder?${lost}${lock}${branch}`,
      { title: "Remove worktree", kind: "warning", okLabel: w.prunable ? "Remove" : "Delete worktree" },
    );
    if (ok) await run("Remove worktree", () => api.removeWorktree(w.path, changed > 0 || w.locked), `Worktree ${name} removed`);
  };

  return (
    <header
      data-tauri-drag-region
      className={`flex h-[max(40px,calc(40px/var(--ui-scale,1)))] shrink-0 items-center gap-1 border-b border-border bg-sidebar pr-2 ${fullscreen ? "pl-2" : "pl-[calc(86px/var(--ui-scale,1))]"}`}
    >
      <Wordmark />
      <div className="mx-2 h-4 w-px bg-border-strong" />
      <ProjectSwitcher repo={repo} root={root} main={main} recent={recent} onOpenRepo={onOpenRepo} onForgetRepo={onForgetRepo} onReorderRepos={onReorderRepos} />
      <span className="text-[13px] text-border-strong select-none">/</span>
      <BranchPicker
        label={branchName}
        branches={branches}
        current={status?.branch ?? null}
        onOpenWorktree={openWorktree}
        onSwitch={(name) => run("Switch branch", () => api.switchBranch(name, false), `Switched to ${name}`)}
        onCreate={(name) => run("Create branch", () => api.switchBranch(name, true), `Switched to new branch ${name}`)}
        onMerge={(name) => run("Merge", () => api.merge(name), `Merged ${name}`)}
        onRebase={(name) => run("Rebase", () => api.rebase(name), `Rebased onto ${name}`)}
        onTerminal={branchTerminal}
      />
      <WorktreePicker worktrees={worktrees} onOpen={onOpenRepo} onTerminal={openTerminal} onRemove={removeWorktree} />
      {status && !status.upstream && status.branch && (
        <span className="flex shrink-0 items-center gap-1 rounded-sm px-1.5 py-0.5 text-[11px] text-subtle select-none">
          <CloudOff className="size-3" /> Not published
        </span>
      )}

      {/* Filler: grabbing the bar anywhere empty moves the window. */}
      <div data-tauri-drag-region className="min-w-4 flex-1 self-stretch" />

      {busy && (
        <span className="mr-1 flex shrink-0 items-center gap-1.5 text-[11.5px] text-muted-foreground select-none">
          <Loader2 className="size-3.5 animate-spin" /> {busy}…
        </span>
      )}
      <Tip label="Fetch">
        <Button variant="ghost" size="icon" disabled={!!busy} onClick={() => run("Fetch", api.fetch)}>
          <RefreshCw />
        </Button>
      </Tip>
      <div className="flex">
        <Tip label="Pull (fast-forward only)">
          <Button variant="secondary" className="rounded-r-none" disabled={!!busy || !status?.upstream} onClick={() => run("Pull", () => api.pull("ff"), "Pulled")}>
            <ArrowDownToLine /> Pull
            {!!status?.behind && <span className="font-mono text-[10.5px] text-primary">{status.behind}</span>}
          </Button>
        </Tip>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="secondary" className="w-5 rounded-l-none border-l-0 px-0" disabled={!!busy || !status?.upstream}>
              <ChevronDown className="size-3" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-56">
            <DropdownMenuLabel>When branches have diverged</DropdownMenuLabel>
            <DropdownMenuItem onSelect={() => run("Pull", () => api.pull("merge"), "Pulled (merge)")}>
              <GitMerge /> Pull with merge
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={() => run("Pull", () => api.pull("rebase"), "Pulled (rebase)")}>
              <GitPullRequestArrow /> Pull with rebase
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      {status?.upstream ? (
        <Tip label={`Push to ${status.upstream}`}>
          <Button variant={status.ahead ? "default" : "secondary"} disabled={!!busy} onClick={() => run("Push", api.push, "Pushed")}>
            <ArrowUpFromLine /> Push
            {!!status.ahead && <span className="font-mono text-[10.5px]">{status.ahead}</span>}
          </Button>
        </Tip>
      ) : (
        <Tip label="Push this branch to origin and track it">
          <Button disabled={!!busy || !status?.branch} onClick={() => run("Publish", api.push, "Branch published")}>
            <UploadCloud /> Publish
          </Button>
        </Tip>
      )}
      <div className="mx-1 h-4 w-px bg-border-strong" />
      <Tip label={terminalOpen ? "Hide terminal" : "Show terminal"} shortcut="⌃`">
        <Button variant="ghost" size="icon" onClick={() => togglePanel(root)} className={cn(terminalOpen && "text-foreground")}>
          <SquareTerminal />
        </Button>
      </Tip>
      <Tip label={leftOpen ? "Hide git panel" : "Show git panel"} shortcut={useShortcut("view.toggleGitPanel")}>
        <Button variant="ghost" size="icon" onClick={onToggleLeft} className={cn(leftOpen && "text-foreground")}>
          {leftOpen ? <PanelLeft /> : <PanelLeftDashed />}
        </Button>
      </Tip>
      <Tip label={rightOpen ? "Hide explorer" : "Show explorer"} shortcut={useShortcut("view.toggleExplorer")}>
        <Button variant="ghost" size="icon" onClick={onToggleRight} className={cn(rightOpen && "text-foreground")}>
          {rightOpen ? <PanelRight /> : <PanelRightDashed />}
        </Button>
      </Tip>
      <SettingsButton />
    </header>
  );
}

function ProjectSwitcher({ repo, main, recent, onOpenRepo, onForgetRepo, onReorderRepos }: Props) {
  const [open, setOpen] = useState(false);
  const totals = changeTotals(repo);
  const name = main.split("/").pop() ?? main;
  const openKey = useShortcut("file.openRepo");
  const pick = (p?: string) => {
    setOpen(false);
    onOpenRepo(p);
  };
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button className="flex h-7 max-w-64 min-w-0 items-center gap-2 rounded-md px-2 hover:bg-hover data-[state=open]:bg-active">
          <ProjectTile name={name} />
          <span className="truncate text-[12.5px] font-semibold">{name}</span>
          {totals.files > 0 && (
            <span className="shrink-0 font-mono text-[10.5px]">
              <span className="text-added">+{totals.add}</span> <span className="text-removed">-{totals.del}</span>
            </span>
          )}
          <ChevronsUpDown className="size-3 shrink-0 text-subtle" />
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" className="flex w-80 flex-col overflow-hidden">
        <div className="px-3 pt-2.5 pb-1 text-[10.5px] font-semibold tracking-[0.08em] text-subtle uppercase">Projects</div>
        <div className="max-h-[360px] min-h-0 overflow-x-hidden overflow-y-auto p-1">
          <SortableList ids={recent} axis="y" onMove={(from, to) => onReorderRepos(arrayMove(recent, from, to))}>
            {recent.map((p) => (
              <ProjectRow key={p} path={p} current={p === main} onOpen={pick} onForget={onForgetRepo} />
            ))}
          </SortableList>
        </div>
        <div className="border-t border-border p-1">
          <button onClick={() => pick()} className="flex h-7 w-full items-center gap-2 rounded-sm px-2 text-[12px] hover:bg-hover">
            <Plus className="size-3.5 text-muted-foreground" /> Open repository…
            <span className="ml-auto font-mono text-[11px] text-subtle">{openKey}</span>
          </button>
        </div>
      </PopoverContent>
    </Popover>
  );
}

function ProjectRow({ path, current, onOpen, onForget }: { path: string; current: boolean; onOpen: (p: string) => void; onForget: (p: string) => void }) {
  const { props, dragging, guard } = useSortableItem(path);
  const name = path.split("/").pop() ?? path;
  return (
    <div
      {...props}
      role="button"
      onClick={guard(() => !current && onOpen(path))}
      className={cn(
        "group flex h-9 items-center gap-2.5 rounded-sm px-2 select-none",
        current ? "cursor-default bg-active" : "cursor-pointer hover:bg-hover",
        dragging && "cursor-grabbing bg-elevated shadow-lg ring-1 shadow-black/40 ring-border-strong",
      )}
    >
      <ProjectTile name={name} large />
      <div className="min-w-0 flex-1">
        <div className="truncate text-[12.5px] font-medium">{name}</div>
        <div className="truncate text-[10.5px] text-subtle">{path}</div>
      </div>
      {current ? (
        <Check className="size-3.5 shrink-0 text-primary" />
      ) : (
        !dragging && (
          <Tip label="Remove from list">
            <button
              // Pressing the button must not start a drag.
              onPointerDown={(e) => e.stopPropagation()}
              onClick={(e) => {
                e.stopPropagation();
                onForget(path);
              }}
              className="hidden size-5 shrink-0 items-center justify-center rounded-sm text-subtle group-hover:flex hover:bg-active hover:text-foreground"
            >
              <X className="size-3" />
            </button>
          </Tip>
        )
      )}
    </div>
  );
}

/** Square letter tile with a stable hue per project, so repos are recognizable at a glance. */
function ProjectTile({ name, large }: { name: string; large?: boolean }) {
  let h = 0;
  for (const c of name) h = (h * 31 + c.charCodeAt(0)) % 360;
  return (
    <span
      className={cn(
        "flex shrink-0 items-center justify-center rounded-[4px] font-bold text-black/80 uppercase",
        large ? "size-6 text-[11px]" : "size-4 text-[9.5px]",
      )}
      style={{ background: `hsl(${h} 55% 62%)` }}
    >
      {name[0]}
    </span>
  );
}

function SettingsButton() {
  return (
    <Tip label="Settings" shortcut={useShortcut("workbench.openSettings")}>
      <Button variant="ghost" size="icon" onClick={() => openSettings()}>
        <Settings2 />
      </Button>
    </Tip>
  );
}
