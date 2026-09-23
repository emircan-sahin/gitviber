import { isValidElement, type ReactNode, type RefObject, useLayoutEffect, useRef, useState } from "react";

/** Up to this many rows render as they are; windowing pays off only for long lists. */
const ALL = 150;
const OVERSCAN = 12;

/**
 * Fixed-height rows inside the scrolling `scroller`, rendered only around what's on screen once
 * there are many (thousands of untracked files). `keep` rows render wherever they are, so the
 * active row can still scroll itself into view and the tab stop can still take focus.
 */
export function Windowed({
  count,
  height,
  scroller,
  keep,
  render,
}: {
  count: number;
  height: number;
  scroller: RefObject<HTMLElement | null>;
  keep: number[];
  render: (i: number) => ReactNode;
}) {
  const box = useRef<HTMLDivElement>(null);
  const [range, setRange] = useState<[number, number]>([0, 0]);
  const windowed = count > ALL;

  const update = useRef(() => {});
  update.current = () => {
    const el = box.current;
    const sc = scroller.current;
    if (!el || !sc) return;
    const top = sc.getBoundingClientRect().top - el.getBoundingClientRect().top;
    const first = Math.max(0, Math.floor(top / height) - OVERSCAN);
    const last = Math.min(count, Math.ceil((top + sc.clientHeight) / height) + OVERSCAN);
    setRange((r) => (r[0] === first && r[1] === last ? r : [first, last]));
  };
  // Every render: what sits above (another section growing, say) moves this list without a scroll.
  useLayoutEffect(() => update.current());
  useLayoutEffect(() => {
    const sc = scroller.current;
    if (!windowed || !sc) return;
    const onChange = () => update.current();
    sc.addEventListener("scroll", onChange, { passive: true });
    const resize = new ResizeObserver(onChange);
    resize.observe(sc);
    return () => {
      sc.removeEventListener("scroll", onChange);
      resize.disconnect();
    };
  }, [windowed, scroller]);

  if (!windowed) return <>{Array.from({ length: count }, (_, i) => render(i))}</>;
  const shown = new Set<number>();
  for (let i = range[0]; i < Math.min(range[1], count); i++) shown.add(i);
  for (const i of keep) if (i >= 0 && i < count) shown.add(i);
  return (
    <div ref={box} style={{ position: "relative", height: count * height }}>
      {[...shown]
        .sort((a, b) => a - b)
        .map((i) => {
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
