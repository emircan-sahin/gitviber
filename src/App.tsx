import { open } from "@tauri-apps/plugin-dialog";
import { Component, type ErrorInfo, type ReactNode, useCallback, useEffect, useState } from "react";
import { Splash } from "@/components/Splash";
import { Toaster } from "@/components/Toaster";
import { Button } from "@/components/ui/button";
import { TooltipProvider } from "@/components/ui/tooltip";
import { AboutDialog } from "@/features/AboutDialog";
import { openSettings, SettingsDialog } from "@/features/SettingsDialog";
import { Welcome } from "@/features/Welcome";
import { Workspace } from "@/features/Workspace";
import { api, errorMessage, type OpenedRepo } from "@/lib/api";
import { useCommands } from "@/lib/keybindings";
import { forgetRepo, lastRepo, recentRepos, rememberRepo, setLastRepo, setRepoOrder, stepUiScale } from "@/lib/settings";
import { toast } from "@/lib/toast";

export function App() {
  const [opened, setOpened] = useState<OpenedRepo | null>(null);
  const [recent, setRecent] = useState(recentRepos);
  const [booting, setBooting] = useState(true);

  const openRepo = useCallback(async (path?: string, quiet = false) => {
    const target = path ?? (await open({ directory: true, title: "Open a git repository" }));
    if (typeof target !== "string") return;
    try {
      const repo = await api.openRepo(target);
      // Projects are keyed by the main worktree; its other worktrees are reached from the top bar.
      // An older entry saved under this worktree's path turns into its project.
      if (repo.root !== repo.main) setRepoOrder([...new Set(recentRepos().map((p) => (p === repo.root ? repo.main : p)))]);
      rememberRepo(repo.main);
      setLastRepo(repo.root);
      setRecent(recentRepos());
      setOpened(repo);
      return true;
    } catch (e) {
      if (!quiet) toast("error", "Could not open repository", errorMessage(e));
      return false;
    }
  }, []);

  // Reopen the last repository on launch. Agent worktrees are short-lived: if the last one
  // is gone, fall back to the project it was under, else the first project.
  useEffect(() => {
    const last = lastRepo();
    const projects = recentRepos();
    const fallback = projects.find((p) => last?.startsWith(`${p}/`)) ?? projects[0];
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
  const onForget = useCallback((p: string) => {
    forgetRepo(p);
    setRecent(recentRepos());
  }, []);

  // App-wide commands, so they also work on the welcome screen.
  useCommands({
    "file.openRepo": () => onOpen(),
    "workbench.openSettings": () => openSettings(),
    "view.zoomIn": () => stepUiScale(1),
    "view.zoomOut": () => stepUiScale(-1),
    "view.zoomReset": () => stepUiScale(0),
  });

  return (
    <TooltipProvider>
      {opened ? (
        <WorkspaceBoundary key={opened.root} onOpenRepo={onOpen}>
          <Workspace root={opened.root} main={opened.main} recent={recent} onOpenRepo={onOpen} onForgetRepo={onForget} onReorderRepos={onReorder} />
        </WorkspaceBoundary>
      ) : (
        !booting && <Welcome recent={recent} onOpenRepo={onOpen} />
      )}
      <SettingsDialog />
      <AboutDialog />
      <Toaster />
      <Splash ready={!booting} />
    </TooltipProvider>
  );
}

/** Without this, a render error anywhere in the workspace unmounts the app and leaves a black window. */
class WorkspaceBoundary extends Component<{ children: ReactNode; onOpenRepo: () => void }, { error: string | null }> {
  state = { error: null as string | null };
  static getDerivedStateFromError(e: unknown) {
    return { error: e instanceof Error && e.stack ? e.stack : errorMessage(e) };
  }
  componentDidCatch(e: unknown, info: ErrorInfo) {
    console.error(e, info.componentStack);
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
