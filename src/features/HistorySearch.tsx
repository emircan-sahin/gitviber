import { FileClock, FolderClock, Loader2, Search, X } from "lucide-react";
import { type ComponentProps, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type Commit, errorMessage } from "@/lib/api";
import { isTyping, useShortcut } from "@/lib/keybindings";
import { isEmptyFilter, parseLogQuery } from "@/lib/logQuery";
import { ForkHistory } from "./ForkHistory";
import { HistoryPanel, type Reveal } from "./HistoryPanel";

const PAGE = 200;
const DEBOUNCE = 250;

export interface HistorySearch {
  query: string;
  /** "Show History" of a file (followed through renames) or a folder, shown as a chip. */
  scope: { path: string; file: boolean } | null;
  /** Blame's link to a commit: open it, and this file in it, once the search finds it. */
  reveal: Reveal | null;
}

export const NO_SEARCH: HistorySearch = { query: "", scope: null, reveal: null };

type Props = ComponentProps<typeof ForkHistory> & {
  search: HistorySearch;
  onSearch: (search: HistorySearch) => void;
  /** The search command asked for the box; `onFocused` says it has it. */
  focusRequested: boolean;
  onFocused: () => void;
};

/** History with a search box on top; while it's searching, the matches replace the full history. */
export function SearchableHistory({ search, onSearch, focusRequested, onFocused, ...props }: Props) {
  const { query, scope, reveal } = search;
  const input = useRef<HTMLInputElement>(null);
  const shortcut = useShortcut("history.search");
  const active = !!scope || !isEmptyFilter(parseLogQuery(query).filter);
  const found = useCommitSearch(active ? search : null, props.commits[0]?.sha);
  const setQuery = (q: string) => onSearch({ ...search, query: q, reveal: null });

  useEffect(() => {
    if (!focusRequested) return;
    input.current?.focus();
    input.current?.select();
    onFocused();
  }, [focusRequested, onFocused]);

  const ScopeIcon = scope?.file ? FileClock : FolderClock;
  return (
    <div
      // Takes focus from clicks in the list, so `/` can reach the search box from there.
      tabIndex={-1}
      onKeyDown={(e) => {
        if (e.key !== "/" || e.metaKey || e.ctrlKey || e.altKey || isTyping(e.nativeEvent)) return;
        e.preventDefault();
        input.current?.focus();
      }}
      className="flex h-full flex-col outline-none"
    >
      <div className="flex h-8 shrink-0 items-center gap-2 border-b border-border px-2.5">
        {found.pending ? <Loader2 className="size-3.5 shrink-0 animate-spin text-subtle" /> : <Search className="size-3.5 shrink-0 text-subtle" />}
        <input
          ref={input}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key !== "Escape") return;
            e.preventDefault();
            // The words first, then the file.
            if (query) setQuery("");
            else if (scope) onSearch({ ...search, scope: null });
            else input.current?.blur();
          }}
          placeholder={`Search commits${shortcut ? ` (${shortcut})` : ""}  ·  author: path: code:`}
          title={"Words match the message (all of them, any case).\nauthor:name  path:src/app  code:text a commit added or removed\nA SHA or prefix finds that commit. Quotes keep spaces."}
          spellCheck={false}
          className="h-full min-w-0 flex-1 bg-transparent text-[12px] outline-none placeholder:text-subtle"
        />
        {query && (
          <button aria-label="Clear search" onClick={() => setQuery("")} className="flex size-4 shrink-0 items-center justify-center rounded-sm text-subtle hover:bg-hover hover:text-foreground">
            <X className="size-3" />
          </button>
        )}
      </div>
      {scope && (
        <div className="flex shrink-0 items-center gap-1 border-b border-border px-2 py-1.5">
          <span className="inline-flex min-w-0 items-center gap-1 rounded-full border border-border-strong bg-active pr-0.5 pl-1.5 text-[10.5px] leading-4" title={scope.path}>
            <ScopeIcon className="size-3 shrink-0 text-subtle" />
            <span className="truncate font-mono">{scope.path}</span>
            <button
              aria-label="Show all history"
              onClick={() => onSearch({ ...search, scope: null })}
              className="flex size-3.5 shrink-0 items-center justify-center rounded-full text-subtle hover:bg-hover hover:text-foreground"
            >
              <X className="size-2.5" />
            </button>
          </span>
        </div>
      )}
      <div className="min-h-0 flex-1">
        {!active ? (
          <ForkHistory {...props} />
        ) : found.error && !found.commits ? (
          <div className="px-4 py-6 text-center text-[12px] text-muted-foreground">{found.error}</div>
        ) : (
          <HistoryPanel
            {...props}
            commits={found.commits ?? []}
            hasMore={found.hasMore}
            loadMore={found.loadMore}
            // Undo and reset act on HEAD, which a list of matches needn't start with.
            headSha={props.commits[0]?.sha ?? ""}
            empty={found.commits ? "No commits match." : "Searching…"}
            reveal={reveal}
          />
        )}
      </div>
    </div>
  );
}

