import { ChevronsDownUp, GitCompareArrows, PanelLeftClose, PanelRightClose, Search } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useDefaultLayout, usePanelRef } from "react-resizable-panels";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@/components/ui/resizable";
import { Tip } from "@/components/ui/tooltip";
import { find } from "@/lib/ui/find";
import { resetGitHubCache, setGitHubOrigin } from "@/lib/github/githubCache";
import { warmHighlighter } from "@/lib/editor/highlight";
import { setLinkHost } from "@/lib/links/linkHost";
import { prepare } from "@/lib/editor/monaco";
import { useCommands, useShortcut } from "@/lib/commands/keybindings";
import { dropReveal, revealWaits } from "@/lib/editor/reveal";
import { type ChangeList, onDisk, type Selection, selectionKey, selectionPath } from "@/lib/repo/selection";
import { codeWantsFocus, focusedPanel, focusList, focusPanel, type Panel, PANELS } from "@/lib/ui/panels";
import { loadWorkspace, saveWorkspace } from "@/lib/repo/session";
import { DEFAULT_FONT_SIZE, updateSettings, useSettings } from "@/lib/settings";
import { goGroup, stepGroup, unmaximize, useTerminalsMaximized, useTerminalsOpen, useTerminalTabCount } from "@/lib/terminal/terminals";
import { useRepo } from "@/lib/repo/useRepo";
import { type OpenedRepo } from "@/lib/api";
import { reviewBase, shortRef } from "@/lib/git/refs";
import { cn } from "@/lib/utils";
import { revealPath } from "@/lib/app/openIn";
import { ChangesPanel } from "@/features/changes/ChangesPanel";
import { changeList } from "@/features/changes/changeList";
import { BranchReview, useBranchReview } from "@/features/changes/BranchReview";
import { showQuickOpen, useQuickOpenSource } from "@/features/palette/CommandPalette";
import { FileTree, type FileTreeHandle } from "@/features/explorer/FileTree";
import { type HistorySearch, NO_SEARCH, SearchableHistory } from "@/features/history/HistorySearch";
import { IssuesPanel } from "@/features/github/issues/IssuesPanel";
import { selectedText } from "@/features/viewer/activeEditor";
import { StatusBar } from "./StatusBar";
import { useTabs } from "./useTabs";
import { useViewed } from "./useViewed";
import { PullsPanel } from "@/features/github/pulls/PullsPanel";
import { SearchView } from "@/features/explorer/SearchView";
import { TerminalPanel, useTerminalSetup } from "@/features/terminal/TerminalPanel";
import { TerminalRestoreOffer } from "@/features/terminal/TerminalFind";
import { TopBar } from "@/features/topbar/TopBar";
import { Viewer } from "@/features/viewer/Viewer";
import { prefetchSelection, resetPairCache } from "@/features/viewer/diffPairs";
import { stackedView } from "@/features/viewer/AllChanges";
import { openEdits } from "@/lib/editor/edits";
import { openNotes, useNoteCheck } from "@/lib/review/noteStore";
import { copyNotes, pendingNotes, sendNotes } from "@/features/review/ReviewNotes";
import { CountBadge } from "@/components/CountBadge";

const LIST_TABS = ["changes", "history", "pulls", "issues"] as const;
type ListTab = (typeof LIST_TABS)[number];

interface Props {
  root: string;
  /** The main worktree; differs from root when a linked worktree is open. */
  main: string;
  recent: string[];
  onOpenRepo: (path?: string) => Promise<void>;
  onForgetRepo: (path: string) => void;
  onReorderRepos: (list: string[]) => void;
  onLocateRepo: (path: string) => void;
  /** The repo's folder was deleted while open. */
  onRepoGone: (repo: OpenedRepo) => void;
}

/**
 * Diffs are cached by revision, which restarts per repo, and GitHub data, unsaved file edits and
 * review notes are per repo: they start over with each repo, during its first render, before
 * anything in it reads them.
 */
