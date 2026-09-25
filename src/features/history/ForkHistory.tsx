import { ArrowDownToLine, RefreshCw } from "lucide-react";
import { type ComponentProps, useCallback, useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Tip } from "@/components/ui/tooltip";
import { api, type Branch, type Commit, errorMessage, fullName, type GitHubAccess, github, LOG_PAGE } from "@/lib/api";
import { toast } from "@/lib/app/toast";
import { tracked, undoAction } from "@/lib/repo/undo";
import { cn } from "@/lib/utils";
import { HistoryPanel } from "./HistoryPanel";
import { RepoPanes } from "@/components/RepoPanes";
import { useGitHubAccount } from "@/features/github/shared/useGitHubAccount";

/** A fetch of the original is a network round trip: not on every switch to History. */
const REFETCH_AFTER = 5 * 60_000;
const lastFetch = new Map<string, number>();

type Props = ComponentProps<typeof HistoryPanel> & { branches: Branch[] };

/**
 * History, and for a fork a second pane under it: the original's default branch, with what
 * your branch doesn't have yet marked. Without GitHub, or for any other repo, just the history.
 */
export function ForkHistory(props: Props) {
  const { parent, origin } = useGitHubAccount();
  // Bumped by the pane's refresh button: fetch the original again.
  const [fetches, setFetches] = useState(0);
  const [loading, setLoading] = useState(false);
  if (!parent) return <HistoryPanel {...props} />;
  const forkBranch = origin?.defaultBranch ?? null;

  // GitHub's "Sync fork": origin's default branch catches up with the original, on GitHub.
  const sync = async () => {
    if (!forkBranch) return;
    setLoading(true);
    try {
      const how = await github.syncFork(forkBranch);
      toast("success", how === "none" ? `origin/${forkBranch} was already up to date` : `origin/${forkBranch} synced with ${fullName(parent.repo)}`);
      await props.refresh();
    } catch (e) {
      // 409: the fork's branch has its own commits in the way; that takes a local merge.
      toast("error", "Could not sync the fork", errorMessage(e));
    } finally {
      setLoading(false);
    }
  };
  return (
    <RepoPanes
      id="history"
      panes={[
        { id: "origin", title: "Your branch", detail: props.status?.branch ?? "", scrolls: true, children: <HistoryPanel {...props} /> },
        {
          id: "parent",
          title: "Original",
          detail: `${fullName(parent.repo)}${parent.defaultBranch ? ` · ${parent.defaultBranch}` : ""}`,
          actions: (
            <>
              {origin?.push && forkBranch && (
                <Tip label={`Sync fork: bring origin/${forkBranch} up to date with the original, on GitHub`}>
                  <Button variant="ghost" size="icon-sm" aria-label="Sync fork" disabled={loading} onClick={sync}>
                    <ArrowDownToLine />
                  </Button>
                </Tip>
              )}
              <Tip label="Fetch the original">
                <Button variant="ghost" size="icon-sm" aria-label="Fetch the original" disabled={loading} onClick={() => setFetches((n) => n + 1)}>
                  <RefreshCw className={cn(loading && "animate-spin")} />
                </Button>
              </Tip>
            </>
          ),
          scrolls: true,
          children: <OriginalHistory {...props} parent={parent} fetches={fetches} onLoading={setLoading} />,
        },
      ]}
    />
  );
}

