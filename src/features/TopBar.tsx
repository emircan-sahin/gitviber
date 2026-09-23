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
  Redo2,
  RefreshCw,
  Settings2,
  SquareTerminal,
  TriangleAlert,
  Undo2,
  UploadCloud,
} from "lucide-react";
import { useEffect, useState } from "react";
import { Wordmark } from "@/components/Logo";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Tip, Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { api, errorMessage, type Branch, type JournalEntry, type Worktree } from "@/lib/api";
import { useCommands, useShortcut } from "@/lib/keybindings";
import { openTerminal, togglePanel, useTerminals } from "@/lib/terminals";
import { toast } from "@/lib/toast";
import { tracked, travel, undoAction } from "@/lib/undo";
import type { RepoData } from "@/lib/useRepo";
import { cn, relativeTime } from "@/lib/utils";
import { folderName } from "@/lib/worktrees";
import { BranchPicker } from "./BranchPicker";
import { ProjectList, ProjectTile } from "./ProjectList";
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
  onLocateRepo: (path: string) => void;
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
export function TopBar({ repo, root, main, recent, onOpenRepo, onForgetRepo, onReorderRepos, onLocateRepo, leftOpen, rightOpen, onToggleLeft, onToggleRight }: Props & LayoutProps) {
  const { status, branches, worktrees } = repo;
  const [busy, setBusy] = useState<string | null>(null);
  const terminalOpen = useTerminals().open;
  const fullscreen = useFullscreen();

  // Operations that can stop on conflicts resolve to true; that's a state to handle, not an error.
  const run = async (label: string, fn: () => Promise<void | boolean>, done?: string) => {
    setBusy(label);
    try {
      const [stopped, entry] = await tracked(fn);
      if (stopped) toast("info", `${label} stopped on conflicts`, "Resolve them in Changes, then continue.");
      else if (done) toast("success", done, undefined, undoAction(entry, repo.refresh));
    } catch (e) {
      toast("error", `${label} failed`, errorMessage(e));
    } finally {
      setBusy(null);
      await repo.refresh();
    }
  };

  const branchName = status?.branch ?? (status?.head ? `detached @ ${status.head}` : "…");

  // A terminal on another branch gets its own worktree rather than a checkout here, which
  // would pull the files out from under this window.
  const branchTerminal = async (name: string) => {
    if (name === status?.branch) return openTerminal(root);
    const where = `${folderName(main)}.worktrees/${name.replaceAll("/", "-")}`;
    const ok = await ask(`${name} isn't checked out anywhere. Create a worktree for it at ${where}, next to this project, and open a terminal there?`, {
      title: "Open terminal on branch",
      okLabel: "Create worktree",
    });
    if (ok) await run("Create worktree", async () => openTerminal(await api.addWorktree(name)), `${name} checked out in ${where}`);
  };

  // Merged is deleted outright: nothing is lost. Anything else needs a yes, then -D.
  // A remote branch always asks: the push takes it away for everyone.
  const deleteBranch = async (b: Branch) => {
    if (b.remote) {
      const [remote, ...rest] = b.name.split("/");
      const ok = await ask(`Delete ${rest.join("/")} from ${remote}? It goes for everyone who uses ${remote}; local branches stay.`, {
        title: "Delete remote branch",
        kind: "warning",
        okLabel: "Delete",
      });
      if (ok) await run("Delete remote branch", () => api.deleteRemoteBranch(b.name), `Deleted ${b.name}`);
      return;
    }
    const here = status?.branch ?? "HEAD";
    if (!b.merged) {
      const ok = await ask(`${b.name} isn't known to be merged into ${here}. Deleting it loses any commits that exist only on it.`, {
        title: "Delete branch",
        kind: "warning",
        okLabel: "Delete",
      });
      if (!ok) return;
    }
    await run("Delete branch", () => api.deleteBranches([b.name], !b.merged), `Deleted ${b.name}`);
  };

  const cleanUp = async (names: string[]) => {
    const shown = names.slice(0, 12).join("\n") + (names.length > 12 ? `\n…and ${names.length - 12} more` : "");
    const ok = await ask(`Delete ${names.length} branches already merged into ${status?.branch ?? "HEAD"}?\n\n${shown}`, {
      title: "Clean up merged branches",
      okLabel: "Delete",
    });
    if (ok) await run("Clean up", () => api.deleteBranches(names, false), `Deleted ${names.length} merged branches`);
  };

  const merge = (name: string) => run("Merge", () => api.merge(name), `Merged ${name}`);
  // Rejected as non-fast-forward: the remote has commits this branch dropped, usually its own
  // old ones after a rebase or amend. Replacing them is a force push, so it asks first.
  // "fetch first" (commits not fetched yet) isn't offered: those want a pull.
  const push = () =>
    run(
      "Push",
      async () => {
        try {
          await api.push();
        } catch (e) {
          if (!errorMessage(e).includes("non-fast-forward")) throw e;
          const ok = await ask(
            "The remote branch has commits yours no longer has, as after a rebase or an amend. Replace them with yours?\n\nThis force-pushes (with lease): it is refused if someone pushed there since your last fetch. Anyone who pulled the old commits will have to reconcile.",
            { title: "Force push", kind: "warning", okLabel: "Force push" },
          );
          if (!ok) throw e;
          await api.push(true);
        }
      },
      "Pushed",
    );
  // Unknown until the push target has the branch; then a push is due.
  const pushAhead = status?.push ? (status.push.branch ? status.push.ahead : null) : (status?.ahead ?? 0);

  // upstream/dev → dev. A local dev that tracks something else (origin/dev, say) is a
  // different line of work; say so rather than switch to it silently.
  const switchRemote = async (b: Branch) => {
    const name = b.name.slice(b.name.indexOf("/") + 1);
    const local = branches.find((x) => !x.remote && x.name === name);
    if (local?.current) return;
    if (local && local.upstream !== b.name) {
      const tracks = local.upstream ? `tracks ${local.upstream}` : "tracks nothing";
      const ok = await ask(`A local ${name} already exists and ${tracks}, not ${b.name}. Switch to it as it is?`, { title: "Switch branch", okLabel: "Switch" });
      if (!ok) return;
    }
    await run("Switch branch", () => api.switchTracking(b.name), `Switched to ${name}`);
  };

  // A fresh count decides force: git refuses a dirty or locked worktree otherwise, and the
  // warning must say what gets lost. If counting fails, git's own refusal is the fallback.
  const removeWorktree = async (w: Worktree) => {
    const name = folderName(w.path);
    const changed = w.prunable ? 0 : await api.worktreeState(w.path).then((s) => s.uncommitted, () => 0);
    const branch = w.branch ? ` The branch ${w.branch} stays.` : "";
    const lost = changed ? ` Its ${changed} uncommitted ${changed === 1 ? "change" : "changes"} will be lost.` : "";
    const lock = w.inUse
      ? ` Something is working in it right now (${w.lockReason ?? "it holds the lock"}); deleting pulls the folder out from under it.`
      : w.locked
        ? " It's locked; this overrides the lock."
        : "";
    const ok = await ask(
      w.prunable ? `${name}'s folder is already gone. Remove it from the worktree list?${branch}` : `Delete worktree ${name} and its folder?${lost}${lock}${branch}`,
      { title: "Remove worktree", kind: "warning", okLabel: w.prunable ? "Remove" : "Delete worktree" },
    );
    if (ok) await run("Remove worktree", () => api.removeWorktree(w.path, changed > 0 || w.locked), `Worktree ${name} removed`);
  };

  useCommands({
    "git.fetch": busy ? undefined : () => run("Fetch", api.fetch),
    "git.pull": busy || !status?.upstream ? undefined : () => run("Pull", () => api.pull("ff"), "Pulled"),
    "git.push": busy || !status?.upstream ? undefined : push,
  });

  return (
    <header
      data-tauri-drag-region
      className={`flex h-[max(40px,calc(40px/var(--ui-scale,1)))] shrink-0 items-center gap-1 border-b border-border bg-sidebar pr-2 ${fullscreen ? "pl-2" : "pl-[calc(86px/var(--ui-scale,1))]"}`}
    >
      <Wordmark />
      <div className="mx-2 h-4 w-px bg-border-strong" />
      <ProjectSwitcher repo={repo} root={root} main={main} recent={recent} onOpenRepo={onOpenRepo} onForgetRepo={onForgetRepo} onReorderRepos={onReorderRepos} onLocateRepo={onLocateRepo} />
      <span className="text-[13px] text-border-strong select-none">/</span>
      <BranchPicker
        label={branchName}
        branches={branches}
        current={status?.branch ?? null}
        onSwitch={(name) => run("Switch branch", () => api.switchBranch(name, false), `Switched to ${name}`)}
        onSwitchRemote={switchRemote}
        onCreate={(name) => run("Create branch", () => api.switchBranch(name, true), `Switched to new branch ${name}`)}
        onMerge={merge}
        onRebase={(name) => run("Rebase", () => api.rebase(name), `Rebased onto ${name}`)}
        onTerminal={branchTerminal}
        onDelete={deleteBranch}
        onCleanUp={cleanUp}
      />
      <WorktreePicker worktrees={worktrees} branches={branches} onOpen={onOpenRepo} onTerminal={openTerminal} onMerge={merge} onRemove={removeWorktree} />
      {status && !status.upstream && status.branch && (
        <span className="flex shrink-0 items-center gap-1 rounded-sm px-1.5 py-0.5 text-[11px] text-subtle select-none">
          <CloudOff className="size-3" /> Not published
        </span>
      )}

      {/* Filler: grabbing the bar anywhere empty moves the window. */}
      <div data-tauri-drag-region className="min-w-4 flex-1 self-stretch" />

      <UndoControls repo={repo} disabled={!!busy} />
      <div className="mx-1 h-4 w-px bg-border-strong" />
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
        // Where the push lands, which a fork can set apart from where it pulls (upstream/dev
        // pulled, origin/dev pushed). Not there yet: the push creates it.
        <Tip label={status.push?.branch ? `Push to ${status.push.branch}` : `Push to ${status.push?.remote ?? "the remote"} (creates the branch there)`}>
          <Button variant={pushAhead !== 0 ? "default" : "secondary"} disabled={!!busy} onClick={push}>
            <ArrowUpFromLine /> Push
            {!!pushAhead && <span className="font-mono text-[10.5px]">{pushAhead}</span>}
          </Button>
        </Tip>
      ) : (
        <PublishButton
          remotes={status?.remotes ?? []}
          preferred={status?.publish ?? null}
          disabled={!!busy || !status?.branch}
          onPublish={(remote) => run("Publish", () => api.push(false, remote), `Branch published to ${remote}`)}
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

/**
 * Undo and redo for the git actions taken in the app, with ⌘Z / ⇧⌘Z, and their history:
 * picking an entry undoes it and everything after it (or redoes up to it).
 */
function UndoControls({ repo, disabled }: { repo: RepoData; disabled: boolean }) {
  const [moving, setMoving] = useState(false);
  const undoKey = useShortcut("git.undo");
  const redoKey = useShortcut("git.redo");
  const journal = repo.journal;
  const undos = journal?.undo ?? [];
  const redos = journal?.redo ?? [];
  const off = disabled || moving;

  const go = async (forward: boolean, ids: number[]) => {
    setMoving(true);
    try {
      await travel(forward, ids, repo.refresh);
    } finally {
      setMoving(false);
    }
  };
  // The shortcut says why nothing happened; the buttons are disabled instead.
  const next = (forward: boolean) => {
    const e = forward ? redos[0] : undos[0];
    const blocked = forward ? journal?.redoBlocked : journal?.undoBlocked;
    const verb = forward ? "redo" : "undo";
    if (off) return;
    if (!e) toast("info", `Nothing to ${verb}`, "Commits, merges, pulls, branch changes and discards made in GitViber can be undone.");
    else if (blocked) toast("error", `Can't ${verb} ${e.label}`, blocked);
    else void go(forward, [e.id]);
  };
  useCommands({ "git.undo": () => next(false), "git.redo": () => next(true) });

  const button = (forward: boolean) => {
    const e = forward ? redos[0] : undos[0];
    const blocked = forward ? journal?.redoBlocked : journal?.undoBlocked;
    const verb = forward ? "Redo" : "Undo";
    return (
      <Tooltip>
        <TooltipTrigger asChild>
          <span>
            <Button variant="ghost" size="icon" aria-label={verb} disabled={off || !e || !!blocked} onClick={() => e && go(forward, [e.id])}>
              {forward ? <Redo2 /> : <Undo2 />}
            </Button>
          </span>
        </TooltipTrigger>
        <TooltipContent className="max-w-80">
          {!e ? `Nothing to ${verb.toLowerCase()}` : blocked ? `Can't ${verb.toLowerCase()} ${e.label}. ${blocked}` : `${verb} ${e.label}`}
          <span className="ml-2 font-mono text-[11px] text-subtle">{forward ? redoKey : undoKey}</span>
        </TooltipContent>
      </Tooltip>
    );
  };

  const row = (e: JournalEntry, forward: boolean, ids: number[], blocked: boolean) => (
    <DropdownMenuItem
      key={e.id}
      disabled={blocked}
      onSelect={() => go(forward, ids)}
      title={forward ? `Redo up to ${e.label}` : `Undo back to before ${e.label}`}
      className={cn(forward && "text-subtle")}
    >
      {forward ? <Redo2 /> : <Undo2 />}
      <span className="min-w-0 flex-1 truncate">{e.label}</span>
      <span className="shrink-0 text-[11px] opacity-70">{relativeTime(e.time)}</span>
    </DropdownMenuItem>
  );
  const blocked = journal?.undoBlocked ?? journal?.redoBlocked;

  return (
    <div className="flex shrink-0 items-center">
      {button(false)}
      {button(true)}
      <DropdownMenu>
        <Tip label="Undo history">
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" className="w-5 px-0" disabled={off} aria-label="Undo history">
              <ChevronDown className="size-3" />
            </Button>
          </DropdownMenuTrigger>
        </Tip>
        <DropdownMenuContent align="end" className="w-80">
          <DropdownMenuLabel>Undo history</DropdownMenuLabel>
          {!undos.length && !redos.length && (
            <div className="px-2 py-1.5 text-[12px] text-muted-foreground">Commits, merges, pulls and branch changes you make in GitViber show up here, to undo and redo.</div>
          )}
          {/* Furthest redo on top, so the list reads newest to oldest. */}
          {redos
            .map((e, i) => row(e, true, redos.slice(0, i + 1).map((x) => x.id), !!journal?.redoBlocked))
            .reverse()}
          {redos.length > 0 && undos.length > 0 && (
            <div className="flex items-center gap-2 px-2 py-0.5 text-[10.5px] tracking-wide text-subtle uppercase select-none">
              <span className="h-px flex-1 bg-border" /> Now <span className="h-px flex-1 bg-border" />
            </div>
          )}
          {undos.map((e, i) =>
            row(
              e,
              false,
              undos.slice(0, i + 1).map((x) => x.id),
              !!journal?.undoBlocked,
            ),
          )}
          {blocked && (
            <>
              <DropdownMenuSeparator />
              <div className="flex gap-2 px-2 py-1.5 text-[11.5px] text-muted-foreground">
                <TriangleAlert className="mt-0.5 size-3.5 shrink-0 text-conflict" />
                {blocked}
              </div>
            </>
          )}
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}

function ProjectSwitcher({ repo, main, recent, onOpenRepo, onForgetRepo, onReorderRepos, onLocateRepo }: Props) {
  const [open, setOpen] = useState(false);
  const totals = changeTotals(repo);
  const name = main.split("/").pop() ?? main;
  const openKey = useShortcut("file.openRepo");
  const pick = (p?: string) => {
    setOpen(false);
    onOpenRepo(p);
  };
  const locate = (p: string) => {
    setOpen(false);
    onLocateRepo(p);
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
          <ProjectList recent={recent} current={main} onOpen={pick} onForget={onForgetRepo} onReorder={onReorderRepos} onLocate={locate} />
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

/** Publishes to `preferred` (see git.rs `publish_remote`); with several remotes, any can be picked. */
function PublishButton({ remotes, preferred, disabled, onPublish }: { remotes: string[]; preferred: string | null; disabled: boolean; onPublish: (remote: string) => void }) {
  if (!remotes.length) {
    return (
      <Tip label="This repository has no remote. Add one (git remote add origin <url>) to publish.">
        <span>
          <Button disabled>
            <UploadCloud /> Publish
          </Button>
        </span>
      </Tip>
    );
  }
  const menu = (
    <DropdownMenuContent align="end" className="w-56">
      <DropdownMenuLabel>Publish to</DropdownMenuLabel>
      {remotes.map((r) => (
        <DropdownMenuItem key={r} onSelect={() => onPublish(r)}>
          <UploadCloud /> {r}
          {r === preferred && <Check className="ml-auto" />}
        </DropdownMenuItem>
      ))}
    </DropdownMenuContent>
  );
  if (!preferred) {
    return (
      <DropdownMenu>
        <Tip label="Choose a remote to push this branch to and track it">
          <DropdownMenuTrigger asChild>
            <Button disabled={disabled}>
              <UploadCloud /> Publish <ChevronDown className="size-3" />
            </Button>
          </DropdownMenuTrigger>
        </Tip>
        {menu}
      </DropdownMenu>
    );
  }
  return (
    <div className="flex">
      <Tip label={`Push this branch to ${preferred} and track it`}>
        <Button className={cn(remotes.length > 1 && "rounded-r-none")} disabled={disabled} onClick={() => onPublish(preferred)}>
          <UploadCloud /> Publish
        </Button>
      </Tip>
      {remotes.length > 1 && (
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button className="w-5 rounded-l-none border-l border-l-black/20 px-0" disabled={disabled} aria-label="Publish to another remote">
              <ChevronDown className="size-3" />
            </Button>
          </DropdownMenuTrigger>
          {menu}
        </DropdownMenu>
      )}
    </div>
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
