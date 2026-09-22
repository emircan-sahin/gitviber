import { RefreshCw } from "lucide-react";
import { type ComponentProps, useCallback, useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Tip } from "@/components/ui/tooltip";
import { api, type Commit, errorMessage, fullName, type GitHubAccess, github } from "@/lib/api";
import { useGitHubData } from "@/lib/githubCache";
import { toast } from "@/lib/toast";
import { cn } from "@/lib/utils";
import { HistoryPanel } from "./HistoryPanel";
import { RepoPanes } from "./RepoPanes";

const PAGE = 200;
/** A fetch of the original is a network round trip: not on every switch to History. */
const REFETCH_AFTER = 5 * 60_000;
const lastFetch = new Map<string, number>();

type Props = ComponentProps<typeof HistoryPanel>;

/**
 * History, and for a fork a second pane under it: the original's default branch, with what
 * your branch doesn't have yet marked. Without GitHub, or for any other repo, just the history.
 */
export function ForkHistory(props: Props) {
  // Same cache entry as the PRs panel's.
  const parent = useGitHubData("account", github.account, 600_000).data?.parent ?? null;
  // Bumped by the pane's refresh button: fetch the original again.
  const [fetches, setFetches] = useState(0);
  const [loading, setLoading] = useState(false);
  if (!parent) return <HistoryPanel {...props} />;
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
            <Tip label="Fetch the original">
              <Button variant="ghost" size="icon-sm" aria-label="Fetch the original" disabled={loading} onClick={() => setFetches((n) => n + 1)}>
                <RefreshCw className={cn(loading && "animate-spin")} />
              </Button>
            </Tip>
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
  ...props
}: Props & { parent: GitHubAccess; fetches: number; onLoading: (loading: boolean) => void }) {
  // undefined while looking, null when the repo has no remote for the original yet.
  const [remote, setRemote] = useState<string | null | undefined>(undefined);
  const [commits, setCommits] = useState<Commit[]>([]);
  const [hasMore, setHasMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
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
          const log = await api.log(0, PAGE, `refs/remotes/${name}/${branch}`);
          setCommits(log);
          setHasMore(log.length === PAGE);
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

  const loadMore = async () => {
    if (!rev) return;
    const more = await api.log(commits.length, PAGE, rev);
    setCommits((c) => {
      const seen = new Set(c.map((x) => x.sha));
      return [...c, ...more.filter((x) => !seen.has(x.sha))];
    });
    setHasMore(more.length === PAGE);
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
  return (
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
    />
  );
}
