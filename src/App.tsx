import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useState } from "react";
import { Toaster } from "@/components/Toaster";
import { TooltipProvider } from "@/components/ui/tooltip";
import { Welcome } from "@/features/Welcome";
import { Workspace } from "@/features/Workspace";
import { api, errorMessage, type OpenedRepo } from "@/lib/api";
import { forgetRepo, lastRepo, recentRepos, rememberRepo, setLastRepo, setRepoOrder } from "@/lib/settings";
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
      rememberRepo(repo.main);
      setLastRepo(repo.root);
      setRecent(recentRepos());
      setOpened(repo);
    } catch (e) {
      if (!quiet) toast("error", "Could not open repository", errorMessage(e));
    }
  }, []);

  // Reopen the last repository on launch.
  useEffect(() => {
    const last = lastRepo() ?? recentRepos()[0];
    (last ? openRepo(last, true) : Promise.resolve()).finally(() => setBooting(false));
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

  return (
    <TooltipProvider>
      {opened ? <Workspace key={opened.root} root={opened.root} main={opened.main} recent={recent} onOpenRepo={onOpen} onForgetRepo={onForget} onReorderRepos={onReorder} /> : !booting && <Welcome recent={recent} onOpenRepo={onOpen} />}
      <Toaster />
    </TooltipProvider>
  );
}
