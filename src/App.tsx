import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { ask } from "@/lib/app/ask";
import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { Splash } from "@/components/Splash";
import { Toaster } from "@/components/Toaster";
import { Button } from "@/components/ui/button";
import { TooltipProvider } from "@/components/ui/tooltip";
import { AboutDialog } from "@/features/app/AboutDialog";
import { UpdateDialog } from "@/features/app/UpdateDialog";
import { CloneDialog, openClone } from "@/features/projects/CloneDialog";
import { CommandPalette, showCommands } from "@/features/palette/CommandPalette";
import { IdentityDialog } from "@/features/app/IdentityDialog";
import { NeedsGit } from "@/features/app/NeedsGit";
import { PromptDialog } from "@/features/app/PromptDialog";
import { openSettings, SettingsDialog } from "@/features/settings/SettingsDialog";
import { ShortcutOverlay } from "@/features/app/ShortcutOverlay";
import { Welcome } from "@/features/projects/Welcome";
import { Workspace } from "@/features/workspace/Workspace";
import { api, errorMessage, type GitInfo, NOT_A_REPO, type OpenedRepo } from "@/lib/api";
import { useCommands } from "@/lib/commands/keybindings";
import { useRecentMenu } from "@/lib/commands/menu";
import { stepUiScale } from "@/lib/settings";
import { forgetRepo, lastRepo, recentRepos, rememberRepo, setLastRepo, setRepoOrder } from "@/lib/repo/recent";
import { toast } from "@/lib/app/toast";
import { openTargetIn } from "@/lib/links/linkHost";
import { folderName, isInside } from "@/lib/path";
import { IS_MAC } from "@/lib/platform";

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
      if (info.state === "old") toast("error", `git ${info.version} is older than GitViber needs (${info.minimum})`, `Worktrees and some actions will fail. Update git${IS_MAC ? ", e.g. brew install git" : " with your package manager"}.`);
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

  // Two quick opens could finish in either order, leaving the backend on one folder and the window
  // on the other. So they run one at a time, each shown once it opened, and one a later open
  // overtook before it started is skipped: both end on the last folder that opened.
  const opens = useRef({ last: 0, queue: Promise.resolve() as Promise<unknown> });

  /**
   * `replacing`: a saved project whose folder moved; this repo takes its place in the list. Its root
   * once open; false: it failed; undefined: nothing to do (no folder picked, or a later open took over).
   */
  const openRepo = useCallback(async (path?: string, quiet = false, replacing?: string): Promise<string | false | undefined> => {
    const target = path ?? (await open({ directory: true, title: "Open a git repository" }));
    if (typeof target !== "string") return;
    const seq = ++opens.current.last;
    const latest = () => seq === opens.current.last;
    const turn = opens.current.queue.then(() => (latest() ? api.openRepo(target) : null));
    opens.current.queue = turn.catch(() => {});
    try {
      const repo = await turn;
      if (!repo) return;
      // Projects are keyed by the main worktree; its other worktrees are reached from the top bar.
      // An older entry saved under this worktree's path, or the moved folder, turns into its project.
      setRepoOrder([...new Set(recentRepos().map((p) => (p === repo.root || p === replacing ? repo.main : p)))]);
      rememberRepo(repo.main);
      setLastRepo(repo.root);
      setRecent(recentRepos());
      setOpened(repo);
      return repo.root;
    } catch (e) {
      if (!latest()) return;
      if (quiet) return false;
      if (e === NOT_A_REPO && (await initAsked(target))) return openRepo(target, quiet, replacing);
      toast("error", "Could not open repository", errorMessage(e));
      return false;
    }
  }, []);

  // The last path opened from outside wins (opened.rs); a file shows once its repository is open.
  const openAsked = useCallback(async () => {
    const { open: targets, missing } = await api.takeOpened().catch(() => ({ open: [], missing: [] }));
    for (const path of missing) toast("error", "No such file or folder", path);
    const asked = targets.at(-1);
    if (!asked) return false;
    const root = await openRepo(asked.folder);
    if (root && asked.file) openTargetIn(root, { path: asked.file, line: asked.line ?? undefined, column: asked.column ?? undefined });
    return root !== false;
  }, [openRepo]);

  const booted = useRef<Promise<void>>(Promise.resolve());
  // Reopen the last repository on launch, unless the launch named one (`gitviber <path>`, a
  // folder dropped on the Dock icon). Agent worktrees are short-lived: if the last one is gone,
  // fall back to the project it was under, else the first project.
  useEffect(() => {
    const last = lastRepo();
    const fallback = fallbackFor(last);
    booted.current = (async () => {
      if (await openAsked()) return;
      // Not false: it opened, or something opened meanwhile took over.
      if (last && (await openRepo(last, true)) !== false) return;
      if (fallback) await openRepo(fallback, true);
    })().finally(() => setBooting(false));
  }, [openRepo, openAsked]);

  // The open worktree was deleted from outside (an agent done with it, `git worktree remove`):
  // its project takes its place, as at launch.
  const onRepoGone = useCallback(
    async (gone: OpenedRepo) => {
      const next = fallbackFor(gone.root, gone.main);
      const root = next ? await openRepo(next, true) : false;
      if (root === false) setOpened(null);
      toast("info", `${folderName(gone.root)} no longer exists`, root ? `Opened ${folderName(root)} in its place.` : undefined);
    },
    [openRepo],
  );

  // Paths opened from outside while the app runs (opened.rs): the last one wins. One that comes
  // during the launch's own reopen waits for it (`booted`), so the last repository can't replace it.
  useEffect(() => {
    let live = true;
    let unlisten: Promise<() => void> | undefined;
    try {
      unlisten = listen("opened", async () => {
        await booted.current;
        if (live) await openAsked();
      });
      unlisten.catch(() => {});
    } catch {
      // Not in Tauri (the browser-only dev fixture).
    }
    return () => {
      live = false;
      void unlisten?.then((stop) => stop()).catch(() => {});
    };
  }, [openAsked]);

  const onOpen = useCallback((p?: string) => openRepo(p).then(() => {}), [openRepo]);
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
          <Workspace root={opened.root} main={opened.main} recent={recent} onOpenRepo={onOpen} onForgetRepo={onForget} onReorderRepos={onReorder} onLocateRepo={onLocate} onRepoGone={onRepoGone} />
        </WorkspaceBoundary>
      ) : (
        !booting &&
        (git && noGit ? <NeedsGit info={git} onRecheck={recheckGit} /> : <Welcome recent={recent} onOpenRepo={onOpen} onForgetRepo={onForget} onReorderRepos={onReorder} onLocateRepo={onLocate} />)
      )}
      <IdentityDialog root={opened?.root ?? null} />
      <CloneDialog onCloned={onOpen} />
      <SettingsDialog />
      <AboutDialog />
      <PromptDialog />
      <UpdateDialog />
      <CommandPalette />
      <ShortcutOverlay />
      <Toaster />
      <Splash ready={!booting} />
    </TooltipProvider>
  );
}

