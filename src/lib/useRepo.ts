import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import { api, type Branch, type Commit, errorMessage, type Journal, type RepoStatus, type Worktree } from "./api";
import { toast } from "./toast";

const PAGE = 200;

interface RepoChanged {
  worktree: boolean;
  git: boolean;
}

/** Loads and live-refreshes everything the workspace shows for the open repo. */
export function useRepo(root: string) {
  const [status, setStatus] = useState<RepoStatus | null>(null);
  const [commits, setCommits] = useState<Commit[]>([]);
  const [hasMore, setHasMore] = useState(false);
  const [branches, setBranches] = useState<Branch[]>([]);
  const [worktrees, setWorktrees] = useState<Worktree[]>([]);
  const [journal, setJournal] = useState<Journal | null>(null);
  // Bumped on every change on disk so open views can reload their content.
  const [revision, setRevision] = useState(0);
  const inFlight = useRef<Promise<void> | null>(null);
  const queued = useRef<{ history: boolean } | null>(null);
  // Commits shown so far: a refresh reloads all of them, or "Load more" pages would be lost.
  const loaded = useRef(0);

  const load = useCallback(async (history: boolean) => {
    const limit = Math.max(PAGE, loaded.current);
    const [st, br, log, wt, jn] = await Promise.all([
      api.status(),
      history ? api.branches() : null,
      history ? api.log(0, limit) : null,
      history ? api.worktrees() : null,
      history ? api.journal() : null,
    ]);
    setStatus(st);
    if (br) setBranches(br);
    if (wt) setWorktrees(wt);
    if (jn) setJournal(jn);
    if (log) {
      loaded.current = log.length;
      setCommits(log);
      setHasMore(log.length === limit);
    }
    setRevision((r) => r + 1);
  }, []);

  // Coalesce: if a refresh is running, remember one more and run it after.
  const refresh = useCallback(
    async (history = true) => {
      if (inFlight.current) {
        queued.current = { history: history || !!queued.current?.history };
        return inFlight.current;
      }
      const run = async (h: boolean) => {
        try {
          await load(h);
        } catch (e) {
          toast("error", "Could not read repository", errorMessage(e));
        }
        const next = queued.current;
        queued.current = null;
        if (next) await run(next.history);
      };
      inFlight.current = run(history).finally(() => {
        inFlight.current = null;
      });
      return inFlight.current;
    },
    [load],
  );

  const loadingMore = useRef(false);
  const loadMore = useCallback(async () => {
    if (loadingMore.current) return;
    loadingMore.current = true;
    try {
      const more = await api.log(commits.length, PAGE);
      // A refresh may have shifted the page boundary meanwhile; never show a commit twice.
      setCommits((c) => {
        const seen = new Set(c.map((x) => x.sha));
        const next = [...c, ...more.filter((x) => !seen.has(x.sha))];
        loaded.current = next.length;
        return next;
      });
      setHasMore(more.length === PAGE);
    } finally {
      loadingMore.current = false;
    }
  }, [commits.length]);

  useEffect(() => {
    setStatus(null);
    setCommits([]);
    setJournal(null);
    loaded.current = 0;
    refresh(true);
    const unlisten = listen<RepoChanged>("repo-changed", (e) => refresh(e.payload.git));
    return () => {
      // During hot reload the listener can already be gone; nothing to clean up then.
      unlisten.then((f) => f()).catch(() => {});
    };
  }, [root, refresh]);

  return { status, commits, hasMore, branches, worktrees, journal, revision, refresh, loadMore };
}

export type RepoData = ReturnType<typeof useRepo>;
