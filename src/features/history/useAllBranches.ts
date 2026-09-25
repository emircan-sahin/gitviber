import { useCallback, useEffect, useRef, useState } from "react";
import { api, type Commit, errorMessage, type GraphRefs, LOG_PAGE } from "@/lib/api";

// Going to a commit loads pages until it's listed, but not the whole history of a huge repo.
const MAX_REACH = 20;
const MAX_RELOADS = 3;

/**
 * Every branch's history as `refs` lets through, for as long as `on`. It's read again with
 * HEAD's (`ours`, a new list whenever the repo's refs change), keeping the pages loaded so far.
 * `reach` loads pages until a commit is listed, for going to it.
 */
export function useAllBranches(on: boolean, ours: Commit[], refs: GraphRefs) {
  const [log, setLog] = useState<{ commits: Commit[]; hasMore: boolean; error: string | null } | null>(null);
  const seq = useRef(0);
  // What's listed, ahead of the render that shows it: paging reads it between awaits.
  const current = useRef(log);
  const key = JSON.stringify(refs);
  const latest = useRef(refs);
  latest.current = refs;

  // The read in flight, for a jump to wait on when the list is read again under it.
  const loading = useRef<Promise<void>>(Promise.resolve());

  useEffect(() => {
    const id = ++seq.current;
    if (!on) {
      current.current = null;
      setLog(null);
      return;
    }
    const limit = Math.max(LOG_PAGE, current.current?.commits.length ?? 0);
    const show = (l: NonNullable<typeof log>) => {
      if (id !== seq.current) return;
      current.current = l;
      setLog(l);
    };
    loading.current = api.log(0, limit, null, null, latest.current).then(
      (commits) => show({ commits, hasMore: commits.length === limit, error: null }),
      (e) => show({ commits: [], hasMore: false, error: errorMessage(e) }),
    );
  }, [on, ours, key]);

  // Appends a page. "stale": the list was read again meanwhile, and the page was the old list's.
  const page = useCallback(async (): Promise<"more" | "done" | "stale"> => {
    const l = current.current;
    if (!l) return "stale";
    if (!l.hasMore) return "done";
    const id = seq.current;
    const more = await api.log(l.commits.length, LOG_PAGE, null, null, latest.current);
    if (id !== seq.current) return "stale";
    const seen = new Set(l.commits.map((c) => c.sha));
    const next = { ...l, commits: [...l.commits, ...more.filter((c) => !seen.has(c.sha))], hasMore: more.length === LOG_PAGE };
    current.current = next;
    setLog(next);
    return "more";
  }, []);

  const loadMore = useCallback(async () => void (await page()), [page]);

  // A reload landing mid-way (a background fetch, say) starts the pages over, not the search.
  const reach = useCallback(
    async (sha: string) => {
      const listed = () => !!current.current?.commits.some((c) => c.sha === sha);
      for (let pages = 0, reloads = 0; pages < MAX_REACH && reloads < MAX_RELOADS; ) {
        if (listed()) return true;
        const r = await page();
        if (r === "done") break;
        if (r === "stale") {
          reloads++;
          await loading.current;
        } else pages++;
      }
      return listed();
    },
    [page],
  );

  return { commits: log?.commits ?? null, hasMore: !!log?.hasMore, error: log?.error ?? null, loadMore, reach };
}
