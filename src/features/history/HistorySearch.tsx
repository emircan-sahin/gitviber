import { CircleHelp, FileClock, FolderClock, Loader2, Search, X } from "lucide-react";
import { type ComponentProps, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Tip } from "@/components/ui/tooltip";
import { api, type Commit, errorMessage, type GraphRefs, LOG_PAGE } from "@/lib/api";
import { useFind } from "@/lib/ui/find";
import { isTyping, matchesCommand, useShortcut } from "@/lib/commands/keybindings";
import { isEmptyFilter, parseLogQuery } from "@/lib/git/logQuery";
import { toast } from "@/lib/app/toast";
import { BisectBar } from "./BisectBar";
import { ForkHistory } from "./ForkHistory";
import { CompareHistory } from "./CompareHistory";
import { GraphMenu, GraphNotice } from "./GraphMenu";
import { useAllBranches } from "./useAllBranches";
import { hideRefs, useAllBranchesSetting, useGraphRefs } from "./useGraphRefs";
import { HistoryPanel } from "./HistoryPanel";
import type { RefMenu } from "./commitActions";
import type { Reveal } from "./CommitRow";

const SYNTAX = "Words match the message (all of them, any case).\nauthor:name  path:src/app  code:text a commit added or removed\nA SHA or prefix finds that commit. Quotes keep spaces.";

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
  /** The main worktree: the graph's hidden branches are the repository's, across its worktrees. */
  main: string;
};

