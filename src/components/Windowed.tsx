import { isValidElement, type ReactNode, useLayoutEffect, useRef, useState } from "react";
import { renderedRows, visibleRows } from "@/lib/ui/windowing";

/** Up to this many rows render as they are; windowing pays off only for long lists. */
const ALL = 150;
const OVERSCAN = 12;

/**
 * Fixed-height rows inside their scrolling ancestor, rendered only around what's on screen once
 * there are many (thousands of untracked files). `keep` rows render wherever they are, so the
 * active row can still scroll itself into view and the tab stop can still take focus.
 */
export function Windowed({ count, height, keep, render }: { count: number; height: number; keep: number[]; render: (i: number) => ReactNode }) {
  const box = useRef<HTMLDivElement>(null);
  const scroller = useRef<HTMLElement | null>(null);
  const [range, setRange] = useState<[number, number]>([0, 0]);
  const windowed = count > ALL;

  const update = useRef(() => {});
  update.current = () => {
    const el = box.current;
    const sc = scroller.current;
    if (!el || !sc) return;
    const offset = sc.getBoundingClientRect().top - el.getBoundingClientRect().top;
    const next = visibleRows(offset, sc.clientHeight, height, count, OVERSCAN);
    setRange((r) => (r[0] === next[0] && r[1] === next[1] ? r : next));
  };
  // The scroller is found from the box, not passed as a ref: on the first mount a parent's ref
  // isn't attached yet when these effects run, and the list then never followed its scrolling.
  useLayoutEffect(() => {
    const sc = windowed ? scrollParent(box.current) : null;
    scroller.current = sc;
    if (!sc) return;
    const onChange = () => update.current();
    onChange();
    sc.addEventListener("scroll", onChange, { passive: true });
    const resize = new ResizeObserver(onChange);
    resize.observe(sc);
    return () => {
      sc.removeEventListener("scroll", onChange);
      resize.disconnect();
    };
  }, [windowed]);
  // Every render: what sits above (another section growing, say) moves this list without a scroll.
  useLayoutEffect(() => update.current());

  if (!windowed) return <>{Array.from({ length: count }, (_, i) => render(i))}</>;
  return (
    <div ref={box} style={{ position: "relative", height: count * height }}>
      {renderedRows(range, keep, count).map((i) => {
        const row = render(i);
        return (
          <div key={isValidElement(row) && row.key !== null ? row.key : i} style={{ position: "absolute", top: i * height, left: 0, right: 0 }}>
            {row}
          </div>
        );
      })}
    </div>
  );
}

function scrollParent(el: HTMLElement | null) {
  for (let p = el?.parentElement; p; p = p.parentElement) if (/auto|scroll/.test(getComputedStyle(p).overflowY)) return p;
  return null;
}
