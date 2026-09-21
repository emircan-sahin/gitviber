import { useEffect, useMemo, useState, useSyncExternalStore } from "react";

/** One line of highlighted code: [text, color, fontStyle bitmask (1 italic, 2 bold, 4 underline)]. */
export type TokenLine = [string, string, number][];
export interface Highlighted {
  lines: TokenLine[];
  fg: string;
}

// The language of the code view on screen, for the status bar (which sits outside the viewer).
let shownLanguage: string | null = null;
const languageListeners = new Set<() => void>();
export function showLanguage(lang: string | null) {
  shownLanguage = lang;
  languageListeners.forEach((l) => l());
}
function subscribeLanguage(listener: () => void) {
  languageListeners.add(listener);
  return () => void languageListeners.delete(listener);
}
export function useShownLanguage() {
  return useSyncExternalStore(subscribeLanguage, () => shownLanguage);
}

// Minified or giant files: tokenizing them costs more than it helps.
const MAX_CHARS = 1_500_000;
const MAX_LINE = 4000;

let worker: Worker | null = null;
let nextId = 1;
const pending = new Map<number, (r: Highlighted | null) => void>();
// Keyed by a hash so a big file's text isn't copied into a key string on every revision;
// the entry keeps a reference to the (shared) text to rule out collisions.
const cache = new Map<string, { code: string; data: Highlighted }>();
const inflight = new Map<string, Promise<Highlighted | null>>();
function cacheKey(code: string, lang: string, theme: string) {
  let h = 0x811c9dc5;
  for (let i = 0; i < code.length; i++) h = Math.imul(h ^ code.charCodeAt(i), 0x01000193);
  return `${theme}\0${lang}\0${code.length}\0${h >>> 0}`;
}
function cached(key: string, code: string) {
  const hit = cache.get(key);
  if (!hit || hit.code !== code) return undefined;
  // Real LRU: a hit moves to the back so files being reviewed stay cached.
  cache.delete(key);
  cache.set(key, hit);
  return hit.data;
}

function getWorker() {
  if (!worker) {
    worker = new Worker(new URL("./highlight.worker.ts", import.meta.url), { type: "module" });
    worker.onmessage = (e) => {
      const { id, lines, fg, error } = e.data;
      pending.get(id)?.(error ? null : { lines, fg });
      pending.delete(id);
    };
    // If the worker dies, settle everything waiting on it (plain text) and start fresh next time.
    worker.onerror = () => {
      pending.forEach((resolve) => resolve(null));
      pending.clear();
      worker?.terminate();
      worker = null;
    };
  }
  return worker;
}

function highlight(code: string, lang: string, theme: string): Promise<Highlighted | null> {
  if (lang === "text" || code.length > MAX_CHARS || code.split("\n", 2000).some((l) => l.length > MAX_LINE)) {
    return Promise.resolve(null);
  }
  const key = cacheKey(code, lang, theme);
  const hit = cached(key, code);
  if (hit) return Promise.resolve(hit);
  const running = inflight.get(key);
  if (running) return running;
  const job = new Promise<Highlighted | null>((resolve) => {
    const id = nextId++;
    pending.set(id, (r) => {
      inflight.delete(key);
      if (r) {
        cache.set(key, { code, data: r });
        // Small LRU: the files being reviewed now plus prefetched neighbours.
        if (cache.size > 48) cache.delete(cache.keys().next().value!);
      }
      resolve(r);
    });
    getWorker().postMessage({ id, code, lang, theme });
  });
  inflight.set(key, job);
  return job;
}

/** Warms the cache so opening this code later shows colors immediately. */
export function prefetchHighlight(code: string, lang: string, theme: string) {
  void highlight(code, lang, theme);
}

/**
 * Tokens for `code`. While a new version is being tokenized the previous result is
 * returned with `fresh: false`; callers reuse it only for lines whose text is unchanged,
 * so live edits don't flash to plain text.
 */
export function useHighlight(code: string | null, lang: string, theme: string) {
  const [result, setResult] = useState<{ code: string; lang: string; theme: string; data: Highlighted | null } | null>(null);
  useEffect(() => {
    if (code == null) return;
    let alive = true;
    highlight(code, lang, theme).then((data) => alive && setResult({ code, lang, theme, data }));
    return () => {
      alive = false;
    };
  }, [code, lang, theme]);
  // A cache hit is used in the same render, so prefetched files open already colored.
  const directKey = useMemo(() => (code != null ? cacheKey(code, lang, theme) : null), [code, lang, theme]);
  const entry = directKey ? cache.get(directKey) : undefined;
  const direct = entry && entry.code === code ? entry.data : undefined;
  const data = direct ?? (result?.lang === lang && result.theme === theme ? result.data : null);
  const fresh = !!direct || result?.code === code;
  // Stable identity so memoized rows don't re-render on every scroll frame.
  return useMemo(() => (data ? { data, fresh } : null), [data, fresh]);
}

/** Looks up tokens for a line: by index when fresh, by identical text when stale. */
export function tokenLookup(h: ReturnType<typeof useHighlight>) {
  if (!h) return () => undefined;
  if (h.fresh) return (index: number) => h.data.lines[index];
  const byText = new Map<string, TokenLine>();
  for (const line of h.data.lines) byText.set(line.map((t) => t[0]).join(""), line);
  return (_index: number, text: string) => byText.get(text);
}