/** History with a search box on top; while it's searching, the matches replace the full history. */
export function SearchableHistory({ search, onSearch, focusRequested, onFocused, main, ...props }: Props) {
  const { query, scope, reveal } = search;
  const input = useRef<HTMLInputElement>(null);
  const shortcut = useShortcut("history.search");
  const active = !!scope || !isEmptyFilter(parseLogQuery(query).filter);
  const head = props.commits[0]?.sha ?? "";
  const [allBranches, setAllBranches] = useAllBranchesSetting();
  const [refs, setRefs] = useGraphRefs(main);
  // A branch's full ref; a search still goes first, and closing it comes back here.
  const [compare, setCompare] = useState<string | null>(null);
  const showAll = allBranches && !active && !compare;
  const found = useCommitSearch(active ? search : null, head, allBranches ? refs : null);
  const all = useAllBranches(showAll, props.commits, refs);
  const setQuery = (q: string) => onSearch({ ...search, query: q, reveal: null });

  const [jump, setJump] = useState<string | null>(null);
  const jumped = useCallback(() => setJump(null), []);
  const goTo = async (sha: string, name: string) => {
    if (await all.reach(sha)) setJump(sha);
    else toast("info", `${name} isn't in the graph`, "It's hidden, or further back than the graph goes.");
  };
  const refMenu: RefMenu = { hide: (r) => setRefs(hideRefs(refs, r)), only: (r) => setRefs({ ...refs, only: r }) };
  useFind("git", () => {
    input.current?.focus();
    input.current?.select();
  });

  useEffect(() => {
    if (!focusRequested) return;
    input.current?.focus();
    input.current?.select();
    onFocused();
  }, [focusRequested, onFocused]);

  const ScopeIcon = scope?.file ? FileClock : FolderClock;
  return (
    <div
      // Takes focus from clicks in the list, so `/` (history.find) can reach the search box from there.
      tabIndex={-1}
      onKeyDown={(e) => {
        if (!matchesCommand("history.find", e.nativeEvent) || isTyping(e.nativeEvent)) return;
        e.preventDefault();
        input.current?.focus();
      }}
      className="flex h-full flex-col outline-none"
    >
      {props.status?.operation?.kind === "bisect" && <BisectBar refresh={props.refresh} />}
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
          spellCheck={false}
          className="h-full min-w-0 flex-1 bg-transparent text-[12px] outline-none placeholder:text-subtle"
        />
        {/* Not the input's title: that tooltip never shows to the keyboard. */}
        <Tip label={SYNTAX}>
          <button aria-label={`Search syntax: ${SYNTAX}`} className="flex size-4 shrink-0 items-center justify-center rounded-sm text-subtle outline-none hover:text-foreground focus-visible:text-foreground focus-visible:ring-1 focus-visible:ring-ring">
            <CircleHelp className="size-3" />
          </button>
        </Tip>
        {query && (
          <button aria-label="Clear search" onClick={() => setQuery("")} className="flex size-4 shrink-0 items-center justify-center rounded-sm text-subtle hover:bg-hover focus-visible:bg-hover hover:text-foreground focus-visible:text-foreground">
            <X className="size-3" />
          </button>
        )}
        <GraphMenu
          all={allBranches}
          setAll={setAllBranches}
          refs={refs}
          setRefs={setRefs}
          branches={props.branches}
          current={props.status?.branch ?? null}
          // Only the graph's own list loads pages to go to; a search or a comparison lists others.
          onGoToHead={showAll ? () => goTo(head, "HEAD") : null}
          onGoTo={showAll ? goTo : null}
          onCompare={setCompare}
        />
      </div>
      {showAll && <GraphNotice refs={refs} setRefs={setRefs} />}
      {scope && (
        <div className="flex shrink-0 items-center gap-1 border-b border-border px-2 py-1.5">
          <span className="inline-flex min-w-0 items-center gap-1 rounded-full border border-border-strong bg-active pr-0.5 pl-1.5 text-[10.5px] leading-4" title={scope.path}>
            <ScopeIcon className="size-3 shrink-0 text-subtle" />
            <span className="truncate font-mono">{scope.path}</span>
            <button
              aria-label="Show all history"
              onClick={() => onSearch({ ...search, scope: null })}
              className="flex size-3.5 shrink-0 items-center justify-center rounded-full text-subtle hover:bg-hover focus-visible:bg-hover hover:text-foreground focus-visible:text-foreground"
            >
              <X className="size-2.5" />
            </button>
          </span>
        </div>
      )}
      <div className="min-h-0 flex-1">
        {active ? (
          found.error && !found.commits ? (
            <div className="px-4 py-6 text-center text-[12px] text-muted-foreground">{found.error}</div>
          ) : (
            <HistoryPanel
              {...props}
              commits={found.commits ?? []}
              hasMore={found.hasMore}
              loadMore={found.loadMore}
              // Undo and reset act on HEAD, which a list of matches needn't start with.
              headSha={head}
              empty={found.commits ? "No commits match." : "Searching…"}
              graph={false}
              reveal={reveal}
            />
          )
        ) : compare ? (
          <CompareHistory {...props} with={compare} current={props.status?.branch ?? "HEAD"} ours={props.commits} headSha={head} onClose={() => setCompare(null)} />
        ) : !allBranches ? (
          <ForkHistory {...props} />
        ) : all.error && !all.commits?.length ? (
          <div className="px-4 py-6 text-center text-[12px] text-muted-foreground">{all.error}</div>
        ) : (
          <HistoryPanel
            {...props}
            commits={all.commits ?? []}
            hasMore={all.hasMore}
            loadMore={all.loadMore}
            // The newest branch goes first, not necessarily HEAD's.
            headSha={head}
            pinHead
            jump={jump}
            onJumped={jumped}
            refMenu={refMenu}
            showRefs={refs}
            empty={all.commits ? "No commits yet." : "Loading…"}
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
 * commit, to search again when it moves. `all`: search the branches the graph shows, not only HEAD's.
 */
function useCommitSearch(search: HistorySearch | null, head: string | undefined, all: GraphRefs | null) {
  const [result, setResult] = useState<{ key: string; all: GraphRefs | null; log: Commit[]; found: Commit[]; hasMore: boolean } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const seq = useRef(0);
  const current = useRef(result);
  current.current = result;
  const key = search && JSON.stringify([search.query, search.scope, all]);
  const latest = useRef({ search, all });
  latest.current = { search, all };

  useEffect(() => {
    const id = ++seq.current;
    const { search: s, all: refs } = latest.current;
    if (!key || !s) {
      setResult(null);
      setPending(false);
      return;
    }
    setPending(true);
    const { filter, shas } = request(s);
    // A refresh of the same search keeps the pages loaded so far.
    const prev = current.current;
    const limit = Math.max(LOG_PAGE, prev?.key === key ? prev.log.length : 0);
    const t = setTimeout(async () => {
      try {
        const [log, ...found] = await Promise.all([api.log(0, limit, null, filter, refs), ...shas.map((sha) => api.findCommit(sha))]);
        if (id !== seq.current) return;
        setResult({ key, all: refs, log, found: found.filter((c): c is Commit => !!c), hasMore: log.length === limit });
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
    const s = latest.current.search;
    if (!r || !s) return;
    const id = seq.current;
    const more = await api.log(r.log.length, LOG_PAGE, null, request(s).filter, r.all);
    if (id !== seq.current) return;
    setResult((x) => {
      if (!x) return x;
      const seen = new Set(x.log.map((c) => c.sha));
      return { ...x, log: [...x.log, ...more.filter((c) => !seen.has(c.sha))], hasMore: more.length === LOG_PAGE };
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
