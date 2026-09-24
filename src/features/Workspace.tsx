import { ArrowDown, ArrowUp, ChevronsDownUp, PanelLeftClose, PanelRightClose, WrapText } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useDefaultLayout, usePanelRef } from "react-resizable-panels";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@/components/ui/resizable";
import { Tip } from "@/components/ui/tooltip";
import { api, errorMessage, type FileChange, type RepoStatus } from "@/lib/api";
import { newerCopy, resetGitHubCache, useGitHubCacheVersion } from "@/lib/githubCache";
import { useShownLanguage, warmHighlighter } from "@/lib/highlight";
import { prepare } from "@/lib/monaco";
import { useCommands, useShortcut } from "@/lib/keybindings";
import { languageLabel } from "@/lib/language";
import { type Selection, selectionKey, selectionPath } from "@/lib/selection";
import { codeWantsFocus, focusedPanel, focusList, focusPanel, type Panel, PANELS } from "@/lib/panels";
import type { OpenTarget } from "@/lib/openIn";
import { loadWorkspace, saveWorkspace } from "@/lib/session";
import { codeFontName, DEFAULT_FONT_SIZE, LIGHT_SYNTAX_THEMES, SYNTAX_THEMES, updateSettings, useSettings } from "@/lib/settings";
import { arrayMove } from "@dnd-kit/sortable";
import { useTerminals } from "@/lib/terminals";
import { toast } from "@/lib/toast";
import { useRepo } from "@/lib/useRepo";
import { cn } from "@/lib/utils";
import { openAbout, useAbout } from "./AboutDialog";
import { ChangesPanel, changeList } from "./ChangesPanel";
import { showQuickOpen, useQuickOpenSource } from "./CommandPalette";
import { FileTree, type FileTreeHandle } from "./FileTree";
import { type HistorySearch, NO_SEARCH, SearchableHistory } from "./HistorySearch";
import { IssuesPanel } from "./IssuesPanel";
import { lineInView } from "./MonacoView";
import { OpenInButton } from "./OpenIn";
import { PullsPanel } from "./PullsPanel";
import { TerminalPanel, TerminalRestoreOffer, useTerminalSetup } from "./TerminalPanel";
import { changeTotals, TopBar } from "./TopBar";
import { prefetchSelection, resetPairCache, type Tab, Viewer } from "./Viewer";

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

const fileSig = (f: FileChange) => `${f.status}:${f.oid ?? `${f.additions}:${f.deletions}`}`;

type ChangeKind = "conflict" | "staged" | "unstaged";
const LISTS: Record<ChangeKind, (s: RepoStatus) => FileChange[]> = {
  conflict: (s) => s.conflicted,
  staged: (s) => s.staged,
  unstaged: (s) => s.unstaged,
};
const isChange = (sel: Selection): sel is Selection & { kind: ChangeKind } => sel.kind in LISTS;

function currentFile(status: RepoStatus | null, sel: Selection): FileChange | undefined {
  if (!status || !isChange(sel)) return undefined;
  return LISTS[sel.kind](status).find((f) => f.path === sel.file.path);
}

/** Where a change tab's file lives now: same list first, else wherever it moved (resolved → staged…). */
function relocate(status: RepoStatus, sel: Selection & { kind: ChangeKind }): Selection | null {
  for (const kind of [sel.kind, "conflict", "staged", "unstaged"] as const) {
    const file = LISTS[kind](status).find((f) => f.path === sel.file.path);
    if (file) return { kind, file };
  }
  return null;
}

