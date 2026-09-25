import { useCallback, useEffect, useSyncExternalStore } from "react";
import { isNotConnected } from "../api";

/**
 * Stale-while-revalidate for GitHub reads. PR views remount on every tab switch; they show
 * the last result at once and refresh in the background at most every MIN_AGE, and callers
 * asking for the same key share one request.
 */
interface Entry {
  data?: unknown;
  error?: unknown;
  /** When `data` arrived. */
  at: number;
  /** When `error` arrived. */
  failedAt?: number;
  pending?: Promise<unknown>;
}

const MIN_AGE = 30_000;
/** Enough for every PR a session opens; beyond it the oldest entries go. */
const MAX_ENTRIES = 200;
const EMPTY: Entry = { at: 0 };

let entries = new Map<string, Entry>();
const listeners = new Set<() => void>();
/** Bumped on every write, for views that read across entries (useGitHubCacheVersion). */
let version = 0;

function put(map: Map<string, Entry>, key: string, e: Entry) {
  // A reply that lands after a reset belongs to the previous repo.
  if (map !== entries) return;
  map.delete(key);
  map.set(key, e);
  // Maps iterate in insertion order and a write re-inserts, so the first key is the stalest.
  if (map.size > MAX_ENTRIES) map.delete(map.keys().next().value!);
  version++;
  listeners.forEach((l) => l());
}

/** One repo's results never show in another's workspace. */
export const resetGitHubCache = () => {
  entries = new Map();
};

/** Marks every key starting with `prefix` stale, so the next read refetches. */
export function invalidate(prefix: string) {
  for (const [key, e] of entries) if (key.startsWith(prefix)) entries.set(key, { ...e, at: 0 });
}

export function revalidate<T>(key: string, fetch: () => Promise<T>, maxAge = MIN_AGE): Promise<T> {
  const map = entries;
  const e = map.get(key) ?? EMPTY;
  if (e.pending) {
    if (maxAge > 0) return e.pending as Promise<T>;
    // A forced read (after a merge, say) must not settle for a reply that predates it. If the
    // repo changed meanwhile, the read belongs to the old one and must not start in the new.
    return e.pending
      .catch(() => {})
      .then(() => (map === entries ? revalidate(key, fetch, 0) : Promise.reject(new Error("The repository changed.")))) as Promise<T>;
  }
  if (e.data !== undefined && !e.error && Date.now() - e.at < maxAge) return Promise.resolve(e.data as T);
  // No sign-in is no answer to retry on every mount: looking for a token runs `gh` and git's
  // credential helper each time. It holds like data does; the Connect screen's retry forces.
  if (isNotConnected(e.error) && Date.now() - (e.failedAt ?? 0) < maxAge) return Promise.reject(e.error);
  const pending = fetch().then(
    (data) => {
      put(map, key, { data, at: Date.now() });
      return data;
    },
    (error) => {
      put(map, key, { ...(map.get(key) ?? EMPTY), error, failedAt: Date.now(), pending: undefined });
      throw error;
    },
  );
  put(map, key, { ...e, pending });
  return pending;
}

/** What `key` holds now, without asking for it: a list shows its shorter copy while a longer one loads. */
export const cached = <T>(key: string) => entries.get(key)?.data as T | undefined;

type Item = { url: string; updatedAt: string };

/**
 * Whether a cached list under `prefix` shows `item` other than a detail read just did (it was
 * closed or edited since): the detail and the list are fetched apart and can disagree.
 */
export function listIsBehind(prefix: string, item: Item) {
  for (const [key, e] of entries) {
    if (!key.startsWith(prefix) || !Array.isArray(e.data)) continue;
    const row = (e.data as { url?: string; updatedAt?: string }[]).find((r) => r.url === item.url);
    if (row && row.updatedAt !== item.updatedAt) return true;
  }
  return false;
}

/**
 * The newest copy of `item` any list or detail read has cached, if newer than `item`, cut to
 * `item`'s fields (a detail's body and thread don't belong in a saved tab).
 */
export function newerCopy<T extends Item>(item: T): T | null {
  let best: Item | null = null;
  for (const e of entries.values()) {
    const rows = Array.isArray(e.data) ? e.data : [e.data];
    for (const r of rows as Partial<Item>[]) {
      if (r?.url === item.url && typeof r.updatedAt === "string" && r.updatedAt > (best ?? item).updatedAt) best = r as Item;
    }
  }
  if (!best) return null;
  const fresh = best as Record<string, unknown>;
  return Object.fromEntries(Object.keys(item).map((k) => [k, k in fresh ? fresh[k] : item[k as keyof T]])) as T;
}

// GitHub doesn't tell the app when something changes there (an issue closed by a push, a
// teammate's review), so mounted views recheck on focus and every 5 minutes while visible.
// Each still honors its maxAge, and an unchanged list is a 304 that costs no rate limit.
const POLL = 300_000;
const wakers = new Set<() => void>();
const wake = () => {
  if (document.visibilityState === "visible") wakers.forEach((w) => w());
};
if (typeof window !== "undefined") {
  window.addEventListener("focus", wake);
  document.addEventListener("visibilitychange", wake);
  setInterval(wake, POLL);
}

const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => void listeners.delete(l);
};

export const useGitHubCacheVersion = () => useSyncExternalStore(subscribe, () => version);

/**
 * The cached value for `key` (null = nothing to load), revalidated on mount, when the key
 * changes, and on wake (above). `fetch` should be stable per key. `refresh(true)` ignores
 * the minimum age.
 */
export function useGitHubData<T>(key: string | null, fetch: () => Promise<T>, maxAge = MIN_AGE) {
  const e = useSyncExternalStore(subscribe, () => (key && entries.get(key)) || EMPTY);
  const refresh = useCallback(
    (force = false) => (key ? revalidate(key, fetch, force ? 0 : maxAge).then(() => {}, () => {}) : Promise.resolve()),
    [key, fetch, maxAge],
  );
  useEffect(() => {
    refresh();
    const w = () => void refresh();
    wakers.add(w);
    return () => void wakers.delete(w);
  }, [refresh]);
  return { data: e.data as T | undefined, error: e.error, loading: !!e.pending, refresh };
}
