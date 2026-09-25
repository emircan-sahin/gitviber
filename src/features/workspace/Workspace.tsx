import { ChevronsDownUp, GitCompareArrows, PanelLeftClose, PanelRightClose, Search } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useDefaultLayout, usePanelRef } from "react-resizable-panels";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@/components/ui/resizable";
import { Tip } from "@/components/ui/tooltip";
import { find } from "@/lib/ui/find";
import { resetGitHubCache } from "@/lib/github/githubCache";
import { warmHighlighter } from "@/lib/editor/highlight";
import { setLinkHost } from "@/lib/links/linkHost";
import { prepare } from "@/lib/editor/monaco";
import { useCommands, useShortcut } from "@/lib/commands/keybindings";
import { dropReveal, revealWaits } from "@/lib/editor/reveal";
import { type Selection, selectionKey, selectionPath } from "@/lib/repo/selection";
import { codeWantsFocus, focusedPanel, focusList, focusPanel, type Panel, PANELS } from "@/lib/ui/panels";
import { loadWorkspace, saveWorkspace } from "@/lib/repo/session";
import { DEFAULT_FONT_SIZE, updateSettings, useSettings } from "@/lib/settings";
import { useTerminals } from "@/lib/terminal/terminals";
import { useRepo } from "@/lib/repo/useRepo";
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
import { TerminalPanel, TerminalRestoreOffer, useTerminalSetup } from "@/features/terminal/TerminalPanel";
import { TopBar } from "@/features/topbar/TopBar";
import { Viewer } from "@/features/viewer/Viewer";
import { prefetchSelection, resetPairCache } from "@/features/viewer/diffPairs";

const LIST_TABS = ["changes", "history", "pulls", "issues"] as const;
type ListTab = (typeof LIST_TABS)[number];

interface Props {
  root: string;
  /** The main worktree; differs from root when a linked worktree is open. */
  main: string;
  recent: string[];
  onOpenRepo: (path?: string) => void;
  onForgetRepo: (path: string) => void;
  onReorderRepos: (list: string[]) => void;
  onLocateRepo: (path: string) => void;
}

/**
 * Diffs are cached by revision, which restarts per repo, and GitHub data is per repo: both start
 * over with each repo, during its first render, before anything in it reads them.
 */
function useFreshCaches(root: string) {
  const cleared = useRef<string | null>(null);
  if (cleared.current === root) return;
  cleared.current = root;
  resetPairCache();
  resetGitHubCache();
}