/** What to open in place of `gone`: its main worktree if that's another, else the project it was inside, else the first. */
function fallbackFor(gone: string | null, main?: string) {
  const projects = recentRepos().filter((p) => p !== gone);
  return (main !== gone ? main : undefined) ?? projects.find((p) => gone && isInside(gone, p)) ?? projects[0];
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
function WorkspaceBoundary({ children, onOpenRepo }: { children: ReactNode; onOpenRepo: () => void }) {
  return (
    <ErrorBoundary
      fallback={(e, componentStack) => (
        <div data-tauri-drag-region className="flex h-full flex-col items-center justify-center gap-3 bg-background p-6">
          <div className="text-[13px] font-medium">Something went wrong in this window</div>
          <pre className="max-h-[50vh] max-w-3xl overflow-auto rounded-md border border-border bg-panel p-3 font-mono text-[11px] whitespace-pre-wrap text-subtle select-text">
            {/* The component stack names what threw; the JS stack alone is minified in release builds. */}
            {`${e instanceof Error && e.stack ? e.stack : errorMessage(e)}\n${componentStack}`}
          </pre>
          <div className="flex gap-2">
            <Button onClick={() => location.reload()}>Reload</Button>
            <Button variant="secondary" onClick={() => onOpenRepo()}>
              Open another repository…
            </Button>
          </div>
        </div>
      )}
    >
      {children}
    </ErrorBoundary>
  );
}
