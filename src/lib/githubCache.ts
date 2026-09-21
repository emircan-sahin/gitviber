import { useCallback, useEffect, useSyncExternalStore } from "react";

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
  pending?: Promise<unknown>;
}

const MIN_AGE = 30_000;
/** Enough for every PR a session opens; beyond it the oldest entries go. */
const MAX_ENTRIES = 200;
const EMPTY: Entry = { at: 0 };

let entries = new Map<string, Entry>();
const listeners = new Set<() => void>();

function put(map: Map<string, Entry>, key: string, e: Entry) {
  // A reply that lands after a reset belongs to the previous repo.
  if (map !== entries) return;
  map.delete(key);
  map.set(key, e);
  // Maps iterate in insertion order and a write re-inserts, so the first key is the stalest.
  if (map.size > MAX_ENTRIES) map.delete(map.keys().next().value!);
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
  const pending = fetch().then(
    (data) => {
      put(map, key, { data, at: Date.now() });
      return data;
    },
    (error) => {
      put(map, key, { ...(map.get(key) ?? EMPTY), error, pending: undefined });
      throw error;
    },
  );
  put(map, key, { ...e, pending });
  return pending;
}

const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => void listeners.delete(l);
};

/**
 * The cached value for `key` (null = nothing to load), revalidated on mount and when the
 * key changes. `fetch` should be stable per key. `refresh(true)` ignores the minimum age.
 */
export function useGitHubData<T>(key: string | null, fetch: () => Promise<T>, maxAge = MIN_AGE) {
  const e = useSyncExternalStore(subscribe, () => (key && entries.get(key)) || EMPTY);
  const refresh = useCallback(
    (force = false) => (key ? revalidate(key, fetch, force ? 0 : maxAge).then(() => {}, () => {}) : Promise.resolve()),
    [key, fetch, maxAge],
  );
  useEffect(() => {
    refresh();
  }, [refresh]);
  return { data: e.data as T | undefined, error: e.error, loading: !!e.pending, refresh };
}
