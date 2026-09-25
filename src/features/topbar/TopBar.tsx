import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  ArrowDownToLine,
  ArrowUpFromLine,
  ChevronDown,
  CloudOff,
  GitMerge,
  GitPullRequestArrow,
  Loader2,
  PanelLeft,
  PanelLeftDashed,
  PanelRight,
  PanelRightDashed,
  RefreshCw,
  Settings2,
  SquareTerminal,
  X,
} from "lucide-react";
import { useEffect, useState } from "react";
import { Wordmark } from "@/components/Logo";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Tip } from "@/components/ui/tooltip";
import { api, cancelNetwork } from "@/lib/api";
import { IS_MAC } from "@/lib/platform";
import { useCommands, useShortcut } from "@/lib/commands/keybindings";
import { openTerminal, togglePanel, useTerminals } from "@/lib/terminal/terminals";
import { useNetActivity } from "@/lib/repo/netActivity";
import { forgetRemoteTags } from "@/lib/repo/remoteTags";
import { cn } from "@/lib/utils";
import { type BranchDialog, BranchDialogs } from "@/features/branches/BranchDialogs";
import { BranchPicker } from "@/features/branches/BranchPicker";
import { PushMenu } from "./PushMenu";
import { ProjectSwitcher, type ProjectSwitcherProps } from "./ProjectSwitcher";
import { PublishButton } from "./PublishButton";
import { UndoControls } from "./UndoControls";
import { useRepoActions } from "./useRepoActions";
import { openSettings } from "@/features/settings/SettingsDialog";
import { openWorktreeDialog, WorktreeDialogs } from "@/features/worktrees/WorktreeDialogs";
import { WorktreePicker } from "@/features/worktrees/WorktreePicker";

interface Props extends ProjectSwitcherProps {
  root: string;
}

