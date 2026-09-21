import { useEffect, useRef } from "react";

export type Lane = "add" | "del" | "mod";
export interface Mark {
  lane: Lane;
  /** Item index range [i0, i1); i0 === i1 marks a point (a deletion between rows). */
  i0: number;
  i1: number;
}

const COLORS: Record<Lane, string> = { add: "--added", del: "--removed", mod: "--primary" };

/**
 * VS Code-style overview ruler that doubles as the vertical scrollbar: change marks for
 * the whole file, a draggable viewport box, click anywhere to jump there.
 */
export function OverviewRuler({
  marks,
  offsetOf,
  split,
  ready,
  scrollRef,
}: {
  marks: Mark[];
  /** Content offset (px) of an item index, measured from the real layout. */
  offsetOf: (i: number) => number;
  split: boolean;
  /** False while the code view holds back content; re-measure once it appears. */
  ready: boolean;
  scrollRef: React.RefObject<HTMLDivElement | null>;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const thumbRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current!;
    const scroller = scrollRef.current;
    if (!scroller) return;
    const css = getComputedStyle(document.documentElement);
    const color = (lane: Lane) => css.getPropertyValue(COLORS[lane]).trim();

    const draw = () => {
      const dpr = window.devicePixelRatio || 1;
      const w = canvas.clientWidth;
      const h = canvas.clientHeight;
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(h * dpr);
      const g = canvas.getContext("2d")!;
      g.scale(dpr, dpr);
      g.clearRect(0, 0, w, h);
      // Same space as the viewport box: the scroller's real height, which for a short
      // file is just the viewport, so marks sit level with the code.
      const scale = scroller.scrollHeight > 0 ? scroller.clientHeight / scroller.scrollHeight : 0;
      for (const m of marks) {
        const y0 = offsetOf(m.i0);
        const y1 = offsetOf(m.i1);
        // Diffs: removals on the left lane, additions on the right, like VS Code.
        const [x, lw] = split ? (m.lane === "del" ? [2, w / 2 - 2] : [w / 2, w / 2 - 2]) : [3, w - 6];
        g.fillStyle = color(m.lane);
        g.fillRect(x, y0 * scale, lw, Math.max(2, (y1 - y0) * scale));
      }
    };

    const moveThumb = () => {
      const t = thumbRef.current!;
      const { scrollTop, scrollHeight, clientHeight } = scroller;
      // Map onto the scroller's visible height (a horizontal scrollbar can make it shorter than the ruler).
      const h = clientHeight;
      const visible = scrollHeight > clientHeight;
      t.style.display = visible ? "block" : "none";
      if (!visible) return;
      t.style.top = `${(scrollTop / scrollHeight) * h}px`;
      t.style.height = `${Math.max(12, (clientHeight / scrollHeight) * h)}px`;
    };

    draw();
    moveThumb();
    scroller.addEventListener("scroll", moveThumb, { passive: true });
    const ro = new ResizeObserver(() => {
      draw();
      moveThumb();
    });
    ro.observe(canvas);
    if (scroller.firstElementChild) ro.observe(scroller.firstElementChild);
    return () => {
      scroller.removeEventListener("scroll", moveThumb);
      ro.disconnect();
    };
  }, [marks, offsetOf, split, ready, scrollRef]);

  // Press anywhere: center that spot; keep dragging to scrub.
  const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    const scroller = scrollRef.current;
    if (!scroller) return;
    const rect = e.currentTarget.getBoundingClientRect();
    const thumb = thumbRef.current!.getBoundingClientRect();
    // Grabbing the thumb keeps the grab point under the cursor instead of re-centering.
    const grab = e.clientY >= thumb.top && e.clientY <= thumb.bottom ? e.clientY - thumb.top : thumb.height / 2;
    const go = (clientY: number) => {
      const f = (clientY - rect.top - grab) / scroller.clientHeight;
      scroller.scrollTop = f * scroller.scrollHeight;
    };
    go(e.clientY);
    e.currentTarget.setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => go(ev.clientY);
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  return (
    <div onPointerDown={onPointerDown} className="relative w-3.5 shrink-0 cursor-pointer border-l border-border bg-panel">
      <canvas ref={canvasRef} className="absolute inset-0 size-full" />
      <div ref={thumbRef} className="pointer-events-none absolute inset-x-0 border-y border-foreground/15 bg-foreground/[0.08]" />
    </div>
  );
}

