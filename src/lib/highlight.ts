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

// Giant files: tokenizing them costs more than it helps. (Minified lines are skipped in the worker.)
const MAX_CHARS = 1_500_000;
// Queued prefetches beyond this are dropped, oldest first: hovering down a list shouldn't
// leave a backlog of files you have moved past.
const MAX_PREFETCH = 4;

interface Job {
  key: string;
  code: string;
  lang: string;
  theme: string;
  /** Mounted views waiting for this job; a view's job is dropped when it's no longer shown. */
  viewers: number;
  prefetched: boolean;
  promise: Promise<Highlighted | null>;
  resolve: (r: Highlighted | null) => void;
}

let worker: Worker | null = null;
let nextId = 1;
// The worker gets one job at a time, so the file on screen goes ahead of anything queued
// before it instead of waiting behind prefetches.
const queue: Job[] = [];
let running: { id: number; job: Job } | null = null;
const jobs = new Map<string, Job>();
// Keyed by a hash so a big file's text isn't copied into a key string on every revision;
// the entry keeps a reference to the (shared) text to rule out collisions.
const cache = new Map<string, { code: string; data: Highlighted }>();
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
      if (!running || running.id !== id) return;
      const { job } = running;
      running = null;
      finish(job, error ? null : { lines, fg });
      pump();
    };
    // If the worker dies, the job it was on shows as plain text; the rest go to a fresh worker.
    worker.onerror = () => {
      worker?.terminate();
      worker = null;
      if (running) finish(running.job, null);
      running = null;
      pump();
    };
  }
  return worker;
}

/** Starts the worker (and its WASM engine) ahead of the first file, so that one opens sooner. */
export function warmHighlighter() {
  getWorker();
}

function finish(job: Job, r: Highlighted | null) {
  jobs.delete(job.key);
  if (r) {
    cache.set(job.key, { code: job.code, data: r });
    // Small LRU: the files being reviewed now plus prefetched neighbours.
    if (cache.size > 48) cache.delete(cache.keys().next().value!);
  }
  job.resolve(r);
}

function drop(job: Job) {
  queue.splice(queue.indexOf(job), 1);
  finish(job, null);
}

function pump() {
  if (running || !queue.length) return;
  // Views in the order they asked, then the most recent prefetch.
  const i = queue.findIndex((j) => j.viewers > 0);
  const job = queue.splice(i < 0 ? queue.length - 1 : i, 1)[0];
  running = { id: nextId++, job };
  getWorker().postMessage({ id: running.id, code: job.code, lang: job.lang, theme: job.theme });
}

const settled = (r: Highlighted | null) => ({ promise: Promise.resolve(r), release: () => {} });

export function highlight(code: string, lang: string, theme: string, view: boolean) {
  if (lang === "text" || code.length > MAX_CHARS) return settled(null);
  const key = cacheKey(code, lang, theme);
  const hit = cached(key, code);
  if (hit) return settled(hit);
  let job = jobs.get(key);
  if (!job) {
    let resolve!: Job["resolve"];
    const promise = new Promise<Highlighted | null>((r) => (resolve = r));
    job = { key, code, lang, theme, viewers: 0, prefetched: false, promise, resolve };
    jobs.set(key, job);
    queue.push(job);
  }
  const j = job;
  if (view) j.viewers++;
  else {
    j.prefetched = true;
    // Asked again: it's the most recent prefetch now.
    const at = queue.indexOf(j);
    if (at >= 0) queue.push(...queue.splice(at, 1));
    const waiting = queue.filter((q) => q.viewers === 0);
    for (const old of waiting.slice(0, Math.max(0, waiting.length - MAX_PREFETCH))) drop(old);
  }
  pump();
  return {
    promise: j.promise,
    release: () => {
      // A revision nobody shows anymore (the file changed again, or you moved on): skip it.
      if (--j.viewers === 0 && !j.prefetched && queue.includes(j)) drop(j);
    },
  };
}

/** Warms the cache so opening this code later shows colors immediately. */
export function prefetchHighlight(code: string, lang: string, theme: string) {
  highlight(code, lang, theme, false);
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
    const { promise, release } = highlight(code, lang, theme, true);
    promise.then((data) => alive && setResult({ code, lang, theme, data }));
    return () => {
      alive = false;
      release();
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
