import { Loader2 } from "lucide-react";
import { CiBadge, ciLabel } from "@/components/CiBadge";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Tip } from "@/components/ui/tooltip";
import { type Commit, errorMessage } from "@/lib/api";
import { commitPlace } from "@/lib/github/checkFailure";
import { commitSummary } from "@/lib/github/checks";
import { useCommitChecks } from "@/lib/github/commitChecks";
import { repoOfCommitUrl } from "@/lib/github/permalink";
import { CheckRows } from "@/features/github/shared/Checks";

/**
 * The commit's CI in its header: the verdict and, where the bar has room, the count; a click lists
 * every check. Nothing for a commit without checks, or without a GitHub account that can see them.
 * `url`: the commit's GitHub page; `web`: origin's.
 */
export function CommitChecks({ commit, url, web }: { commit: Commit; url: string; web: string | null }) {
  const { state, data, error } = useCommitChecks(commit.sha, url, web);
  if (!state) return null;
  const summary = commitSummary(data, ciLabel(state));
  return (
    <Popover>
      <Tip label={`Checks: ${summary}`}>
        <PopoverTrigger asChild>
          <button
            aria-label={`Checks: ${summary}`}
            className="flex min-w-0 items-center gap-1 rounded-sm outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring data-[state=open]:text-foreground"
          >
            <CiBadge state={state} bare className="size-3.5" />
            {/* Dropped first when the bar narrows: the sha and the buttons stay. */}
            <span className="hidden max-w-60 truncate @3xl:inline">{summary}</span>
          </button>
        </PopoverTrigger>
      </Tip>
      <PopoverContent align="end" className="flex w-[min(36rem,calc(100vw-2rem))] flex-col overflow-hidden">
        <div className="flex h-8 shrink-0 items-center border-b border-border px-3 text-[10.5px] font-semibold tracking-[0.08em] text-subtle uppercase">
          Checks
          <span className="ml-auto font-mono tracking-normal normal-case">{summary}</span>
        </div>
        <div className="min-h-0 overflow-auto py-1">
          {data ? (
            <>
              {!data.checks.length && !data.checksError && <div className="px-3 py-2 text-[12px] text-subtle">GitHub lists no checks on this commit.</div>}
              <CheckRows ci={data} home={{ url: repoOfCommitUrl(url), number: null }} where={commitPlace(commit.shortSha, commit.subject, url)} />
            </>
          ) : error !== undefined ? (
            <div className="px-3 py-2 text-[12px] text-removed">Could not read the checks: {errorMessage(error)}</div>
          ) : (
            <div className="flex items-center gap-2 px-3 py-2 text-[12px] text-subtle">
              <Loader2 className="size-3.5 animate-spin" /> Reading the checks…
            </div>
          )}
        </div>
      </PopoverContent>
    </Popover>
  );
}
