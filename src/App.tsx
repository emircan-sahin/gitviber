import { ask, open } from "@tauri-apps/plugin-dialog";
import { Component, type ErrorInfo, type ReactNode, useCallback, useEffect, useState } from "react";
import { Splash } from "@/components/Splash";
import { Toaster } from "@/components/Toaster";
import { Button } from "@/components/ui/button";
import { TooltipProvider } from "@/components/ui/tooltip";
import { AboutDialog } from "@/features/AboutDialog";
import { CloneDialog, openClone } from "@/features/CloneDialog";
import { CommandPalette, showCommands } from "@/features/CommandPalette";
import { IdentityDialog } from "@/features/IdentityDialog";
import { NeedsGit } from "@/features/NeedsGit";
import { openSettings, SettingsDialog } from "@/features/SettingsDialog";
import { ShortcutOverlay } from "@/features/ShortcutOverlay";
import { Welcome } from "@/features/Welcome";
import { Workspace } from "@/features/Workspace";
import { api, errorMessage, type GitInfo, NOT_A_REPO, type OpenedRepo } from "@/lib/api";
import { useCommands } from "@/lib/keybindings";
import { useRecentMenu } from "@/lib/menu";
import { forgetRepo, lastRepo, recentRepos, rememberRepo, setLastRepo, setRepoOrder, stepUiScale } from "@/lib/settings";
import { toast } from "@/lib/toast";
import { folderName, isInside } from "@/lib/worktrees";

export function App() {
  const [opened, setOpened] = useState<OpenedRepo | null>(null);
  const [recent, setRecent] = useState(recentRepos);
  const [booting, setBooting] = useState(true);
  const [git, setGit] = useState<GitInfo | null>(null);

  // Checked alongside the reopen below, not before it: the usual answer is "fine".
  useEffect(() => {
    let live = true;
    api.gitInfo().then((info) => {
      if (!live) return;
      setGit(info);
      // Errors stay until dismissed, and an old git does fail: every repo open lists worktrees.
      if (info.state === "old") toast("error", `git ${info.version} is older than GitViber needs (${info.minimum})`, "Worktrees and some actions will fail. Update git, e.g. brew install git.");
    }, () => {});
    return () => {
      live = false;
    };
  }, []);
  const recheckGit = useCallback(async () => {
    const info = await api.gitInfo(true).catch(() => null);
    if (info) setGit(info);
    if (info?.state === "ok" || info?.state === "old") toast("success", `Found git ${info.version}`);
  }, []);
  const noGit = git?.state === "missing" || git?.state === "tools";

  /** `replacing`: a saved project whose folder moved; this repo takes its place in the list. */
  const openRepo = useCallback(async (path?: string, quiet = false, replacing?: string) => {
    const target = path ?? (await open({ directory: true, title: "Open a git repository" }));
    if (typeof target !== "string") return;
    try {
      const repo = await api.openRepo(target);
      // Projects are keyed by the main worktree; its other worktrees are reached from the top bar.
      // An older entry saved under this worktree's path, or the moved folder, turns into its project.
      setRepoOrder([...new Set(recentRepos().map((p) => (p === repo.root || p === replacing ? repo.main : p)))]);
      rememberRepo(repo.main);
      setLastRepo(repo.root);
      setRecent(recentRepos());
      setOpened(repo);
      return true;
    } catch (e) {
      if (quiet) return false;
      if (e === NOT_A_REPO && (await initAsked(target))) return openRepo(target, quiet, replacing);
      toast("error", "Could not open repository", errorMessage(e));
      return false;
    }
  }, []);

  // Reopen the last repository on launch. Agent worktrees are short-lived: if the last one
  // is gone, fall back to the project it was under, else the first project.
  useEffect(() => {
    const last = lastRepo();
    const projects = recentRepos();
    const fallback = projects.find((p) => last && isInside(last, p)) ?? projects[0];
    (async () => {
      if (last && (await openRepo(last, true))) return;
      if (fallback && fallback !== last) await openRepo(fallback, true);
    })().finally(() => setBooting(false));
  }, [openRepo]);

  const onOpen = useCallback((p?: string) => void openRepo(p), [openRepo]);
  const onReorder = useCallback((list: string[]) => {
    setRepoOrder(list);
    setRecent(list);
  }, []);
  const onLocate = useCallback(
    async (p: string) => {
      const target = await open({ directory: true, title: `Locate ${folderName(p)}` });
      if (typeof target === "string") await openRepo(target, false, p);
    },
    [openRepo],
  );
  const onForget = useCallback((p: string) => {
    forgetRepo(p);
    setRecent(recentRepos());
  }, []);
  // The open project stays: the top bar still shows it.
  const onClearRecent = useCallback(() => {
    setRepoOrder(opened ? [opened.main] : []);
    setRecent(recentRepos());
  }, [opened]);
  useRecentMenu(recent, onOpen, onClearRecent);

  // App-wide commands, so they also work on the welcome screen.
  useCommands({
    "file.openRepo": () => onOpen(),
    "file.cloneRepo": openClone,
    "workbench.openSettings": () => openSettings(),
    "workbench.showCommands": showCommands,
    "help.shortcuts": () => openSettings("shortcuts"),
    "view.zoomIn": () => stepUiScale(1),
    "view.zoomOut": () => stepUiScale(-1),
    "view.zoomReset": () => stepUiScale(0),
    "window.reload": () => location.reload(),
  });

  return (
    <TooltipProvider>
      {opened ? (
        <WorkspaceBoundary key={opened.root} onOpenRepo={onOpen}>
          <Workspace root={opened.root} main={opened.main} recent={recent} onOpenRepo={onOpen} onForgetRepo={onForget} onReorderRepos={onReorder} onLocateRepo={onLocate} />
        </WorkspaceBoundary>
      ) : (
        !booting &&
        (git && noGit ? <NeedsGit info={git} onRecheck={recheckGit} /> : <Welcome recent={recent} onOpenRepo={onOpen} onForgetRepo={onForget} onReorderRepos={onReorder} onLocateRepo={onLocate} />)
      )}
      <IdentityDialog root={opened?.root ?? null} />
      <CloneDialog onCloned={onOpen} />
      <SettingsDialog />
      <AboutDialog />
      <CommandPalette />
      <ShortcutOverlay />
      <Toaster />
      <Splash ready={!booting} />
    </TooltipProvider>
  );
}