export function Workspace({ root, main, recent, onOpenRepo, onForgetRepo, onReorderRepos, onLocateRepo }: Props) {
  // Diffs are cached by revision, which restarts per repo.
  useState(resetPairCache);
  useState(resetGitHubCache);
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
  // Tabs and the active key change together, so they live in one state (no nested updates).
  const [tabState, setTabState] = useState<{ tabs: Tab[]; active: string | null }>(() => ({ tabs: saved?.tabs ?? [], active: saved?.active ?? null }));
  const { tabs, active: activeKey } = tabState;
  const setActiveKey = useCallback((key: string | null) => setTabState((t) => ({ ...t, active: key })), []);
  // Viewed marks remember the file's content id; a new edit by the agent clears them.
  const [viewedMap, setViewedMap] = useState<Map<string, string>>(() => new Map(saved?.viewed));
  useEffect(() => saveWorkspace(root, { tabs, active: activeKey, listTab, viewed: [...viewedMap] }), [root, tabs, activeKey, listTab, viewedMap]);
  // Git work on the left, files on the right; both collapse to give code the room.
  const listPanel = usePanelRef();
  const filesPanel = usePanelRef();
  useTerminalSetup(root);
  const terminalOpen = useTerminals().open;
  const fileTree = useRef<FileTreeHandle>(null);
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
    fileTree.current?.reveal(path);
  }, []);
  const showInHistory = useCallback((search: HistorySearch) => {
    setHistorySearch(search);
    setListTab("history");
    listPanel.current?.expand();
  }, []);
  const showHistory = useCallback((path: string, file: boolean) => showInHistory({ ...NO_SEARCH, scope: { path, file } }), [showInHistory]);
  const layout = useDefaultLayout({ id: "gitviber-main-v4", storage: localStorage });
  const viewerLayout = useDefaultLayout({ id: "gitviber-viewer-v1", storage: localStorage, panelIds: terminalOpen ? ["editor", "terminal"] : ["editor"] });

  const open = useCallback((sel: Selection, pin = false) => {
    const key = selectionKey(sel);
    setTabState(({ tabs: prev }) => {
      const existing = prev.find((t) => t.key === key);
      if (existing) return { tabs: prev.map((t) => (t.key === key ? { ...t, sel, preview: t.preview && !pin } : t)), active: key };
      // Single click reuses the preview tab (VS Code style); double click keeps it.
      const previewAt = prev.findIndex((t) => t.preview);
      const tab = { key, sel, preview: !pin };
      if (previewAt >= 0 && !pin) return { tabs: prev.map((t, i) => (i === previewAt ? tab : t)), active: key };
      return { tabs: [...prev, tab], active: key };
    });
  }, []);

  const close = useCallback((key: string) => {
    setTabState(({ tabs: prev, active }) => {
      const i = prev.findIndex((t) => t.key === key);
      const next = prev.filter((t) => t.key !== key);
      return { tabs: next, active: active === key ? (next[Math.min(i, next.length - 1)]?.key ?? null) : active };
    });
  }, []);

  const moveTab = useCallback((from: number, to: number) => setTabState((st) => ({ ...st, tabs: arrayMove(st.tabs, from, to) })), []);

  // ⌘1–⌘9 and next/previous (wrapping), as in browsers.
  const goTab = (i: number) => (tabs[i] ? () => setActiveKey(tabs[i].key) : undefined);
  const stepTab = (dir: 1 | -1) =>
    tabs.length > 1 ? () => setActiveKey(tabs[(tabs.findIndex((t) => t.key === activeKey) + dir + tabs.length) % tabs.length].key) : undefined;

  const pin = useCallback((key: string) => setTabState((st) => ({ ...st, tabs: st.tabs.map((t) => (t.key === key ? { ...t, preview: false } : t)) })), []);

  // Explorer rename/trash: file tabs at or under `from` move to `to` in place, or close when it's null.
  const onPathMoved = useCallback((from: string, to: string | null) => {
    setTabState(({ tabs: prev, active }) => {
      const hit = (t: Tab) => t.sel.kind === "file" && (t.sel.path === from || t.sel.path.startsWith(`${from}/`));
      const i = prev.findIndex((t) => t.key === active);
      if (to === null) {
        // Like closing a tab: the next surviving one to the right, else to the left.
        const near = prev.slice(i + 1).find((t) => !hit(t)) ?? prev.slice(0, Math.max(i, 0)).reverse().find((t) => !hit(t));
        return { tabs: prev.filter((t) => !hit(t)), active: i >= 0 && hit(prev[i]) ? (near?.key ?? null) : active };
      }
      // A tab already open at the new path absorbs the moved one, as in the git sync below.
      const tabs: Tab[] = [];
      let nextActive = active;
      for (const t of prev) {
        const sel: Selection = hit(t) ? { kind: "file", path: to + selectionPath(t.sel).slice(from.length) } : t.sel;
        const key = selectionKey(sel);
        if (t.key === active) nextActive = key;
        const twin = tabs.findIndex((x) => x.key === key);
        if (twin >= 0) tabs[twin] = { ...tabs[twin], preview: tabs[twin].preview && t.preview };
        else tabs.push(key === t.key ? t : { ...t, key, sel });
      }
      return { tabs, active: nextActive };
    });
  }, []);

  // Keep change tabs in sync with git: a staged or resolved file moves lists, a
  // committed/discarded one disappears. Two tabs that land on the same file merge.
  useEffect(() => {
    if (!status) return;
    setTabState(({ tabs: prev, active }) => {
      const moved = new Map<string, string | null>();
      const next: Tab[] = [];
      for (const t of prev) {
        const sel = isChange(t.sel) ? relocate(status, t.sel) : t.sel;
        if (!sel) {
          moved.set(t.key, null);
          continue;
        }
        const key = selectionKey(sel);
        if (key !== t.key) moved.set(t.key, key);
        const twin = next.findIndex((x) => x.key === key);
        if (twin >= 0) next[twin] = { ...next[twin], preview: next[twin].preview && t.preview };
        else next.push({ ...t, key, sel });
      }
      if (!moved.size && next.length === prev.length && next.every((t, i) => t.sel === prev[i].sel)) return { tabs: prev, active };
      const nextActive = active && moved.has(active) ? (moved.get(active) ?? next[0]?.key ?? null) : active;
      return { tabs: next, active: nextActive };
    });
  }, [status]);

  // Issue and PR tabs hold the item as it was when opened, saved across restarts too. When a
  // list or detail read brings a newer copy (closed, renamed), the tab's title and icon follow.
  const gitHubVersion = useGitHubCacheVersion();
  useEffect(() => {
    setTabState((st) => {
      let changed = false;
      const tabs = st.tabs.map((t): Tab => {
        let sel: Selection | null = null;
        if (t.sel.kind === "issue") {
          const issue = newerCopy(t.sel.issue);
          if (issue) sel = { kind: "issue", issue };
        } else if (t.sel.kind === "pull") {
          const pull = newerCopy(t.sel.pull);
          if (pull) sel = { kind: "pull", pull };
        }
        if (!sel) return t;
        changed = true;
        return { ...t, sel };
      });
      return changed ? { ...st, tabs } : st;
    });
  }, [gitHubVersion]);

  const viewed = useCallback(
    (sel: Selection) => {
      // Staging is the act of accepting a file, so staged files always count as reviewed.
      if (sel.kind === "staged") return true;
      const f = currentFile(status, sel);
      return !!f && viewedMap.get(`${sel.kind}:${f.path}`) === fileSig(f);
    },
    [status, viewedMap],
  );

  const setViewed = useCallback(
    (sels: Selection[], on: boolean) => {
      const files = sels.flatMap((sel) => {
        const f = currentFile(status, sel);
        return f ? [{ kind: sel.kind, f }] : [];
      });
      // Unchecking a staged file takes it back out of the commit; the tab follows it to Changes.
      const unstage = on ? [] : files.filter((x) => x.kind === "staged").map((x) => x.f.path);
      setViewedMap((m) => {
        const next = new Map(m);
        for (const { kind, f } of files) {
          // Drop the mark it had in Changes before staging, or it would come back already checked.
          if (kind === "staged") next.delete(`unstaged:${f.path}`);
          else if (on) next.set(`${kind}:${f.path}`, fileSig(f));
          else next.delete(`${kind}:${f.path}`);
        }
        return next;
      });
      // One call for all of them: parallel git calls would fight over index.lock.
      if (unstage.length) api.unstage(unstage).catch((e) => toast("error", "Unstage failed", errorMessage(e))).finally(() => repo.refresh(false));
    },
    [status, repo.refresh],
  );

  const toggleViewed = useCallback((sel: Selection) => setViewed([sel], !viewed(sel)), [setViewed, viewed]);

  const changes = useMemo(() => (status ? changeList(status) : []), [status]);
  const remoteNames = useMemo(() => new Set(repo.branches.filter((b) => b.remote).map((b) => b.name)), [repo.branches]);

  // A merge/rebase that stopped on conflicts: bring the conflicts into view.
  const conflictCount = status?.conflicted.length ?? 0;
  useEffect(() => {
    if (conflictCount > 0) setListTab("changes");
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
    "view.showExplorer": () => show(filesPanel, "explorer"),
    "view.focusCode": () => focusPanel("code"),
    "view.focusNextPanel": () => cycle(1),
    "view.focusPrevPanel": () => cycle(-1),
    "tab.close": activeKey ? () => close(activeKey) : undefined,
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
    "workbench.openChange": changes.length ? () => showQuickOpen("changes") : undefined,
  });

  // Quick open's picks take the code view along, where a new tab then takes focus (MonacoView).
  useQuickOpenSource({
    root,
    changes,
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

  const active = tabs.find((t) => t.key === activeKey) ?? null;
  const changeCount = changes.length;

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
                <CollapseButton side="left" onClick={() => toggle(listPanel, "git")} />
              </div>
              <div className="min-h-0 flex-1">
                {listTab === "changes" && status && (
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
                    if (e.key === "Escape" && (target === e.currentTarget || target.matches(".monaco-editor textarea.inputarea"))) focusList();
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
              <div className="flex h-9 shrink-0 items-center border-b border-border pr-1 pl-3">
                <span className="text-[10.5px] font-semibold tracking-[0.08em] text-subtle uppercase">Explorer</span>
                {/* One group: two ml-autos split the free space, leaving Collapse folders mid-header. */}
                <div className="ml-auto flex items-center gap-0.5">
                  <Tip label="Collapse folders">
                    <button aria-label="Collapse folders" onClick={() => fileTree.current?.collapseAll()} className="flex size-6 items-center justify-center rounded-sm text-subtle hover:bg-hover hover:text-foreground">
                      <ChevronsDownUp className="size-3.5" />
                    </button>
                  </Tip>
                  <CollapseButton side="right" onClick={() => toggle(filesPanel, "explorer")} />
                </div>
              </div>
              <div className="min-h-0 flex-1">
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
      <StatusBar repo={repo} reviewed={changes.filter(viewed).length} openTarget={() => openTarget(active?.sel)} />
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
      <button onClick={onClick} className="ml-auto flex size-6 items-center justify-center rounded-sm text-subtle hover:bg-hover hover:text-foreground">
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
        active ? "bg-active text-foreground" : "text-subtle hover:text-foreground",
      )}
    >
      {children}
      {!!count && <span className={cn("rounded-sm px-1 font-mono text-[10px] leading-4", active ? "bg-modified-fill text-on-status" : "bg-elevated text-muted-foreground")}>{count}</span>}
    </button>
  );
}

