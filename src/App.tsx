import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useState } from "react";
import { Toaster } from "@/components/Toaster";
import { TooltipProvider } from "@/components/ui/tooltip";
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
      {opened ? <Workspace key={opened.root} root={opened.root} main={opened.main} recent={recent} onOpenRepo={onOpen} onForgetRepo={onForget} onReorderRepos={onReorder} /> : !booting && <Welcome recent={recent} onOpenRepo={onOpen} />}
      <SettingsDialog />
      <Toaster />
    </TooltipProvider>
  );
}
