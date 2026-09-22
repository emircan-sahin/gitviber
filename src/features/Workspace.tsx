import { ArrowDown, ArrowUp, ChevronsDownUp, PanelLeftClose, PanelRightClose, WrapText } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useDefaultLayout, usePanelRef } from "react-resizable-panels";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@/components/ui/resizable";
import { Tip } from "@/components/ui/tooltip";
import { api, errorMessage, type FileChange, type RepoStatus } from "@/lib/api";
import { resetGitHubCache } from "@/lib/githubCache";
import { useShownLanguage } from "@/lib/highlight";
import { useCommands, useShortcut } from "@/lib/keybindings";
import { languageLabel } from "@/lib/language";
import { type Selection, selectionKey, selectionPath } from "@/lib/selection";
import { loadWorkspace, saveWorkspace } from "@/lib/session";
import { DEFAULT_FONT_SIZE, LIGHT_SYNTAX_THEMES, SYNTAX_THEMES, updateSettings, useSettings } from "@/lib/settings";
import { arrayMove } from "@dnd-kit/sortable";
import { useTerminals } from "@/lib/terminals";
import { toast } from "@/lib/toast";
import { useRepo } from "@/lib/useRepo";
import { cn } from "@/lib/utils";
import { openAbout, useAbout } from "./AboutDialog";
import { ChangesPanel, changeList } from "./ChangesPanel";
import { FileTree, type FileTreeHandle } from "./FileTree";
import { ForkHistory } from "./ForkHistory";
import { IssuesPanel } from "./IssuesPanel";
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

