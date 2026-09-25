import { useEffect } from "react";
import { api, CANCELLED, errorMessage, networkBusy, netOp } from "../api";
import { useSettings } from "../settings";

const FIRST_CHECK_MS = 5_000;
const CHECK_EVERY_MS = 60_000;

/**
 * Fetches the open repo quietly once its last fetch, by anyone (the app, a terminal, an agent),
 * is older than the interval set in Settings. Checked shortly after it opens, then every minute.
 * A failed try also waits a full interval, so being offline doesn't mean a try every minute.
 */
export function useBackgroundFetch(root: string, hasRemotes: boolean, refresh: () => Promise<void>) {
  const minutes = useSettings().backgroundFetch;
  useEffect(() => {
    if (!minutes || !hasRemotes) return;
    let lastTry = 0;
    let running = false;
    const check = async () => {
      if (running || networkBusy()) return;
      running = true;
      const before = lastTry;
      try {
        const last = Math.max((await api.lastFetch()) ?? 0, lastTry);
        const now = Date.now() / 1000;
        if (now - last < minutes * 60 || networkBusy()) return;
        lastTry = now;
        await api.fetch(netOp(undefined, true));
        await refresh();
      } catch (e) {
        // It gave way to something the user started: not a failed try, so the next check retries.
        if (e === CANCELLED) lastTry = before;
        else console.warn("Background fetch:", errorMessage(e));
      } finally {
        running = false;
      }
    };
    const first = setTimeout(check, FIRST_CHECK_MS);
    const timer = setInterval(check, CHECK_EVERY_MS);
    return () => {
      clearTimeout(first);
      clearInterval(timer);
    };
  }, [root, minutes, hasRemotes, refresh]);
}