export function Workspace({ root, main, recent, onOpenRepo, onForgetRepo, onReorderRepos, onLocateRepo }: Props) {
  useFreshCaches(root);
  const repo = useRepo(root);
  const { status } = repo;
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
  const branchReview = useBranchReview(review, repo.revision, reviewing);
  const { tabs, activeKey, setActiveKey, open, closeTabs, close, closeAround, reopen, canReopen, moveTab, goTab, stepTab, pin, onPathMoved } = useTabs(saved, status, branchReview.review && branchReview.rows);
  const { viewedMap, viewed, setViewed, toggleViewed } = useViewed(saved, status, repo.refresh, branchReview.review);
  useEffect(() => saveWorkspace(root, { tabs, active: activeKey, listTab, viewed: [...viewedMap], review }), [root, tabs, activeKey, listTab, viewedMap, review]);
  // Git work on the left, files on the right; both collapse to give code the room.
  const listPanel = usePanelRef();
  const filesPanel = usePanelRef();
  useTerminalSetup(root);
  const terminalOpen = useTerminals().open;
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

  // ⌘-click in the code view and the terminal opens files here (lib/links/linkHost).
  useEffect(() => {
    setLinkHost({
      root,
      revision: repo.revision,
      open: (path, focus) => {
        open({ kind: "file", path }, true);
        if (focus) focusPanel("code");
      },
    });
  }, [root, repo.revision, open]);
  useEffect(() => () => setLinkHost(null), []);

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

  // J/K walk the changed files, the core loop of reviewing an agent's work.
  const step = (dir: 1 | -1) => {
    if (!changes.length) return;
    const i = changes.findIndex((c) => selectionKey(c) === activeKey);
    open(changes[i < 0 ? 0 : Math.min(changes.length - 1, Math.max(0, i + dir))]);
    setListTab("changes");
  };

  useCommands({
    "review.nextFile": () => step(1),
    "review.prevFile": () => step(-1),
    "review.branch": startReview,
    "review.toggleViewed": () => {
      const t = tabs.find((x) => x.key === activeKey);
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
    "view.focusCode": () => focusPanel("code"),
    "view.focusNextPanel": () => cycle(1),
    "view.focusPrevPanel": () => cycle(-1),
    "tab.close": activeKey ? () => close(activeKey) : undefined,
    "tab.reopenClosed": canReopen ? reopen : undefined,
    "tab.closeOthers": closeAround("others"),
    "tab.closeLeft": closeAround("left"),
    "tab.closeRight": closeAround("right"),
    "tab.closeAll": closeAround("all"),
    "tab.goto1": goTab(0),
    "tab.goto2": goTab(1),
    "tab.goto3": goTab(2),
    "tab.goto4": goTab(3),
    "tab.goto5": goTab(4),
    "tab.goto6": goTab(5),
    "tab.goto7": goTab(6),
    "tab.goto8": goTab(7),
    "tab.last": goTab(tabs.length - 1),
    "tab.next": stepTab(1),
    "tab.prev": stepTab(-1),
    "file.reveal": () => revealInFinder(tabs.find((t) => t.key === activeKey)?.sel),
    "repo.refresh": () => repo.refresh(),
    "workbench.quickOpen": () => showQuickOpen(),
    "workbench.openChange": uncommitted.length ? () => showQuickOpen("changes") : undefined,
  });

  // Quick open's picks take the code view along, where a new tab then takes focus (MonacoView).
  useQuickOpenSource({
    root,
    changes: uncommitted,
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

  const active = tabs.find((t) => t.key === activeKey) ?? null;
  const changeCount = uncommitted.length;

  return (
    <div className="flex h-full flex-col">
      <TopBar
        repo={repo}
        root={root}
        main={main}
        recent={recent}
        onOpenRepo={onOpenRepo}
        onForgetRepo={onForgetRepo}
        onReorderRepos={onReorderRepos}
        onLocateRepo={onLocateRepo}
        leftOpen={leftOpen}
        rightOpen={rightOpen}
        onToggleLeft={() => toggle(listPanel, "git")}
        onToggleRight={() => toggle(filesPanel, "explorer")}
      />
      <div className="min-h-0 flex-1">
        <ResizablePanelGroup orientation="horizontal" defaultLayout={layout.defaultLayout} onLayoutChanged={layout.onLayoutChanged}>
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
            <ResizablePanelGroup orientation="vertical" defaultLayout={viewerLayout.defaultLayout} onLayoutChanged={viewerLayout.onLayoutChanged}>
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
                    onShowCommit={(sha, path) => showInHistory({ query: sha, scope: null, reveal: { sha, path, id: ++reveals.current } })}
                  />
                </div>
              </ResizablePanel>
              {/* Rendered only while open, so the viewer keeps its state when the panel toggles. */}
              {terminalOpen && (
                <>
                  <ResizableHandle className="h-px w-full bg-border after:inset-x-0 after:inset-y-auto after:top-1/2 after:left-0 after:h-2 after:w-full after:translate-x-0 after:-translate-y-1/2" />
                  <ResizablePanel id="terminal" defaultSize="35" minSize={100}>
                    <div data-panel="terminal" className="group/panel relative h-full">
                      <FocusLine />
                      <TerminalPanel root={root} worktrees={repo.worktrees} />
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
                <ListTabButton active={explorerView === "search"} onClick={() => setExplorerView("search")}>
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
      className={cn(
        "flex h-6 items-center gap-1.5 rounded-md px-2 text-[12px] font-medium",
        active ? "bg-active text-foreground" : "text-subtle hover:text-foreground focus-visible:text-foreground",
      )}
    >
      {children}
      {!!count && <span className={cn("rounded-sm px-1 font-mono text-[10px] leading-4", active ? "bg-modified-fill text-on-status" : "bg-elevated text-muted-foreground")}>{count}</span>}
    </button>
  );
}

/** The open file's working copy, else the repository's folder (a PR or an issue has no file). */
function revealInFinder(sel: Selection | undefined) {
  const path = sel && ["file", "unstaged", "staged", "conflict", "branch"].includes(sel.kind) ? selectionPath(sel) : "";
  void revealPath(path);
}