function StatusBar({ repo, reviewed, openTarget }: { repo: ReturnType<typeof useRepo>; reviewed: number; openTarget: () => OpenTarget }) {
  const s = useSettings();
  const language = useShownLanguage();
  const wrapKey = useShortcut("editor.toggleWrap");
  const { status } = repo;
  const totals = changeTotals(repo);
  return (
    // The branch is in the top bar's breadcrumb already, so it isn't repeated here.
    <div className="flex h-6 shrink-0 items-center gap-3 border-t border-border bg-sidebar px-3 text-[11px] text-subtle">
      {status?.upstream && (
        <span className="flex items-center gap-1.5 font-mono">
          <span className={cn("flex items-center", status.ahead && "text-primary")}>
            <ArrowUp className="size-3" />
            {status.ahead}
          </span>
          <span className={cn("flex items-center", status.behind && "text-modified")}>
            <ArrowDown className="size-3" />
            {status.behind}
          </span>
        </span>
      )}
      {status?.operation && (
        <span className="font-semibold text-conflict uppercase">
          {status.operation.kind}
          {status.operation.step != null && ` ${status.operation.step}/${status.operation.total}`}
          {status.conflicted.length > 0 && ` · ${status.conflicted.length} ${status.conflicted.length === 1 ? "conflict" : "conflicts"}`}
        </span>
      )}
      {totals.files > 0 && (
        <span>
          {totals.files} changed · <span className="font-mono text-added">+{totals.add}</span> <span className="font-mono text-removed">-{totals.del}</span> · {reviewed}/
          {totals.files} reviewed
        </span>
      )}
      <span className="ml-auto">{s.dark ? SYNTAX_THEMES[s.syntaxTheme] : LIGHT_SYNTAX_THEMES[s.lightSyntaxTheme]}</span>
      <span>
        {codeFontName(s)} {s.codeFontSize}
      </span>
      <span>{s.sideBySide ? "Split" : "Unified"}</span>
      <Tip label="Word wrap" shortcut={wrapKey}>
        <button
          onClick={() => updateSettings({ wordWrap: !s.wordWrap })}
          className={cn("flex items-center gap-1 hover:text-foreground", s.wordWrap && "text-primary hover:text-primary")}
        >
          <WrapText className="size-3" />
          Wrap
        </button>
      </Tip>
      {language && <span>{languageLabel(language)}</span>}
      <VersionInfo />
      <OpenInButton target={openTarget} />
    </div>
  );
}

/** `v0.1.0 · macOS 15.5`; opens About, which can copy it for a bug report. */
function VersionInfo() {
  const about = useAbout();
  if (!about) return null;
  return (
    <Tip label="About GitViber">
      <button onClick={openAbout} className="hover:text-foreground">
        v{about.version} · {about.os}
      </button>
    </Tip>
  );
}

/** The open file's working copy, else the repository's folder (a PR or an issue has no file). */
function revealInFinder(sel: Selection | undefined) {
  const path = sel && ["file", "unstaged", "staged", "conflict"].includes(sel.kind) ? selectionPath(sel) : "";
  api.revealPath(path).catch((e) => toast("error", "Could not reveal in Finder", errorMessage(e)));
}

/** The open file while it's on disk, at the line in view; else the whole worktree. */
function openTarget(sel: Selection | undefined): OpenTarget {
  const change = sel?.kind === "unstaged" || sel?.kind === "staged" || sel?.kind === "conflict";
  const path = sel?.kind === "file" ? sel.path : change && sel.file.status !== "D" ? sel.file.path : null;
  return path === null ? { path: "" } : { path, line: lineInView(path) };
}