interface LayoutProps {
  leftOpen: boolean;
  rightOpen: boolean;
  onToggleLeft: () => void;
  onToggleRight: () => void;
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
 * of the window, so the inset goes too, as it does off macOS. The traffic lights ignore page zoom, so both are
 * divided by --ui-scale to stay in points (the height only grows: at 150% a 40pt bar can't
 * fit its buttons).
 */
export function TopBar({ repo, root, main, recent, onOpenRepo, onForgetRepo, onReorderRepos, onLocateRepo, leftOpen, rightOpen, onToggleLeft, onToggleRight }: Props & LayoutProps) {
  const { status, branches, worktrees } = repo;
  const [branchDialog, setBranchDialog] = useState<BranchDialog | null>(null);
  const net = useNetActivity();
  const terminalOpen = useTerminals().open;
  const fullscreen = useFullscreen();

  const { busy, run, runNet, pull, sync, branchTerminal, deleteBranch, cleanUp, publish, publishTo, merge, push, pushAhead, switching, switchRemote, removeWorktree, unlockWorktree } = useRepoActions(repo, root, main);

  const activity = busy ?? net?.label;
  const progress = net?.progress ? `${net.progress.phase}${net.progress.percent !== null ? ` ${net.progress.percent}%` : ""}` : "";
  const branchName = status?.branch ?? (status?.head ? `detached @ ${status.head}` : "…");

  useCommands({
    "git.fetch": busy ? undefined : () => runNet("Fetch", api.fetch),
    "git.pull": busy || !status?.upstream ? undefined : () => pull("ff"),
    // With no upstream yet, pushing is publishing, where Publish would without asking.
    "git.push": busy ? undefined : status?.upstream ? () => push() : publishTo ? () => publish(publishTo) : undefined,
    "git.sync": busy || !status?.upstream ? undefined : () => sync(),
    "git.newBranch": () => setBranchDialog({ kind: "new", base: status?.branch ? `refs/heads/${status.branch}` : "HEAD" }),
    "git.newWorktree": () => openWorktreeDialog({ kind: "new" }),
  });

  return (
    <header
      data-tauri-drag-region
      className={`flex h-[max(40px,calc(40px/var(--ui-scale,1)))] shrink-0 items-center gap-1 border-b border-border bg-sidebar pr-2 ${fullscreen || !IS_MAC ? "pl-2" : "pl-[calc(86px/var(--ui-scale,1))]"}`}
    >
      <Wordmark />
      <div className="mx-2 h-4 w-px bg-border-strong" />
      <ProjectSwitcher repo={repo} main={main} recent={recent} onOpenRepo={onOpenRepo} onForgetRepo={onForgetRepo} onReorderRepos={onReorderRepos} onLocateRepo={onLocateRepo} />
      <span className="text-[13px] text-border-strong select-none">/</span>
      <BranchPicker
        label={branchName}
        branches={branches}
        current={status?.branch ?? null}
        onSwitch={(name) => run("Switch branch", switching(name, () => api.switchBranch(name, false)), `Switched to ${name}`)}
        onSwitchRemote={switchRemote}
        onCreate={(name) => run("Create branch", () => api.switchBranch(name, true), `Switched to new branch ${name}`)}
        onMerge={merge}
        onRebase={(name) => run("Rebase", () => api.rebase(name), `Rebased onto ${name}`)}
        onTerminal={branchTerminal}
        onDelete={deleteBranch}
        onCleanUp={cleanUp}
        onRename={(branch) => setBranchDialog({ kind: "rename", branch })}
        onNewBranch={(base) => setBranchDialog({ kind: "new", base })}
        onSetUpstream={(branch) => setBranchDialog({ kind: "upstream", branch })}
        onUnsetUpstream={(b) => run("Unset upstream", () => api.setUpstream(b.name, null), `${b.name} no longer tracks ${b.upstream}`)}
      />
      {branchDialog && <BranchDialogs dialog={branchDialog} branches={branches} onClose={() => setBranchDialog(null)} run={run} runNet={runNet} />}
      <WorktreePicker
        worktrees={worktrees}
        branches={branches}
        onOpen={onOpenRepo}
        onTerminal={openTerminal}
        onMerge={merge}
        onRemove={removeWorktree}
        onRename={(worktree) => openWorktreeDialog({ kind: "rename", worktree })}
        onLock={(worktree) => openWorktreeDialog({ kind: "lock", worktree })}
        onUnlock={unlockWorktree}
        onNew={() => openWorktreeDialog({ kind: "new" })}
      />
      <WorktreeDialogs branches={branches} main={main} run={run} runNet={runNet} onOpen={onOpenRepo} />
      {status && !status.upstream && status.branch && (
        <span className="flex shrink-0 items-center gap-1 rounded-sm px-1.5 py-0.5 text-[11px] text-subtle select-none">
          <CloudOff className="size-3" /> Not published
        </span>
      )}

      {/* Filler: grabbing the bar anywhere empty moves the window. A running command shows at
          its right end, so starting or ending one never shifts the buttons around it (a click
          meant for × once landed on the undo history), and its text truncates, not the bar.
          History's tag pushes show here too: `net` is whichever network command runs. */}
      <div data-tauri-drag-region className="flex min-w-4 flex-1 items-center justify-end self-stretch overflow-hidden">
        {activity && (
          <span className="mr-1.5 flex min-w-0 items-center gap-1.5 text-[11.5px] text-muted-foreground select-none">
            <Loader2 className="size-3.5 shrink-0 animate-spin" />
            <span className="min-w-0 truncate tabular-nums" title={`${activity}… ${progress}`}>
              {activity}… <span className="text-subtle">{progress}</span>
            </span>
            {net && (
              <Tip label={net.progress?.cancellable === false ? "Too late to cancel: git is updating your files" : `Cancel ${net.label.toLowerCase()}`}>
                <span className="shrink-0">
                  <Button variant="ghost" size="icon-sm" aria-label="Cancel" disabled={net.progress?.cancellable === false} onClick={() => void cancelNetwork(net.op)}>
                    <X />
                  </Button>
                </span>
              </Tip>
            )}
          </span>
        )}
      </div>

      <UndoControls repo={repo} disabled={!!busy} />
      <div className="mx-1 h-4 w-px bg-border-strong" />
      <Tip label="Fetch">
        <Button variant="ghost" size="icon" disabled={!!busy} onClick={() => runNet("Fetch", api.fetch)}>
          <RefreshCw />
        </Button>
      </Tip>
      <div className="flex">
        <Tip label="Pull (fast-forward only)">
          <Button variant="secondary" className="rounded-r-none" disabled={!!busy || !status?.upstream} onClick={() => pull("ff")}>
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
            <DropdownMenuItem onSelect={() => pull("merge")}>
              <GitMerge /> Pull with merge
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={() => pull("rebase")}>
              <GitPullRequestArrow /> Pull with rebase
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      {status?.upstream ? (
        // Where the push lands, which a fork can set apart from where it pulls (upstream/dev
        // pulled, origin/dev pushed). Not there yet: the push creates it.
        <div className="flex">
          <Tip label={status.push?.branch ? `Push to ${status.push.branch}` : `Push to ${status.push?.remote ?? "the remote"} (creates the branch there)`}>
            <Button variant={pushAhead !== 0 ? "default" : "secondary"} className="rounded-r-none" disabled={!!busy} onClick={() => push()}>
              <ArrowUpFromLine /> Push
              {!!pushAhead && <span className="font-mono text-[10.5px]">{pushAhead}</span>}
            </Button>
          </Tip>
          <PushMenu
            primary={pushAhead !== 0}
            disabled={!!busy}
            onPush={push}
            onPushTags={(names, remote) =>
              runNet("Push tags", (op) => api.pushTags(names, op).then(forgetRemoteTags), `Pushed ${names.length === 1 ? names[0] : `${names.length} tags`} to ${remote}`)
            }
          />
        </div>
      ) : (
        <PublishButton
          remotes={status?.remotes ?? []}
          preferred={status?.publish ?? null}
          // Before the first commit there's nothing to push.
          disabled={!!busy || !status?.branch || !status.head}
          onPublish={publish}
        />
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

function SettingsButton() {
  return (
    <Tip label="Settings" shortcut={useShortcut("workbench.openSettings")}>
      <Button variant="ghost" size="icon" onClick={() => openSettings()}>
        <Settings2 />
      </Button>
    </Tip>
  );
}
