import { Activity, type ReactNode, useLayoutEffect, useRef, useState } from "react";

/**
 * A panel kept while another shows in its place (the sidebar's tabs): its state stays (filters,
 * loaded pages, the open commit) and its effects stop while hidden, so nothing there polls.
 */
export function KeptPanel({ shown, children }: { shown: boolean; children: ReactNode }) {
  return (
    <Activity mode={shown ? "visible" : "hidden"}>
      <KeepScroll>{children}</KeepScroll>
    </Activity>
  );
}

/** Puts the panel's scrollers back where they were: WebKit drops a scroller's offset under display: none. */
function KeepScroll({ children }: { children: ReactNode }) {
  const box = useRef<HTMLDivElement>(null);
  const [tops] = useState(() => new Map<Element, number>());
  // Recorded as it scrolls, not on hiding: by then the offset may already be gone.
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    const save = (e: Event) => void (e.target instanceof Element && tops.set(e.target, e.target.scrollTop));
    el.addEventListener("scroll", save, { capture: true, passive: true });
    return () => el.removeEventListener("scroll", save, { capture: true });
  }, [tops]);
  return (
    <div ref={box} className="h-full">
      <Restore tops={tops} />
      {children}
    </div>
  );
}

/** Rendered before the panel, so its layout effect (run again on each show) restores before the lists' own measure (Windowed). */
function Restore({ tops }: { tops: Map<Element, number> }) {
  useLayoutEffect(() => {
    for (const [el, top] of tops) {
      if (el.isConnected) el.scrollTop = top;
      else tops.delete(el);
    }
  }, [tops]);
  return null;
}
