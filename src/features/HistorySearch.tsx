import { Loader2, Search, X } from "lucide-react";
import { type ComponentProps, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type Commit, errorMessage } from "@/lib/api";
import { isTyping, useShortcut } from "@/lib/keybindings";
import { isEmptyFilter, parseLogQuery } from "@/lib/logQuery";
import { ForkHistory } from "./ForkHistory";
import { HistoryPanel } from "./HistoryPanel";

const PAGE = 200;
const DEBOUNCE = 250;
/** Focus requests handled so far; the search box takes focus only for a new one. */
let focused = 0;

type Props = ComponentProps<typeof ForkHistory> & {
  query: string;
  onQuery: (query: string) => void;
  /** Bumped by the search command: focus the box. */
  focusRequest: number;
};

/** History with a search box on top; while it's searching, the matches replace the full history. */
export function SearchableHistory({ query, onQuery, focusRequest, ...props }: Props) {
  const input = useRef<HTMLInputElement>(null);
  const shortcut = useShortcut("history.search");
  const active = !isEmptyFilter(parseLogQuery(query).filter);
  const search = useCommitSearch(active ? query : "", props.commits[0]?.sha);

  useEffect(() => {
    if (focusRequest === focused) return;
    focused = focusRequest;
    input.current?.focus();
    input.current?.select();
  }, [focusRequest]);

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
        {search.pending ? <Loader2 className="size-3.5 shrink-0 animate-spin text-subtle" /> : <Search className="size-3.5 shrink-0 text-subtle" />}
        <input
          ref={input}
          value={query}
          onChange={(e) => onQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key !== "Escape") return;
            e.preventDefault();
            if (query) onQuery("");
            else input.current?.blur();
          }}
          placeholder={`Search commits${shortcut ? ` (${shortcut})` : ""}  ·  author: path: code:`}
          title={"Words match the message (all of them, any case).\nauthor:name  path:src/app  code:text a commit added or removed\nA SHA or prefix finds that commit. Quotes keep spaces."}
          spellCheck={false}
          className="h-full min-w-0 flex-1 bg-transparent text-[12px] outline-none placeholder:text-subtle"
        />
        {query && (
          <button aria-label="Clear search" onClick={() => onQuery("")} className="flex size-4 shrink-0 items-center justify-center rounded-sm text-subtle hover:bg-hover hover:text-foreground">
            <X className="size-3" />
          </button>
        )}
      </div>
      <div className="min-h-0 flex-1">
        {!active ? (
          <ForkHistory {...props} />
        ) : search.error && !search.commits ? (
          <div className="px-4 py-6 text-center text-[12px] text-muted-foreground">{search.error}</div>
        ) : (
          <HistoryPanel
            {...props}
            commits={search.commits ?? []}
            hasMore={search.hasMore}
            loadMore={search.loadMore}
            // Undo and reset act on HEAD, which a list of matches needn't start with.
            headSha={props.commits[0]?.sha ?? ""}
            empty={search.commits ? "No commits match." : "Searching…"}
          />
        )}
      </div>
    </div>
  );
}

/**
 * The commits `query` matches, a page at a time, then any commit a SHA in it names on top.
 * Typing waits DEBOUNCE before asking; a reply to an older query is dropped. `head`: HEAD's
 * commit, to search again when it moves.
 */
function useCommitSearch(query: string, head: string | undefined) {
  const [result, setResult] = useState<{ query: string; log: Commit[]; found: Commit[]; hasMore: boolean } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const seq = useRef(0);
  const current = useRef(result);
  current.current = result;

  useEffect(() => {
    const id = ++seq.current;
    if (!query) {
      setResult(null);
      setPending(false);
      return;
    }
    setPending(true);
    const { filter, shas } = parseLogQuery(query);
    // A refresh of the same search keeps the pages loaded so far.
    const prev = current.current;
    const limit = Math.max(PAGE, prev?.query === query ? prev.log.length : 0);
    const t = setTimeout(async () => {
      try {
        const [log, ...found] = await Promise.all([api.log(0, limit, null, filter), ...shas.map((s) => api.findCommit(s))]);
        if (id !== seq.current) return;
        setResult({ query, log, found: found.filter((c): c is Commit => !!c), hasMore: log.length === limit });
        setError(null);
      } catch (e) {
        if (id === seq.current) setError(errorMessage(e));
      } finally {
        if (id === seq.current) setPending(false);
      }
    }, DEBOUNCE);
    return () => clearTimeout(t);
  }, [query, head]);

  const loadMore = useCallback(async () => {
    const r = current.current;
    if (!r) return;
    const id = seq.current;
    const more = await api.log(r.log.length, PAGE, null, parseLogQuery(r.query).filter);
    if (id !== seq.current) return;
    setResult((x) => {
      if (!x) return x;
      const seen = new Set(x.log.map((c) => c.sha));
      return { ...x, log: [...x.log, ...more.filter((c) => !seen.has(c.sha))], hasMore: more.length === PAGE };
    });
  }, []);

  // The last answer stays on screen while the next query is typed and asked.
  const commits = useMemo(() => result && mergeFound(result.log, result.found), [result]);

  return { commits, hasMore: !!result?.hasMore, loadMore, pending, error };
}

/** The commits a SHA named go first, and only once. */
function mergeFound(log: Commit[], found: Commit[]) {
  const first = found.filter((c, i) => found.findIndex((x) => x.sha === c.sha) === i);
  const shas = new Set(first.map((c) => c.sha));
  return [...first, ...log.filter((c) => !shas.has(c.sha))];
}
