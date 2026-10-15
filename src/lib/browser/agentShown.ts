import type { Selection } from "../repo/selection.ts";

// Which agent tabs the tab strip shows (Settings → Browser → Show agents' tabs). A tab left out
// stays in the workspace's list and keeps running.

type Tab = { key: string; sel: Selection };

/** The ids of the agent tabs left out: all of them while hidden, but those that are the user's now (taken, or their pane closed). */
export function hiddenAgentIds(owners: ReadonlyMap<string, number>, theirs: ReadonlySet<string>, show: boolean): ReadonlySet<string> {
  return show ? new Set() : new Set([...owners.keys()].filter((id) => !theirs.has(id)));
}

const isHidden = (t: Tab, hidden: ReadonlySet<string>) => t.sel.kind === "browser" && hidden.has(t.sel.id);

/** `tabs` less the hidden ones; the same array when none is. */
export function shownTabs<T extends Tab>(tabs: T[], hidden: ReadonlySet<string>): T[] {
  return hidden.size && tabs.some((t) => isHidden(t, hidden)) ? tabs.filter((t) => !isHidden(t, hidden)) : tabs;
}

/** The key to show instead of tab `key`: the next shown tab to its right, else to its left, skipping the `gone` keys too. */
export function nearShown(tabs: readonly Tab[], hidden: ReadonlySet<string>, key: string, gone: ReadonlySet<string> = new Set()): string | null {
  const i = tabs.findIndex((t) => t.key === key);
  const shown = (t: Tab) => !gone.has(t.key) && !isHidden(t, hidden);
  return (tabs.slice(i + 1).find(shown) ?? tabs.slice(0, Math.max(i, 0)).reverse().find(shown))?.key ?? null;
}

/** The place in `tabs` of the shown tab at index `i` of the shown ones; -1 for none. */
export function shownIndexToAll(tabs: readonly Tab[], hidden: ReadonlySet<string>, i: number): number {
  const shown = tabs.filter((t) => !isHidden(t, hidden));
  return shown[i] ? tabs.indexOf(shown[i]) : -1;
}