function OriginalHistory({
  parent,
  fetches,
  onLoading,
  commits: ours,
  refresh,
  branches,
  ...props
}: Props & { parent: GitHubAccess; fetches: number; onLoading: (loading: boolean) => void }) {
  // undefined while looking, null when the repo has no remote for the original yet.
  const [remote, setRemote] = useState<string | null | undefined>(undefined);
  const [commits, setCommits] = useState<Commit[]>([]);
  const [hasMore, setHasMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [merging, setMerging] = useState(false);
  const branch = parent.defaultBranch ?? "main";
  const rev = remote ? `refs/remotes/${remote}/${branch}` : null;
  const headSha = ours[0]?.sha ?? "";

  const original = fullName(parent.repo);
  const load = useCallback(
    async (fetch: boolean) => {
      onLoading(true);
      try {
        // Offline or refused, the last fetched state still shows.
        const name = fetch
          ? await github.originalRemote(original, true).catch((e) => {
              toast("error", "Could not fetch the original", errorMessage(e));
              return github.originalRemote(original, false);
            })
          : await github.originalRemote(original, false);
        if (fetch) lastFetch.set(original, Date.now());
        setRemote(name);
        if (name) {
          const log = await api.log(0, LOG_PAGE, `refs/remotes/${name}/${branch}`);
          setCommits(log);
          setHasMore(log.length === LOG_PAGE);
        }
        setError(null);
      } catch (e) {
        setError(errorMessage(e));
      } finally {
        onLoading(false);
      }
    },
    [branch, onLoading, original],
  );

  // Fetch on the refresh button, and when shown if the last fetch is old; HEAD moving
  // only changes the marks.
  const clicks = useRef(fetches);
  useEffect(() => {
    const clicked = clicks.current !== fetches;
    clicks.current = fetches;
    load(clicked || Date.now() - (lastFetch.get(original) ?? 0) > REFETCH_AFTER);
  }, [load, fetches, headSha, original]);

  // A fetch from the terminal moves the original's branch but not HEAD: reread the log then.
  // Only a move; the first sighting is the load above.
  const tip = branches.find((b) => b.remote && b.name === `${remote}/${branch}`)?.timestamp;
  const seenTip = useRef(tip);
  useEffect(() => {
    const seen = seenTip.current;
    seenTip.current = tip;
    if (seen !== undefined && tip !== seen) load(false);
  }, [tip, load]);

  const loadMore = async () => {
    if (!rev) return;
    const more = await api.log(commits.length, LOG_PAGE, rev);
    setCommits((c) => {
      const seen = new Set(c.map((x) => x.sha));
      return [...c, ...more.filter((x) => !seen.has(x.sha))];
    });
    setHasMore(more.length === LOG_PAGE);
  };

  const add = async () => {
    setAdding(true);
    try {
      const name = await github.addOriginalRemote();
      toast("success", `Added remote ${name}`, `It points at ${original}.`);
      await load(false);
    } catch (e) {
      toast("error", "Could not add the remote", errorMessage(e));
    } finally {
      setAdding(false);
    }
  };

  // Still looking: an empty list here would read as "no commits".
  if (remote === undefined) return null;
  // The pane leaves scrolling to the history list; anything else scrolls here.
  if (remote === null) {
    return (
      <div className="h-full overflow-y-auto px-4 py-3 text-center text-[12px] text-muted-foreground">
        <div>No remote points at {original} yet.</div>
        <Button variant="secondary" size="sm" className="mt-2" disabled={adding} onClick={add}>
          {adding ? "Adding…" : "Add it as upstream"}
        </Button>
      </div>
    );
  }
  if (error && !commits.length) return <div className="h-full overflow-y-auto px-4 py-3 text-center text-[12px] text-muted-foreground">{error}</div>;
  // What the original has that this branch doesn't, as far as the loaded page shows.
  const missing = commits.filter((c) => c.notInHead).length;
  const into = props.status?.branch;
  const mergeIn = async () => {
    if (!rev || !into) return;
    setMerging(true);
    try {
      const [stopped, entry] = await tracked(() => api.merge(`${remote}/${branch}`));
      if (stopped) toast("info", "Merge stopped on conflicts", "Resolve them in Changes, then continue.");
      else toast("success", `Merged ${remote}/${branch} into ${into}`, undefined, undoAction(entry, refresh));
    } catch (e) {
      toast("error", "Merge failed", errorMessage(e));
    } finally {
      setMerging(false);
      await refresh();
      await load(false);
    }
  };
  const list = (
    <HistoryPanel
      {...props}
      commits={commits}
      hasMore={hasMore}
      loadMore={loadMore}
      refresh={async () => {
        await refresh();
        await load(false);
      }}
      headSha={headSha}
      web={`https://github.com/${original}`}
      ciTarget={original}
    />
  );
  if (!missing || !into) return list;
  return (
    <div className="flex h-full flex-col">
      <div className="flex shrink-0 items-center gap-2 border-b border-border px-3 py-1.5 text-[11.5px] text-muted-foreground">
        <span className="min-w-0 flex-1 truncate">
          {missing === commits.length && hasMore ? `${missing}+` : missing} commit{missing === 1 ? "" : "s"} not in <span className="font-mono">{into}</span>
        </span>
        <Button size="sm" variant="secondary" disabled={merging || !!props.status?.operation} onClick={mergeIn}>
          {merging ? "Merging…" : `Merge into ${into}`}
        </Button>
      </div>
      <div className="min-h-0 flex-1">{list}</div>
    </div>
  );
}
