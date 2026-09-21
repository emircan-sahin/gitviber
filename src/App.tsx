import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useState } from "react";
import { Toaster } from "@/components/Toaster";
import { TooltipProvider } from "@/components/ui/tooltip";
import { Welcome } from "@/features/Welcome";
import { Workspace } from "@/features/Workspace";
import { api, errorMessage } from "@/lib/api";
import { forgetRepo, lastRepo, recentRepos, rememberRepo, setLastRepo, setRepoOrder } from "@/lib/settings";
import { toast } from "@/lib/toast";

export function App() {
  const [root, setRoot] = useState<string | null>(null);
  const [recent, setRecent] = useState(recentRepos);
  const [booting, setBooting] = useState(true);

  const openRepo = useCallback(async (path?: string, quiet = false) => {
    const target = path ?? (await open({ directory: true, title: "Open a git repository" }));
    if (typeof target !== "string") return;
    try {
      const top = await api.openRepo(target);
      rememberRepo(top);
      setLastRepo(top);
      setRecent(recentRepos());
      setRoot(top);
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
      {root ? <Workspace key={root} root={root} recent={recent} onOpenRepo={onOpen} onForgetRepo={onForget} onReorderRepos={onReorder} /> : !booting && <Welcome recent={recent} onOpenRepo={onOpen} />}
      <Toaster />
    </TooltipProvider>
  );
}