/** A folder outside any repository: offer to make it one. True once it is. */
async function initAsked(path: string) {
  const ok = await ask(`${folderName(path)} isn't a git repository yet. Initialize one here?`, {
    title: "Open repository",
    okLabel: "Initialize repository",
  });
  if (!ok) return false;
  try {
    await api.initRepo(path);
    return true;
  } catch (e) {
    toast("error", "Could not initialize repository", errorMessage(e));
    return false;
  }
}

/** Without this, a render error anywhere in the workspace unmounts the app and leaves a black window. */
class WorkspaceBoundary extends Component<{ children: ReactNode; onOpenRepo: () => void }, { error: string | null }> {
  state = { error: null as string | null };
  static getDerivedStateFromError(e: unknown) {
    return { error: e instanceof Error && e.stack ? e.stack : errorMessage(e) };
  }
  // Logged with its component stack by main.tsx.
  componentDidCatch(_: unknown, info: ErrorInfo) {
    // The component stack names what threw; the JS stack alone is minified in release builds.
    this.setState((s) => ({ error: `${s.error}\n${info.componentStack ?? ""}` }));
  }
  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div data-tauri-drag-region className="flex h-full flex-col items-center justify-center gap-3 bg-background p-6">
        <div className="text-[13px] font-medium">Something went wrong in this window</div>
        <pre className="max-h-[50vh] max-w-3xl overflow-auto rounded-md border border-border bg-panel p-3 font-mono text-[11px] whitespace-pre-wrap text-subtle select-text">{this.state.error}</pre>
        <div className="flex gap-2">
          <Button onClick={() => location.reload()}>Reload</Button>
          <Button variant="secondary" onClick={() => this.props.onOpenRepo()}>
            Open another repository…
          </Button>
        </div>
      </div>
    );
  }
}