export function Workspace({ root, main, recent, onOpenRepo, onForgetRepo, onReorderRepos }: Props) {
  // Diffs are cached by revision, which restarts per repo.
  useState(resetPairCache);
  useState(resetGitHubCache);
  const repo = useRepo(root);
  const { status } = repo;
  const s = useSettings();
  const [saved] = useState(() => loadWorkspace(root));
  const [listTab, setListTab] = useState<ListTab>(() => LIST_TABS.find((t) => t === saved?.listTab) ?? "changes");
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
  const fileTree = useRef<FileTreeHandle>(null);
  const [leftOpen, setLeftOpen] = useState(true);
  const [rightOpen, setRightOpen] = useState(true);
  const toggle = useCallback((panel: typeof listPanel) => panel.current?.[panel.current.isCollapsed() ? "expand" : "collapse"](), []);
  const revealInExplorer = useCallback((path: string) => {
    filesPanel.current?.expand();
    fileTree.current?.reveal(path);
  }, []);
  const layout = useDefaultLayout({ id: "gitviber-main-v4", storage: localStorage });
  useTerminalSetup(root);
  const terminalOpen = useTerminals().open;
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

  const viewed = useCallback(
    (sel: Selection) => {
      // Staging is the act of accepting a file, so staged files always count as reviewed.
      if (sel.kind === "staged") return true;
      const f = currentFile(status, sel);
      return !!f && viewedMap.get(`${sel.kind}:${f.path}`) === fileSig(f);
    },
    [status, viewedMap],
  );

  const toggleViewed = useCallback(
    (sel: Selection) => {
      const f = currentFile(status, sel);
      if (!f) return;
      if (sel.kind === "staged") {
        // Unchecking a staged file takes it back out of the commit; the tab follows it to Changes.
        // Drop the mark it had there before staging, or it would come back already checked.
        setViewedMap((m) => {
          const next = new Map(m);
          next.delete(`unstaged:${f.path}`);
          return next;
        });
        api.unstage([f.path]).catch((e) => toast("error", "Unstage failed", errorMessage(e))).finally(() => repo.refresh(false));
        return;
      }
      const key = `${sel.kind}:${f.path}`;
      setViewedMap((m) => {
        const next = new Map(m);
        if (next.get(key) === fileSig(f)) next.delete(key);
        else next.set(key, fileSig(f));
        return next;
      });
    },
    [status, repo.refresh],
  );

  const changes = useMemo(() => (status ? changeList(status) : []), [status]);
  const remoteNames = useMemo(() => new Set(repo.branches.filter((b) => b.remote).map((b) => b.name)), [repo.branches]);

  // A merge/rebase that stopped on conflicts: bring the conflicts into view.
  const conflictCount = status?.conflicted.length ?? 0;
  useEffect(() => {
    if (conflictCount > 0) setListTab("changes");
  }, [conflictCount]);

  const prefetch = useCallback((sel: Selection) => prefetchSelection(sel, repo.revision, s.codeTheme), [repo.revision, s.codeTheme]);

  // Reviewing is sequential: have the neighbours of the open file ready before J/K.
  useEffect(() => {
    const i = changes.findIndex((c) => selectionKey(c) === activeKey);
    if (i < 0) return;
    for (const n of [changes[i + 1], changes[i - 1]]) if (n) prefetchSelection(n, repo.revision, s.codeTheme);
  }, [changes, activeKey, repo.revision, s.codeTheme]);

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
    "editor.toggleWrap": () => updateSettings({ wordWrap: !s.wordWrap }),
    "editor.fontZoomIn": () => updateSettings({ codeFontSize: s.codeFontSize + 0.5 }),
    "editor.fontZoomOut": () => updateSettings({ codeFontSize: s.codeFontSize - 0.5 }),
    "editor.fontZoomReset": () => updateSettings({ codeFontSize: DEFAULT_FONT_SIZE }),
    "view.changes": () => setListTab("changes"),
    "view.history": () => setListTab("history"),
    "view.pulls": () => setListTab("pulls"),
    "view.issues": () => setListTab("issues"),
    "view.toggleGitPanel": () => toggle(listPanel),
    "view.toggleExplorer": () => toggle(filesPanel),
    "view.showExplorer": () => filesPanel.current?.expand(),
    // Registered even with no tab open: an unhandled ⌘W would close the window.
    "tab.close": () => activeKey && close(activeKey),
    // ⌘R would reload the webview; make it a git refresh instead.
    "repo.refresh": () => repo.refresh(),
  });

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
        leftOpen={leftOpen}
        rightOpen={rightOpen}
        onToggleLeft={() => toggle(listPanel)}
        onToggleRight={() => toggle(filesPanel)}
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
            <div className="flex h-full flex-col bg-panel">
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
                <CollapseButton side="left" onClick={() => toggle(listPanel)} />
              </div>
              <div className="min-h-0 flex-1">
                {listTab === "changes" && status && (
                  <ChangesPanel status={status} activeKey={activeKey} onOpen={open} onHover={prefetch} refresh={() => repo.refresh(false)} viewed={viewed} toggleViewed={toggleViewed} onRevealInExplorer={revealInExplorer} />
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
                  <ForkHistory
                    commits={repo.commits}
                    status={status}
                    remotes={remoteNames}
                    hasMore={repo.hasMore}
                    loadMore={repo.loadMore}
                    refresh={() => repo.refresh()}
                    activeKey={activeKey}
                    onOpen={open}
                    onHover={prefetch}
                  />
                )}
              </div>
            </div>
          </ResizablePanel>
          <ResizableHandle className="bg-border" />
          <ResizablePanel id="viewer" minSize={360}>
            <ResizablePanelGroup orientation="vertical" defaultLayout={viewerLayout.defaultLayout} onLayoutChanged={viewerLayout.onLayoutChanged}>
              <ResizablePanel id="editor" minSize={120}>
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
                />
              </ResizablePanel>
              {/* Rendered only while open, so the viewer keeps its state when the panel toggles. */}
              {terminalOpen && (
                <>
                  <ResizableHandle className="h-px w-full bg-border after:inset-x-0 after:inset-y-auto after:top-1/2 after:left-0 after:h-2 after:w-full after:translate-x-0 after:-translate-y-1/2" />
                  <ResizablePanel id="terminal" defaultSize="35" minSize={100}>
                    <TerminalPanel root={root} worktrees={repo.worktrees} />
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
            <div className="flex h-full flex-col bg-panel">
              <div className="flex h-9 shrink-0 items-center border-b border-border pr-1 pl-3">
                <span className="text-[10.5px] font-semibold tracking-[0.08em] text-subtle uppercase">Explorer</span>
                <Tip label="Collapse folders">
                  <button aria-label="Collapse folders" onClick={() => fileTree.current?.collapseAll()} className="ml-auto flex size-6 items-center justify-center rounded-sm text-subtle hover:bg-hover hover:text-foreground">
                    <ChevronsDownUp className="size-3.5" />
                  </button>
                </Tip>
                <CollapseButton side="right" onClick={() => toggle(filesPanel)} />
              </div>
              <div className="min-h-0 flex-1">
                <FileTree ref={fileTree} status={status} revision={repo.revision} activeKey={activeKey} onOpen={open} onHover={prefetch} onPathMoved={onPathMoved} />
              </div>
            </div>
          </ResizablePanel>
        </ResizablePanelGroup>
      </div>
      <StatusBar repo={repo} reviewed={changes.filter(viewed).length} />
      <TerminalRestoreOffer />
    </div>
  );
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

function StatusBar({ repo, reviewed }: { repo: ReturnType<typeof useRepo>; reviewed: number }) {
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
          {status.conflicted.length > 0 && ` · ${status.conflicted.length} conflicts`}
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
        {s.codeFont} {s.codeFontSize}
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