function useFreshCaches(root: string) {
  const cleared = useRef<string | null>(null);
  if (cleared.current === root) return;
  cleared.current = root;
  resetPairCache();
  resetGitHubCache();
  openEdits(root);
  openNotes(root);
}

export function Workspace({ root, main, recent, onOpenRepo, onForgetRepo, onReorderRepos, onLocateRepo, onRepoGone }: Props) {
  useFreshCaches(root);
  const repo = useRepo(root, () => onRepoGone({ root, main }));
  const { status } = repo;
  const origin = status?.origin;
  // origin's page on GitHub, for links; kept while git couldn't read the remotes, as the origin is.
  const [webUrl, setWebUrl] = useState<string | null>(null);
  const web = status?.webUrl;
  useEffect(() => {
    if (origin !== undefined) setGitHubOrigin(origin);
    if (web !== undefined) setWebUrl(web);
  }, [origin, web]);
  const s = useSettings();
  const [saved] = useState(() => loadWorkspace(root));
  const [listTab, setListTab] = useState<ListTab>(() => LIST_TABS.find((t) => t === saved?.listTab) ?? "changes");
  // Here, not in History: the search outlives a switch to another list.
  const [historySearch, setHistorySearch] = useState<HistorySearch>(NO_SEARCH);
  // Until History shows and takes it: a request, not a count, so no later mount repeats it.
  const [searchFocus, setSearchFocus] = useState(false);
  const searchFocused = useCallback(() => setSearchFocus(false), []);
  // Blame clicks so far: each is a new request, even for the commit already on show.
  const reveals = useRef(0);
  // The full ref Changes reviews the branch against, in place of the uncommitted list; null: not reviewing.
  const [review, setReview] = useState<string | null>(() => (typeof saved?.review === "string" ? saved.review : null));
  const reviewing = listTab === "changes" && review !== null;
  // All Branch Changes on show reads the review too, with the list on another tab. It's known once
  // the tabs are, below: a change there renders this again before anything is drawn.
  const [branchOnShow, setBranchOnShow] = useState(false);
  const branchReview = useBranchReview(review, repo.revision, reviewing || branchOnShow);
  const { tabs, activeKey, setActiveKey, open: openTab, closeTabs, close, closeAround, reopen, canReopen, moveTab, goTab, stepTab, pin, onPathMoved } = useTabs(saved, status, branchReview.review && branchReview.rows);
  const branchTab = activeKey === selectionKey({ kind: "changes", list: "branch" });
  if (branchTab !== branchOnShow) setBranchOnShow(branchTab);
  // A file opened while the terminal covers the code view (⌘P, a path clicked in the terminal) comes into view.
  const open = useCallback(
    (sel: Selection, pin?: boolean) => {
      unmaximize();
      openTab(sel, pin);
    },
    [openTab],
  );
  const { viewedMap, viewed, setViewed, toggleViewed } = useViewed(saved, status, repo.refresh, branchReview.review);
  useNoteCheck(repo.revision);
  useEffect(() => saveWorkspace(root, { tabs, active: activeKey, listTab, viewed: [...viewedMap], review }), [root, tabs, activeKey, listTab, viewedMap, review]);
  // Git work on the left, files on the right; both collapse to give code the room.
  const listPanel = usePanelRef();
  const filesPanel = usePanelRef();
  useTerminalSetup(root);
  // Where a terminal can start besides this repo's worktrees, without switching the window there.
  const projects = recent.filter((p) => p !== main);
  const terminalOpen = useTerminalsOpen();
  const terminalMaximized = useTerminalsMaximized();
  // goGroup and stepGroup are read as this renders: a tab opened since was out of ⌘1–⌘9's reach.
  useTerminalTabCount();
  const fileTree = useRef<FileTreeHandle>(null);
  const findKey = useShortcut("editor.find");
  // The explorer panel shows the files or Search in Files; `searchAsk` brings the search box up.
  const [explorerView, setExplorerView] = useState<"files" | "search">("files");
  const [searchAsk, setSearchAsk] = useState({ id: 0, seed: "" });
  const [leftOpen, setLeftOpen] = useState(true);
  const [rightOpen, setRightOpen] = useState(true);
  // Focus goes to a panel once it's rendered: after a view switch or with the panel expanded.
  const [focusTo, setFocusTo] = useState<{ panel: Panel } | null>(null);
  useEffect(() => {
    if (focusTo) focusPanel(focusTo.panel);
  }, [focusTo]);
  const show = useCallback((panel: typeof listPanel, name: Panel) => {
    panel.current?.expand();
    setFocusTo({ panel: name });
  }, []);
  // Opening a panel focuses it, as in VS Code; closing the one holding focus leaves it to the code view.
  const toggle = useCallback(
    (panel: typeof listPanel, name: Panel) => {
      if (panel.current?.isCollapsed()) return show(panel, name);
      const had = focusedPanel() === name;
      panel.current?.collapse();
      if (had) focusPanel("code");
    },
    [show],
  );
  const showList = (tab: ListTab) => {
    setListTab(tab);
    show(listPanel, "git");
  };
  const cycle = (dir: 1 | -1) => {
    const open = PANELS.filter((p) => (p === "git" ? leftOpen : p === "explorer" ? rightOpen : p === "terminal" ? terminalOpen : true));
    const at = open.indexOf(focusedPanel() as Panel);
    focusPanel(open[at < 0 ? (dir > 0 ? 0 : open.length - 1) : (at + dir + open.length) % open.length]);
  };
  const revealInExplorer = useCallback((path: string) => {
    filesPanel.current?.expand();
    setExplorerView("files");
    // Once the tree shows: a hidden one can't take focus.
    requestAnimationFrame(() => fileTree.current?.reveal(path));
  }, []);
  const showInHistory = useCallback((search: HistorySearch) => {
    setHistorySearch(search);
    setListTab("history");
    listPanel.current?.expand();
  }, []);
  const showHistory = useCallback((path: string, file: boolean) => showInHistory({ ...NO_SEARCH, scope: { path, file } }), [showInHistory]);
  const layout = useDefaultLayout({ id: "gitviber-main-v4", storage: localStorage });
  const viewerLayout = useDefaultLayout({ id: "gitviber-viewer-v1", storage: localStorage, panelIds: terminalOpen ? ["editor", "terminal"] : ["editor"] });

  const uncommitted = useMemo(() => (status ? changeList(status) : []), [status]);
  // What J/K walk: the list Changes shows.
  const changes: Selection[] = review === null ? uncommitted : branchReview.rows;
  const startReview = () => {
    setReview((r) => r ?? reviewBase(repo.branches) ?? "");
    showList("changes");
  };
  const reviewLabel = shortRef(review || reviewBase(repo.branches) || "") || "a base branch";
  const remoteNames = useMemo(() => new Set(repo.branches.filter((b) => b.remote).map((b) => b.name)), [repo.branches]);

  // A merge/rebase that stopped on conflicts: bring the conflicts into view.
  const conflictCount = status?.conflicted.length ?? 0;
  useEffect(() => {
    if (conflictCount === 0) return;
    setListTab("changes");
    setReview(null);
  }, [conflictCount]);

  const prefetch = useCallback((sel: Selection) => prefetchSelection(sel, repo.revision), [repo.revision]);

  // Load the highlighters (and compile their WASM) while the app settles, not on the first file.
  useEffect(() => {
    const t = setTimeout(() => {
      warmHighlighter();
      void prepare("text", s.codeTheme);
    }, 300);
    return () => clearTimeout(t);
    // Once: a theme picked later is loaded when it's applied.
  }, []);

  // Reviewing is sequential: have the neighbours of the open file ready before J/K.
  useEffect(() => {
    const i = changes.findIndex((c) => selectionKey(c) === activeKey);
    if (i < 0) return;
    for (const n of [changes[i + 1], changes[i - 1]]) if (n) prefetchSelection(n, repo.revision);
  }, [changes, activeKey, repo.revision]);

  // J/K walk the changed files, the core loop of reviewing an agent's work; in a stacked view of
  // them, its files.
  const step = (dir: 1 | -1) => {
    if (active?.sel.kind === "changes") return stackedView()?.step(dir);
    if (!changes.length) return;
    const i = changes.findIndex((c) => selectionKey(c) === activeKey);
    open(changes[i < 0 ? 0 : Math.min(changes.length - 1, Math.max(0, i + dir))]);
    setListTab("changes");
  };

  // With focus in the terminal (a pane, its tabs, its find), the tab keys pick its tabs, as in iTerm2.
  // Where focus is now, not the panel it was last in, which may be hidden since. When that side has
  // no such tab, the key goes on (⌘→ to the code view's text).
  const orTerminal = (code: (() => void) | undefined, terminal: (() => void) | undefined) =>
    code || terminal
      ? () => {
          const run = document.activeElement?.closest('[data-panel="terminal"]') ? terminal : code;
          return run ? run() : false;
        }
      : undefined;

  const active = tabs.find((t) => t.key === activeKey) ?? null;
  // From the palette, as quick open: the code view takes the keys, and the stacked view them as it opens.
  const openAll = (list: ChangeList) => {
    focusPanel("code");
    open({ kind: "changes", list }, true);
  };
  useCommands({
    "review.nextFile": () => step(1),
    "review.prevFile": () => step(-1),
    "review.branch": startReview,
    "review.openAll": status?.unstaged.length ? () => openAll("unstaged") : undefined,
    "review.openAllStaged": status?.staged.length ? () => openAll("staged") : undefined,
    "review.openAllBranch": reviewing && branchReview.rows.length ? () => openAll("branch") : undefined,
    "review.copyNotes": () => copyNotes(pendingNotes()),
    "review.sendNotes": () => sendNotes(pendingNotes()),
    "review.toggleViewed": () => {
      const t = tabs.find((x) => x.key === activeKey);
      if (t?.sel.kind === "changes") return stackedView()?.toggleViewed();
      // On a staged file this would unstage it; too much for a stray single key.
      if (t && t.sel.kind !== "staged") toggleViewed(t.sel);
    },
    "diff.toggleSplit": () => updateSettings({ sideBySide: !s.sideBySide }),
    "diff.toggleCollapse": () => updateSettings({ hideUnchanged: !s.hideUnchanged }),
    "diff.toggleWhitespace": () => updateSettings({ ignoreWhitespace: !s.ignoreWhitespace }),
    "editor.toggleWrap": () => updateSettings({ wordWrap: !s.wordWrap }),
    "editor.toggleBlame": () => updateSettings({ blame: !s.blame }),
    "editor.fontZoomIn": () => updateSettings({ codeFontSize: s.codeFontSize + 0.5 }),
    "editor.fontZoomOut": () => updateSettings({ codeFontSize: s.codeFontSize - 0.5 }),
    "editor.fontZoomReset": () => updateSettings({ codeFontSize: DEFAULT_FONT_SIZE }),
    "editor.find": find,
    "search.findInFiles": () => {
      setExplorerView("search");
      filesPanel.current?.expand();
      setSearchAsk((a) => ({ id: a.id + 1, seed: selectedText() }));
    },
    "view.changes": () => showList("changes"),
    "view.history": () => showList("history"),
    "view.pulls": () => showList("pulls"),
    "view.issues": () => showList("issues"),
    "history.search": () => {
      setListTab("history");
      listPanel.current?.expand();
      setSearchFocus(true);
    },
    "view.toggleGitPanel": () => toggle(listPanel, "git"),
    "view.toggleExplorer": () => toggle(filesPanel, "explorer"),
    "view.focusGitPanel": () => show(listPanel, "git"),
    "view.showExplorer": () => {
      setExplorerView("files");
      show(filesPanel, "explorer");
    },
    "view.focusCode": () => void focusPanel("code"),
    "view.focusNextPanel": () => cycle(1),
    "view.focusPrevPanel": () => cycle(-1),
    "tab.close": activeKey ? () => close(activeKey) : undefined,
    "tab.reopenClosed": canReopen ? reopen : undefined,
    "tab.closeOthers": closeAround("others"),
    "tab.closeLeft": closeAround("left"),
    "tab.closeRight": closeAround("right"),
    "tab.closeAll": closeAround("all"),
    "tab.goto1": orTerminal(goTab(0), goGroup(0)),
    "tab.goto2": orTerminal(goTab(1), goGroup(1)),
    "tab.goto3": orTerminal(goTab(2), goGroup(2)),
    "tab.goto4": orTerminal(goTab(3), goGroup(3)),
    "tab.goto5": orTerminal(goTab(4), goGroup(4)),
    "tab.goto6": orTerminal(goTab(5), goGroup(5)),
    "tab.goto7": orTerminal(goTab(6), goGroup(6)),
    "tab.goto8": orTerminal(goTab(7), goGroup(7)),
    "tab.last": orTerminal(goTab(tabs.length - 1), goGroup(-1)),
    "tab.next": orTerminal(stepTab(1), stepGroup(1)),
    "tab.prev": orTerminal(stepTab(-1), stepGroup(-1)),
    "file.reveal": () => revealInFinder(active?.sel),
    "file.revealInExplorer": active && onDisk(active.sel) ? () => revealInExplorer(selectionPath(active.sel)) : undefined,
    "repo.refresh": () => repo.refresh(),
    "workbench.quickOpen": () => showQuickOpen(),
    "workbench.openChange": uncommitted.length ? () => showQuickOpen("changes") : undefined,
    "terminal.newInProject": projects.length ? () => showQuickOpen("projects") : undefined,
  });

  // Quick open's picks take the code view along, where a new tab then takes focus (MonacoView).
  useQuickOpenSource({
    root,
    changes: uncommitted,
    projects,
    openFile: (path) => {
      focusPanel("code");
      open({ kind: "file", path }, true);
    },
    openChange: (change) => {
      focusPanel("code");
      open(change, true);
      setListTab("changes");
    },
  });

  // A tab switched to from the code view keeps the keys there (the view it replaced took focus along).
  useEffect(() => {
    if (codeWantsFocus()) focusPanel("code");
  }, [activeKey]);

  // A search result's reveal waits for its own file only: any other tab shown drops it, so it
  // can't land on a later visit (a file too large to show never takes it).
  useEffect(() => {
    const t = tabs.find((x) => x.key === activeKey);
    if (t?.sel.kind !== "file" || !revealWaits(t.sel.path)) dropReveal();
    // On a tab switch, not on every tab list change.
  }, [activeKey]);

  // ⌘-click in the code view and the terminal opens files here (lib/links/linkHost). After the
  // effect above: a file waiting for this workspace (`gitviber a.ts:12`) opens as it mounts, and
  // the first run above would drop its line.
  useEffect(() => {
    setLinkHost({
      root,
      worktrees: repo.worktrees.map((w) => w.path),
      revision: repo.revision,
      open: (path, focus) => {
        open({ kind: "file", path }, true);
        if (focus) focusPanel("code");
      },
      // A file opened from the terminal is picked in the explorer too; a closed explorer stays closed.
      reveal: (path, show) => {
        if (show) revealInExplorer(path);
        else if (!filesPanel.current?.isCollapsed()) fileTree.current?.reveal(path, false);
      },
    });
  }, [root, repo.worktrees, repo.revision, open, revealInExplorer]);
  useEffect(() => () => setLinkHost(null), []);

  const changeCount = uncommitted.length;

  return (
    <div className="flex h-full flex-col">
      <TopBar
        webUrl={webUrl}
        repo={repo}
        root={root}
        main={main}
        recent={recent}
        onOpenRepo={onOpenRepo}
        onForgetRepo={onForgetRepo}
        onReorderRepos={onReorderRepos}
        onLocateRepo={onLocateRepo}
        onOpenPull={(pull) => open({ kind: "pull", pull }, true)}
        leftOpen={leftOpen}
        rightOpen={rightOpen}
        onToggleLeft={() => toggle(listPanel, "git")}
        onToggleRight={() => toggle(filesPanel, "explorer")}
      />
      <div className="relative min-h-0 flex-1">
        {/* The library finds a divider by where the pointer is: under the maximized terminal, the hidden ones would still drag. */}
        <ResizablePanelGroup orientation="horizontal" defaultLayout={layout.defaultLayout} onLayoutChanged={layout.onLayoutChanged} disabled={terminalMaximized}>
          <ResizablePanel
            id="list"
            panelRef={listPanel}
            defaultSize={320}
            minSize={240}
            maxSize="45"
            collapsible
            collapsedSize={0}
            onResize={(size) => setLeftOpen(size.inPixels > 0)}
          >
            <div data-panel="git" tabIndex={-1} className="group/panel relative flex h-full flex-col bg-panel outline-none">
              <FocusLine />
              <div className="flex h-9 shrink-0 items-center gap-0.5 border-b border-border pr-1 pl-2">
                <ListTabButton active={listTab === "changes"} onClick={() => setListTab("changes")} count={changeCount}>
                  Changes
                </ListTabButton>
                <ListTabButton active={listTab === "history"} onClick={() => setListTab("history")}>
                  History
                </ListTabButton>
                <ListTabButton active={listTab === "pulls"} onClick={() => setListTab("pulls")}>
                  PRs
                </ListTabButton>
                <ListTabButton active={listTab === "issues"} onClick={() => setListTab("issues")}>
                  Issues
                </ListTabButton>
                {/* One group: two ml-autos split the free space. */}
                <div className="ml-auto flex items-center gap-0.5">
                  <Tip label={reviewing ? "Back to uncommitted changes" : `Review branch against ${reviewLabel}`}>
                    <button
                      aria-label={reviewing ? "Back to uncommitted changes" : "Review branch"}
                      aria-pressed={reviewing}
                      onClick={reviewing ? () => setReview(null) : startReview}
                      className={cn(
                        "flex size-6 items-center justify-center rounded-sm text-subtle hover:bg-hover focus-visible:bg-hover hover:text-foreground focus-visible:text-foreground",
                        reviewing && "bg-active text-foreground",
                      )}
                    >
                      <GitCompareArrows className="size-3.5" />
                    </button>
                  </Tip>
                  <CollapseButton side="left" onClick={() => toggle(listPanel, "git")} />
                </div>
              </div>
              <div className="min-h-0 flex-1">
                {reviewing && (
                  <BranchReview
                    base={review}
                    data={branchReview}
                    branches={repo.branches}
                    activeKey={activeKey}
                    onOpen={open}
                    onHover={prefetch}
                    viewed={viewed}
                    setViewed={setViewed}
                    onBase={setReview}
                    onClose={() => setReview(null)}
                  />
                )}
                {listTab === "changes" && review === null && status && (
                  <ChangesPanel status={status} head={repo.commits[0] ?? null} main={main} activeKey={activeKey} onOpen={open} onHover={prefetch} refresh={() => repo.refresh(false)} viewed={viewed} setViewed={setViewed} onRevealInExplorer={revealInExplorer} onShowHistory={(path) => showHistory(path, true)} />
                )}
                {listTab === "pulls" && (
                  <PullsPanel
                    status={status}
                    branches={repo.branches}
                    lastCommit={repo.commits[0] ?? null}
                    activeKey={activeKey}
                    onOpen={open}
                    refreshRepo={() => repo.refresh()}
                  />
                )}
                {listTab === "issues" && <IssuesPanel activeKey={activeKey} onOpen={open} />}
                {listTab === "history" && (
                  <SearchableHistory
                    main={main}
                    search={historySearch}
                    onSearch={setHistorySearch}
                    focusRequested={searchFocus}
                    onFocused={searchFocused}
                    commits={repo.commits}
                    branches={repo.branches}
                    status={status}
                    remotes={remoteNames}
                    webUrl={webUrl}
                    hasMore={repo.hasMore}
                    loadMore={repo.loadMore}
                    refresh={() => repo.refresh()}
                    activeKey={activeKey}
                    onOpen={open}
                    onHover={prefetch}
                    worktrees={repo.worktrees}
                    onOpenRepo={onOpenRepo}
                  />
                )}
              </div>
            </div>
          </ResizablePanel>
          <ResizableHandle className="bg-border" />
          <ResizablePanel id="viewer" minSize={360}>
            <ResizablePanelGroup orientation="vertical" defaultLayout={viewerLayout.defaultLayout} onLayoutChanged={viewerLayout.onLayoutChanged} disabled={terminalMaximized}>
              <ResizablePanel id="editor" minSize={120}>
                {/* Esc from the view itself (a PR, an image) or the code, when Monaco had no use for it. */}
                <div
                  data-panel="code"
                  tabIndex={-1}
                  onKeyDown={(e) => {
                    const target = e.target as HTMLElement;
                    if (e.key === "Escape" && (target === e.currentTarget || target.matches(".monaco-editor textarea.inputarea, [data-code-scroll]"))) focusList();
                  }}
                  className="group/panel relative h-full outline-none"
                >
                  <FocusLine />
                  <Viewer
                    tabs={tabs}
                    active={active}
                    status={status}
                    branchRows={branchReview.review && branchReview.rows}
                    revision={repo.revision}
                    viewed={viewed}
                    toggleViewed={toggleViewed}
                    onActivate={setActiveKey}
                    onClose={close}
                    onCloseTabs={closeTabs}
                    refresh={repo.refresh}
                    onPin={pin}
                    onMoveTab={moveTab}
                    onOpen={(sel) => open(sel, true)}
                    onShowHistory={(path) => showHistory(path, true)}
                    onRevealInExplorer={revealInExplorer}
                    onShowCommit={(sha, path) => showInHistory({ query: sha, scope: null, reveal: { sha, path, id: ++reveals.current } })}
                    webUrl={webUrl}
                  />
                </div>
              </ResizablePanel>
              {/* Rendered only while open, so the viewer keeps its state when the panel toggles. */}
              {terminalOpen && (
                <>
                  <ResizableHandle className="bg-border" />
                  <ResizablePanel id="terminal" defaultSize="35" minSize={100}>
                    {/* Maximized, it covers the workspace, which stays mounted beneath: nothing reloads on the way back. */}
                    <div data-panel="terminal" className={cn("group/panel h-full", terminalMaximized ? "absolute inset-0 z-40" : "relative")}>
                      <FocusLine />
                      <TerminalPanel root={root} worktrees={repo.worktrees} projects={projects} />
                    </div>
                  </ResizablePanel>
                </>
              )}
            </ResizablePanelGroup>
          </ResizablePanel>
          <ResizableHandle className="bg-border" />
          <ResizablePanel
            id="files"
            panelRef={filesPanel}
            defaultSize={260}
            minSize={200}
            maxSize="40"
            collapsible
            collapsedSize={0}
            onResize={(size) => setRightOpen(size.inPixels > 0)}
          >
            <div data-panel="explorer" tabIndex={-1} className="group/panel relative flex h-full flex-col bg-panel outline-none">
              <FocusLine />
              <div className="flex h-9 shrink-0 items-center gap-0.5 border-b border-border pr-1 pl-2">
                <ListTabButton active={explorerView === "files"} onClick={() => setExplorerView("files")}>
                  Explorer
                </ListTabButton>
                <ListTabButton
                  active={explorerView === "search"}
                  onClick={() => {
                    setExplorerView("search");
                    setSearchAsk((a) => ({ id: a.id + 1, seed: "" }));
                  }}
                >
                  Search
                </ListTabButton>
                {/* One group: two ml-autos split the free space, leaving Collapse folders mid-header. */}
                <div className="ml-auto flex items-center gap-0.5">
                  {explorerView === "files" && (
                  <>
                  <Tip label="Filter files" shortcut={findKey}>
                    <button aria-label="Filter files" onClick={() => fileTree.current?.filter()} className="flex size-6 items-center justify-center rounded-sm text-subtle hover:bg-hover focus-visible:bg-hover hover:text-foreground focus-visible:text-foreground">
                      <Search className="size-3.5" />
                    </button>
                  </Tip>
                  <Tip label="Collapse folders">
                    <button aria-label="Collapse folders" onClick={() => fileTree.current?.collapseAll()} className="flex size-6 items-center justify-center rounded-sm text-subtle hover:bg-hover focus-visible:bg-hover hover:text-foreground focus-visible:text-foreground">
                      <ChevronsDownUp className="size-3.5" />
                    </button>
                  </Tip>
                  </>
                  )}
                  <CollapseButton side="right" onClick={() => toggle(filesPanel, "explorer")} />
                </div>
              </div>
              {/* Both stay mounted, so each keeps its state (open folders, results) while the other shows. */}
              <div className={cn("min-h-0 flex-1", explorerView !== "search" && "hidden")}>
                <SearchView active={explorerView === "search"} ask={searchAsk} onOpen={open} />
              </div>
              <div className={cn("min-h-0 flex-1", explorerView !== "files" && "hidden")}>
                <FileTree
                  ref={fileTree}
                  status={status}
                  revision={repo.revision}
                  activeKey={activeKey}
                  onOpen={open}
                  onHover={prefetch}
                  onPathMoved={onPathMoved}
                  onShowHistory={showHistory}
                  webUrl={webUrl}
                />
              </div>
            </div>
          </ResizablePanel>
        </ResizablePanelGroup>
      </div>
      <StatusBar repo={repo} reviewed={uncommitted.filter(viewed).length} active={active?.sel} />
      <TerminalRestoreOffer />
    </div>
  );
}