/** The log filter a search asks for, and the words in it that could be a SHA. */
function request({ query, scope }: HistorySearch) {
  const { filter, shas } = parseLogQuery(query);
  if (!scope) return { filter, shas };
  // git follows one path only: a typed `path:` next to the file's turns following off.
  return { filter: { ...filter, paths: [scope.path, ...filter.paths], follow: scope.file && !filter.paths.length }, shas };
}

/**
 * The commits `search` matches, a page at a time, then any commit a SHA in it names on top.
 * Typing waits DEBOUNCE before asking; a reply to an older search is dropped. `head`: HEAD's
 * commit, to search again when it moves.
 */
function useCommitSearch(search: HistorySearch | null, head: string | undefined) {
  const [result, setResult] = useState<{ key: string; log: Commit[]; found: Commit[]; hasMore: boolean } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const seq = useRef(0);
  const current = useRef(result);
  current.current = result;
  const key = search && JSON.stringify([search.query, search.scope]);
  const latest = useRef(search);
  latest.current = search;

  useEffect(() => {
    const id = ++seq.current;
    const s = latest.current;
    if (!key || !s) {
      setResult(null);
      setPending(false);
      return;
    }
    setPending(true);
    const { filter, shas } = request(s);
    // A refresh of the same search keeps the pages loaded so far.
    const prev = current.current;
    const limit = Math.max(PAGE, prev?.key === key ? prev.log.length : 0);
    const t = setTimeout(async () => {
      try {
        const [log, ...found] = await Promise.all([api.log(0, limit, null, filter), ...shas.map((sha) => api.findCommit(sha))]);
        if (id !== seq.current) return;
        setResult({ key, log, found: found.filter((c): c is Commit => !!c), hasMore: log.length === limit });
        setError(null);
      } catch (e) {
        if (id === seq.current) setError(errorMessage(e));
      } finally {
        if (id === seq.current) setPending(false);
      }
    }, DEBOUNCE);
    return () => clearTimeout(t);
  }, [key, head]);

  const loadMore = useCallback(async () => {
    const r = current.current;
    const s = latest.current;
    if (!r || !s) return;
    const id = seq.current;
    const more = await api.log(r.log.length, PAGE, null, request(s).filter);
    if (id !== seq.current) return;
    setResult((x) => {
      if (!x) return x;
      const seen = new Set(x.log.map((c) => c.sha));
      return { ...x, log: [...x.log, ...more.filter((c) => !seen.has(c.sha))], hasMore: more.length === PAGE };
    });
  }, []);

  // The last answer stays on screen while the next search is typed and asked.
  const commits = useMemo(() => result && mergeFound(result.log, result.found), [result]);

  return { commits, hasMore: !!result?.hasMore, loadMore, pending, error };
}

/** The commits a SHA named go first, and only once. */
function mergeFound(log: Commit[], found: Commit[]) {
  const first = found.filter((c, i) => found.findIndex((x) => x.sha === c.sha) === i);
  const shas = new Set(first.map((c) => c.sha));
  return [...first, ...log.filter((c) => !shas.has(c.sha))];
}
