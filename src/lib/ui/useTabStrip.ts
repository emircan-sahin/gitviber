import { useLayoutEffect, useRef, type WheelEvent } from "react";

/**
 * A row of tabs that scrolls sideways, with no bar (`data-scrollbar="none"`), when it doesn't fit.
 * A mouse wheel scrolls it too, where only a trackpad would, and the tab on show (aria-selected or
 * aria-pressed) is scrolled into view as `selected` changes.
 */
export function useTabStrip<T extends HTMLElement>(selected: unknown) {
  const ref = useRef<T>(null);
  useLayoutEffect(() => {
    const strip = ref.current;
    const tab = strip?.querySelector('[aria-selected="true"], [aria-pressed="true"]');
    if (!strip || !tab) return;
    // The strip only: scrollIntoView would scroll the panels around it too, a collapsed one included.
    const s = strip.getBoundingClientRect();
    const t = tab.getBoundingClientRect();
    if (t.left < s.left) strip.scrollLeft -= s.left - t.left;
    else if (t.right > s.right) strip.scrollLeft += Math.min(t.right - s.right, t.left - s.left);
  }, [selected]);
  const onWheel = (e: WheelEvent<T>) => {
    if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) e.currentTarget.scrollLeft += e.deltaY;
  };
  return { ref, onWheel };
}
