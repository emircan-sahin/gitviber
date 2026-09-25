import { arrayMove } from "@dnd-kit/sortable";
import { useCallback, useEffect, useRef, useState } from "react";
import type { RepoStatus } from "@/lib/api";
import { newerCopy, useGitHubCacheVersion } from "@/lib/github/githubCache";
import { type Selection, selectionKey, selectionPath } from "@/lib/repo/selection";
import type { loadWorkspace } from "@/lib/repo/session";
import { isChange, relocate } from "@/features/changes/changeList";
import { type Tab, type TabGroup, tabGroup } from "@/features/viewer/tabs";

/** The code view's tabs: preview and pinned, closed ones to reopen, and following their files through git and renames. */
export function useTabs(saved: ReturnType<typeof loadWorkspace>, status: RepoStatus | null) {
  // Tabs and the active key change together, so they live in one state (no nested updates).
  const [tabState, setTabState] = useState<{ tabs: Tab[]; active: string | null }>(() => ({ tabs: saved?.tabs ?? [], active: saved?.active ?? null }));
  const { tabs, active: activeKey } = tabState;
  const setActiveKey = useCallback((key: string | null) => setTabState((t) => ({ ...t, active: key })), []);

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

  // Closed tabs, the latest last, where they were: ⇧⌘T brings them back as browsers do.
  const [closed, setClosed] = useState<{ sel: Selection; index: number }[]>([]);
  const tabsNow = useRef(tabs);
  tabsNow.current = tabs;
  const closeTabs = useCallback((keys: string[]) => {
    const gone = new Set(keys);
    const shut = tabsNow.current.flatMap((t, index) => (gone.has(t.key) ? [{ sel: t.sel, index }] : []));
    if (!shut.length) return;
    // The leftmost comes back first, so each returns to its own place.
    setClosed((c) => [...c.filter((t) => !gone.has(selectionKey(t.sel))), ...shut.reverse()].slice(-20));
    setTabState(({ tabs: prev, active }) => {
      const next = prev.filter((t) => !gone.has(t.key));
      if (!active || !gone.has(active)) return { tabs: next, active };
      // The next open tab to the right, else to the left.
      const i = prev.findIndex((t) => t.key === active);
      const near = prev.slice(i + 1).find((t) => !gone.has(t.key)) ?? prev.slice(0, i).reverse().find((t) => !gone.has(t.key));
      return { tabs: next, active: near?.key ?? null };
    });
  }, []);
  const close = useCallback((key: string) => closeTabs([key]), [closeTabs]);
  // Close Others and the like, around the active tab.
  const closeAround = (which: TabGroup) => {
    const i = tabs.findIndex((t) => t.key === activeKey);
    const keys = i < 0 ? [] : tabGroup(tabs, i, which);
    return keys.length ? () => closeTabs(keys) : undefined;
  };

  const reopen = () => {
    const last = closed.at(-1);
    if (!last) return;
    setClosed((c) => c.slice(0, -1));
    const key = selectionKey(last.sel);
    setTabState(({ tabs: prev }) => ({
      tabs: prev.some((t) => t.key === key) ? prev : [...prev.slice(0, last.index), { key, sel: last.sel, preview: false }, ...prev.slice(last.index)],
      active: key,
    }));
  };

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

  return { tabs, activeKey, setActiveKey, open, closeTabs, close, closeAround, reopen, canReopen: closed.length > 0, moveTab, goTab, stepTab, pin, onPathMoved };
}
