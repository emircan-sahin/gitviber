/** How often relative times ("5m ago") are worked out again. */
export const MINUTE = 60_000;

/**
 * One timer for every relative time on screen: it runs only while one is mounted and the window
 * shows, and a window shown again catches up at once.
 */
export function minuteTicker(env: { visible: () => boolean; every: (fn: () => void, ms: number) => () => void }) {
  const listeners = new Set<() => void>();
  let stop: (() => void) | null = null;
  const tick = () => listeners.forEach((l) => l());
  const sync = () => {
    const want = listeners.size > 0 && env.visible();
    if (want && !stop) stop = env.every(tick, MINUTE);
    else if (!want && stop) {
      stop();
      stop = null;
    }
  };
  return {
    subscribe(l: () => void) {
      listeners.add(l);
      sync();
      return () => {
        listeners.delete(l);
        sync();
      };
    },
    visibilityChanged() {
      if (env.visible()) tick();
      sync();
    },
  };
}

export const minute = minuteTicker({
  visible: () => typeof document === "undefined" || document.visibilityState === "visible",
  every: (fn, ms) => {
    const id = setInterval(fn, ms);
    return () => clearInterval(id);
  },
});
if (typeof document !== "undefined") document.addEventListener("visibilitychange", minute.visibilityChanged);
