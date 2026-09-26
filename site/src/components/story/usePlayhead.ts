import { useCallback, useEffect, useRef, useState } from "react";

// A seek lands over about this long, so a wheel tick fast-forwards instead of jumping.
const EASE_MS = 180;
// Re-render at most this often: the window is a big tree, and 30fps reads as smooth.
const FRAME_MS = 33;

/**
 * A clip's playhead: runs in real time while `running` and starts over after `length + hold`.
 * `seek(ms)` moves it by that much, eased in, so scrolling fast-forwards (or rewinds) the clip
 * rather than skipping it. A seek stops at the clip's ends; only time loops it. Coming back to an
 * earlier key (scrolling up) starts at its end, so the rewind carries on. `wait(true)` holds the
 * frame, for a clip whose copy hasn't settled yet.
 */
export function usePlayhead(key: number, running: boolean, { length, hold }: { length: number; hold: number }) {
  const [clock, setClock] = useState({ key, t: 0 });
  const pending = useRef(0);
  const waiting = useRef(false);
  const lastKey = useRef(key);
  useEffect(() => {
    if (!running) return;
    pending.current = 0;
    let t = key < lastKey.current ? length : 0;
    lastKey.current = key;
    setClock({ key, t });
    let shown = t;
    let last = performance.now();
    let frame = requestAnimationFrame(function tick(now) {
      const dt = now - last;
      last = now;
      const step = pending.current * Math.min(1, dt / EASE_MS);
      pending.current -= step;
      if (!waiting.current) t += dt;
      t = step >= 0 ? (t >= length ? t : Math.min(length, t + step)) : Math.max(0, t + step);
      if (t >= length + hold) t = 0;
      if (Math.abs(t - shown) >= FRAME_MS) {
        shown = t;
        setClock({ key, t });
      }
      frame = requestAnimationFrame(tick);
    });
    return () => cancelAnimationFrame(frame);
  }, [key, running, length, hold]);
  const seek = useCallback((ms: number) => {
    pending.current += ms;
  }, []);
  const wait = useCallback((on: boolean) => {
    waiting.current = on;
  }, []);
  const t = !running ? Infinity : clock.key === key ? clock.t : key < lastKey.current ? length : 0;
  return [t, seek, wait] as const;
}
