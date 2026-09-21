import { ArrowDown, ArrowUp, ChevronsDownUp, GitBranch, PanelLeftClose, PanelRightClose, WrapText } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useDefaultLayout, usePanelRef } from "react-resizable-panels";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@/components/ui/resizable";
import { Tip } from "@/components/ui/tooltip";
import type { FileChange, RepoStatus } from "@/lib/api";
import { type Selection, selectionKey } from "@/lib/selection";
import { DEFAULT_FONT_SIZE, LIGHT_SYNTAX_THEMES, SYNTAX_THEMES, updateSettings, useSettings } from "@/lib/settings";
import { arrayMove } from "@dnd-kit/sortable";
import { useRepo } from "@/lib/useRepo";
import { cn } from "@/lib/utils";
import { ChangesPanel, changeList } from "./ChangesPanel";
import { FileTree, type FileTreeHandle } from "./FileTree";
import { HistoryPanel } from "./HistoryPanel";
import { PullsPanel } from "./PullsPanel";
import { changeTotals, TopBar } from "./TopBar";
import { prefetchSelection, resetPairCache, type Tab, Viewer } from "./Viewer";

type ListTab = "changes" | "history" | "pulls";

interface Props {
  root: string;
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

/** Focus is somewhere that owns its keystrokes: text fields, menus, dialogs, pickers. */
export function isTyping(e: KeyboardEvent) {
  const el = e.target instanceof HTMLElement ? e.target : null;
  return !!el && (el.isContentEditable || !!el.closest("input,textarea,select,[role=menu],[role=listbox],[role=dialog]"));
}

export function Workspace({ root, recent, onOpenRepo, onForgetRepo, onReorderRepos }: Props) {
  // Diffs are cached by revision, which restarts per repo.
  useState(resetPairCache);
  const repo = useRepo(root);
  const { status } = repo;
  const s = useSettings();
  const [listTab, setListTab] = useState<ListTab>("changes");
  // Tabs and the active key change together, so they live in one state (no nested updates).
  const [tabState, setTabState] = useState<{ tabs: Tab[]; active: string | null }>({ tabs: [], active: null });
  const { tabs, active: activeKey } = tabState;
  const setActiveKey = useCallback((key: string | null) => setTabState((t) => ({ ...t, active: key })), []);
  // Viewed marks remember the file's content id; a new edit by the agent clears them.
  const [viewedMap, setViewedMap] = useState<Map<string, string>>(() => new Map());
  // Git work on the left, files on the right; both collapse (⌘B / ⌥⌘B) to give code the room.
  const listPanel = usePanelRef();
  const filesPanel = usePanelRef();
  const fileTree = useRef<FileTreeHandle>(null);
  const [leftOpen, setLeftOpen] = useState(true);
  const [rightOpen, setRightOpen] = useState(true);
  const toggle = useCallback((panel: typeof listPanel) => panel.current?.[panel.current.isCollapsed() ? "expand" : "collapse"](), []);
  const layout = useDefaultLayout({ id: "gitviber-main-v4", storage: localStorage });

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
      const f = currentFile(status, sel);
      return !!f && viewedMap.get(`${sel.kind}:${f.path}`) === fileSig(f);
    },
    [status, viewedMap],
  );

  const toggleViewed = useCallback(
    (sel: Selection) => {
      const f = currentFile(status, sel);
      if (!f) return;
      const key = `${sel.kind}:${f.path}`;
      setViewedMap((m) => {
        const next = new Map(m);
        if (next.get(key) === fileSig(f)) next.delete(key);
        else next.set(key, fileSig(f));
        return next;
      });
    },
    [status],
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

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const typing = isTyping(e);
      // ⌘ shortcuts work everywhere; plain/⌥ keys only when not typing, since ⌥+letter
      // types characters (ç, ß) and menus/dialogs own their own keys.
      if (typing && !e.metaKey) return;
      // e.code, not e.key: Option+letter types symbols on macOS.
      const toggles: Record<string, Partial<typeof s>> = {
        KeyZ: { wordWrap: !s.wordWrap },
        KeyS: { sideBySide: !s.sideBySide },
        KeyC: { hideUnchanged: !s.hideUnchanged },
      };
      if (e.altKey && !e.metaKey && toggles[e.code]) {
        e.preventDefault();
        updateSettings(toggles[e.code]);
        return;
      }
      // J/K walk the changed files, the core loop of reviewing an agent's work.
      if (!typing && !e.metaKey && !e.ctrlKey && !e.altKey && (e.key === "j" || e.key === "k") && changes.length) {
        const i = changes.findIndex((c) => selectionKey(c) === activeKey);
        const next = e.key === "j" ? Math.min(changes.length - 1, i + 1) : Math.max(0, i - 1);
        open(changes[i < 0 ? 0 : next]);
        setListTab("changes");
        return;
      }
      if (!typing && e.key === "v" && !e.metaKey && activeKey) {
        const t = tabs.find((x) => x.key === activeKey);
        if (t) toggleViewed(t.sel);
        return;
      }
      // ⌘ only: Ctrl+letters are macOS text-editing keys (Ctrl+B back a char, Ctrl+O open line).
      if (!e.metaKey) return;
      // Physical keys: with ⌥ held, macOS turns "b" into "∫".
      if (e.code === "KeyB" && e.altKey) {
        e.preventDefault();
        toggle(filesPanel);
        return;
      }
      if (e.code === "KeyE" && e.shiftKey) {
        e.preventDefault();
        filesPanel.current?.expand();
        return;
      }
      const k = e.key;
      if (k === "=" || k === "+") updateSettings({ codeFontSize: s.codeFontSize + 0.5 });
      else if (k === "-") updateSettings({ codeFontSize: s.codeFontSize - 0.5 });
      else if (k === "0") updateSettings({ codeFontSize: DEFAULT_FONT_SIZE });
      else if (k === "1") setListTab("changes");
      else if (k === "2") setListTab("history");
      else if (k === "3") setListTab("pulls");
      else if (k === "b") toggle(listPanel);
      else if (k === "o") onOpenRepo();
      // Always swallow ⌘W: with no tab open it would close the window.
      else if (k === "w") activeKey && close(activeKey);
      // Cmd+R would reload the webview; make it a git refresh instead.
      else if (k === "r") repo.refresh();
      else return;
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [s, listPanel, filesPanel, toggle, onOpenRepo, repo, changes, activeKey, open, close, tabs, toggleViewed]);

  const active = tabs.find((t) => t.key === activeKey) ?? null;
  const changeCount = changes.length;

  return (
    <div className="flex h-full flex-col">
      <TopBar
        repo={repo}
        root={root}
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
                <CollapseButton side="left" onClick={() => toggle(listPanel)} />
              </div>
              <div className="min-h-0 flex-1">
                {listTab === "changes" && status && (
                  <ChangesPanel status={status} activeKey={activeKey} onOpen={open} onHover={prefetch} refresh={() => repo.refresh(false)} viewed={viewed} toggleViewed={toggleViewed} />
                )}
                {listTab === "pulls" && (
                  <PullsPanel
                    status={status}
                    branches={repo.branches}
                    lastSubject={repo.commits[0]?.subject ?? null}
                    activeKey={activeKey}
                    onOpen={open}
                    refreshRepo={() => repo.refresh()}
                  />
                )}
                {listTab === "history" && <HistoryPanel commits={repo.commits} remotes={remoteNames} hasMore={repo.hasMore} loadMore={repo.loadMore} activeKey={activeKey} onOpen={open} onHover={prefetch} />}
              </div>
            </div>
          </ResizablePanel>
          <ResizableHandle className="bg-border" />
          <ResizablePanel id="viewer" minSize={360}>
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
                  <button onClick={() => fileTree.current?.collapseAll()} className="ml-auto flex size-6 items-center justify-center rounded-sm text-subtle hover:bg-hover hover:text-foreground">
                    <ChevronsDownUp className="size-3.5" />
                  </button>
                </Tip>
                <CollapseButton side="right" onClick={() => toggle(filesPanel)} />
              </div>
              <div className="min-h-0 flex-1">
                <FileTree ref={fileTree} status={status} revision={repo.revision} activeKey={activeKey} onOpen={open} onHover={prefetch} />
              </div>
            </div>
          </ResizablePanel>
        </ResizablePanelGroup>
      </div>
      <StatusBar repo={repo} reviewed={changes.filter(viewed).length} />
    </div>
  );
}

function CollapseButton({ side, onClick }: { side: "left" | "right"; onClick: () => void }) {
  const Icon = side === "left" ? PanelLeftClose : PanelRightClose;
  return (
    <Tip label={side === "left" ? "Hide panel" : "Hide explorer"} shortcut={side === "left" ? "⌘B" : "⌥⌘B"}>
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
  const { status } = repo;
  const totals = changeTotals(repo);
  return (
    <div className="flex h-6 shrink-0 items-center gap-3 border-t border-border bg-sidebar px-3 text-[11px] text-subtle">
      <span className="flex items-center gap-1 text-muted-foreground">
        <GitBranch className="size-3" />
        <span className="font-mono">{status?.branch ?? status?.head ?? "…"}</span>
      </span>
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
      <Tip label="Word wrap" shortcut="⌥Z">
        <button
          onClick={() => updateSettings({ wordWrap: !s.wordWrap })}
          className={cn("flex items-center gap-1 hover:text-foreground", s.wordWrap && "text-primary hover:text-primary")}
        >
          <WrapText className="size-3" />
          Wrap
        </button>
      </Tip>
    </div>
  );
}