/** Marks the panel the keys go to: a thin accent along its top while focus is inside. */
function FocusLine() {
  return <span aria-hidden className="pointer-events-none absolute inset-x-0 top-0 z-20 h-px bg-primary opacity-0 group-focus-within/panel:opacity-100" />;
}

function CollapseButton({ side, onClick }: { side: "left" | "right"; onClick: () => void }) {
  const Icon = side === "left" ? PanelLeftClose : PanelRightClose;
  const shortcut = useShortcut(side === "left" ? "view.toggleGitPanel" : "view.toggleExplorer");
  return (
    <Tip label={side === "left" ? "Hide panel" : "Hide explorer"} shortcut={shortcut}>
      <button onClick={onClick} className="ml-auto flex size-6 items-center justify-center rounded-sm text-subtle hover:bg-hover focus-visible:bg-hover hover:text-foreground focus-visible:text-foreground">
        <Icon className="size-3.5" />
      </button>
    </Tip>
  );
}

function ListTabButton({ active, onClick, count, children }: { active: boolean; onClick: () => void; count?: number; children: React.ReactNode }) {
  return (
    <button
      onClick={onClick}
      aria-pressed={active}
      className={cn(
        "flex h-6 items-center gap-1.5 rounded-md px-2 text-[12px] font-medium",
        active ? "bg-active text-foreground" : "text-subtle hover:text-foreground focus-visible:text-foreground",
      )}
    >
      {children}
      {!!count && <CountBadge active={active}>{count}</CountBadge>}
    </button>
  );
}

/** The open file's working copy, else the repository's folder (a PR or an issue has no file). */
function revealInFinder(sel: Selection | undefined) {
  const path = sel && ["file", "unstaged", "staged", "conflict", "branch"].includes(sel.kind) ? selectionPath(sel) : "";
  void revealPath(path);
}
