import { useEffect, useLayoutEffect, useRef } from "react";
import { focusPanel } from "./panels";

/**
 * The keyboard for the sidebar lists (History, Pull Requests, Issues), one hook so they can't
 * drift apart. Rows are the `[data-row]` elements inside, top to bottom, and keys do what the
 * mouse does on them: landing on a row is a click (the preview), ↵ a double-click (keeps the
 * tab), → goes on to its code. A row with aria-expanded (a commit) only opens and closes, with
 * ↵, Space or → / ←.
 */
export function useListNav({ activeKey, loadMore }: { activeKey: string | null; loadMore?: (() => Promise<unknown>) | null }) {
  const ref = useRef<HTMLDivElement>(null);
  // The row focus was last on: the list's one tab stop, else the open row, else the first.
  const cursor = useRef<string | null>(null);
  // ↓ on the last row loads more: the row to move on to once it's there.
  const pending = useRef<number | null>(null);
  const loading = useRef(false);

  const syncStops = () => {
    const rows = rowsIn(ref.current);
    const stop = rows.find((r) => r.dataset.row === cursor.current) ?? rows.find((r) => r.dataset.row === activeKey) ?? rows[0];
    for (const r of rows) r.tabIndex = r === stop ? 0 : -1;
  };

  useLayoutEffect(() => {
    const at = pending.current;
    const rows = rowsIn(ref.current);
    if (at !== null && rows.length > at) {
      pending.current = null;
      // Unless focus has moved on meanwhile.
      if (document.activeElement === rows[at - 1]) land(rows[at]);
    }
    syncStops();
  });

  // Opened from elsewhere (a pinned tab, a new PR): bring the row into view.
  useEffect(() => {
    if (activeKey) rowsIn(ref.current).find((r) => r.dataset.row === activeKey)?.scrollIntoView({ block: "nearest" });
  }, [activeKey]);

  const onKeyDown = (e: React.KeyboardEvent) => {
    // The row itself, not a button inside it.
    const row = e.target instanceof HTMLElement && e.target.matches("[data-row]") ? e.target : null;
    if (!row || e.altKey || e.metaKey || e.ctrlKey) return;
    if (isMenuKey(e)) {
      openRowMenu(row);
      e.preventDefault();
      return;
    }
    if (e.shiftKey) return;
    const rows = rowsIn(ref.current);
    const i = rows.indexOf(row);
    const parent = row.hasAttribute("aria-expanded");
    const expanded = row.getAttribute("aria-expanded") === "true";
    const to = moveTarget(e.key, i, rows.length, pageOf(row));
    if (to !== null) {
      if (e.key === "ArrowDown" && i === rows.length - 1 && loadMore) {
        if (!loading.current) {
          loading.current = true;
          pending.current = rows.length;
          void loadMore().finally(() => (loading.current = false));
        }
      } else if (to !== i) land(rows[to]);
    } else if (e.key === "Enter") {
      if (parent) row.click();
      else row.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    } else if (e.key === " ") row.click();
    else if (e.key === "ArrowRight" && parent) {
      if (!expanded) row.click();
      else if (rows[i + 1]) land(rows[i + 1]);
    } else if (e.key === "ArrowRight") {
      if (row.dataset.row !== activeKey) row.click();
      focusPanel("code");
    } else if (e.key === "ArrowLeft") {
      if (expanded) row.click();
      else if (Number(row.getAttribute("aria-level")) > 1) {
        const up = rows.slice(0, i).reverse().find((r) => r.hasAttribute("aria-expanded"));
        if (up) land(up);
      } else return;
    } else return;
    e.preventDefault();
  };

  const onFocus = (e: React.FocusEvent) => {
    if (!(e.target instanceof HTMLElement) || e.target.dataset.row === undefined) return;
    cursor.current = e.target.dataset.row;
    syncStops();
  };

  // Marks the list for isTyping: a listbox of its own, but no typeahead to keep keys from.
  return { ref, onKeyDown, onFocus, "data-list-nav": "" };
}

const rowsIn = (el: HTMLElement | null) => (el ? [...el.querySelectorAll<HTMLElement>("[data-row]")] : []);

/** Moves focus to a row and previews it, as a click would; a commit row only takes focus. */
function land(row: HTMLElement) {
  row.focus();
  row.scrollIntoView({ block: "nearest" });
  if (!row.hasAttribute("aria-expanded")) row.click();
}

/** Where a navigation key moves from row `i` of `n`, or null when it isn't one. */
export function moveTarget(key: string, i: number, n: number, page: number): number | null {
  const to = ({ ArrowDown: i + 1, ArrowUp: i - 1, Home: 0, End: n - 1, PageDown: i + page, PageUp: i - page } as Record<string, number>)[key];
  return to === undefined ? null : Math.max(0, Math.min(n - 1, to));
}

/** Rows a PageUp/PageDown moves: as many as fit in the scroller, one kept for context. */
export function pageOf(row: HTMLElement) {
  let el = row.parentElement;
  while (el && !(el.scrollHeight > el.clientHeight && /auto|scroll/.test(getComputedStyle(el).overflowY))) el = el.parentElement;
  return Math.max(1, Math.floor((el?.clientHeight ?? 0) / row.offsetHeight) - 1);
}

/** ⇧F10, or the menu key some keyboards have. */
export const isMenuKey = (e: React.KeyboardEvent) => e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey && !e.altKey && !e.metaKey && !e.ctrlKey);

/** Opens a row's right-click menu from the keyboard, just under the row. */
export function openRowMenu(row: HTMLElement) {
  const r = row.getBoundingClientRect();
  row.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: r.left + 24, clientY: r.bottom }));
}
