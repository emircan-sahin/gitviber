import { useEffect, useState } from "react";

const TICK_MS = 50;

/**
 * Milliseconds since `key` last changed, while `running`, starting over every `loop` ms so a clip
 * nobody finished watching plays again. Keyed so a change never renders the new key with the old
 * time. While not running it reads `idle`: Infinity (the finished state) by default, 0 for
 * something that hasn't started yet.
 */
export function useElapsed(key: number, running: boolean, { loop, idle = Infinity }: { loop: number; idle?: number }) {
  const [clock, setClock] = useState({ key, t: 0 });
  useEffect(() => {
    if (!running) return;
    const start = performance.now();
    setClock({ key, t: 0 });
    const timer = setInterval(() => setClock({ key, t: (performance.now() - start) % loop }), TICK_MS);
    return () => clearInterval(timer);
  }, [key, running, loop]);
  if (!running) return idle;
  return clock.key === key ? clock.t : 0;
}
