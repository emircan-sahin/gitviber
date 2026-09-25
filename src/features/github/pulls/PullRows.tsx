import { FolderGit2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ContextMenuItem, ContextMenuSeparator } from "@/components/ui/context-menu";
import { type GitHubAccount, PR_PAGE, type Pull, type Target } from "@/lib/api";
import { CiBadge } from "@/components/CiBadge";
import { useCi } from "@/lib/github/ci";
import { type Selection, selectionKey } from "@/lib/repo/selection";
import { isoToUnix, relativeTime } from "@/lib/format";
import { openWorktreeDialog } from "@/features/worktrees/WorktreeDialogs";
import { useListNav } from "@/lib/ui/useListNav";
import { cn } from "@/lib/utils";
import type { Filter } from "@/features/github/shared/FilterTabs";
import { LinkMenu } from "@/features/github/shared/LinkMenu";
import { PullStateIcon } from "@/features/github/shared/StateBadges";
import { pullSource } from "./pullSource";
import { EmptyNote, ListError } from "@/features/github/shared/ListNotes";

/** github/client.rs reads at most this many pages. */
const MAX_PAGES = 30;

export function PullRows({
  pulls: loaded,
  match,
  error,
  filter,
  activeKey,
  onOpen,
  account,
  roomy,
  shown,
  loading,
  onMore,
  target,
}: {
  pulls: Pull[] | null;
  /** The list filter's test, while it has text. */
  match: ((p: Pull) => boolean) | null;
  error: string | null;
  filter: Filter;
  activeKey: string | null;
  onOpen: (s: Selection, pin?: boolean) => void;
  /** For where a PR would be checked out; null until it loads. */
  account: GitHubAccount | null;
  /** The whole panel, not a pane: the empty note sits lower. */
  roomy: boolean;
  /** Pages in `pulls`; a full last page means there may be more. */
  shown: number;
  loading: boolean;
  onMore: () => void;
  /** The repository the PRs are on, for their checks. */
  target: Target;
}) {
  const full = loaded?.length === shown * PR_PAGE;
  const pulls = match ? (loaded?.filter(match) ?? null) : loaded;
  const ci = useCi(target, (loaded ?? []).filter((p) => p.state === "open").map((p) => p.headSha));
  const nav = useListNav({ activeKey });
  return (
    <>
      <ListError error={error} loaded={!!loaded} />
      {pulls?.length === 0 && (
        <EmptyNote roomy={roomy}>
          {match && loaded?.length ? `None of the ${loaded?.length} loaded pull requests match.` : `No ${filter === "all" ? "" : filter} pull requests.`}
        </EmptyNote>
      )}
      <div role="listbox" aria-label="Pull requests" {...nav}>
      {pulls?.map((p) => {
        const sel: Selection = { kind: "pull", pull: p };
        const key = selectionKey(sel);
        const active = activeKey === key;
        const source = p.state === "open" ? pullSource(p, account) : null;
        const extra = source && (
          <>
            <ContextMenuItem onSelect={() => openWorktreeDialog({ kind: "new", pull: source })}>
              <FolderGit2 /> Check out in new worktree…
            </ContextMenuItem>
            <ContextMenuSeparator />
          </>
        );
        return (
          <LinkMenu key={p.number} url={p.url} extra={extra}>
          <div
            role="option"
            aria-selected={active}
            tabIndex={-1}
            data-row={key}
            onClick={() => onOpen(sel)}
            onDoubleClick={() => onOpen(sel, true)}
            className={cn("relative flex cursor-pointer gap-2.5 py-1.5 pr-2 pl-3 outline-none focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset", active ? "bg-primary/15" : "hover:bg-hover focus:bg-hover")}
          >
            {active && <span className="absolute inset-y-0 left-0 w-0.5 bg-primary" />}
            <PullStateIcon pull={p} className="mt-0.5" />
            <div className="min-w-0 flex-1">
              <div className="truncate text-[12px] leading-4 text-foreground/90">{p.title}</div>
              <div className="mt-0.5 flex items-center gap-1.5 text-[10.5px] text-subtle">
                <span className="font-mono">#{p.number}</span>
                <span>·</span>
                <span className="truncate">{p.author}</span>
                <span>·</span>
                <span className="min-w-0 truncate font-mono">{p.headRef}</span>
                <CiBadge state={p.state === "open" ? ci[p.headSha] : undefined} className="ml-auto" />
                <span className={cn("shrink-0", !(p.state === "open" && ci[p.headSha]) && "ml-auto")}>{relativeTime(isoToUnix(p.updatedAt))}</span>
              </div>
            </div>
          </div>
          </LinkMenu>
        );
      })}
      </div>
      {full && shown < MAX_PAGES && (
        <div className="p-2">
          <Button variant="secondary" size="sm" className="w-full" disabled={loading} onClick={onMore}>
            {loading ? "Loading…" : "Load more"}
          </Button>
        </div>
      )}
      {/* Past client.rs's cap: say so rather than look complete. */}
      {full && shown >= MAX_PAGES && <div className="px-4 py-2 text-center text-[11px] text-subtle">Showing the {loaded.length} most recently updated</div>}
    </>
  );
}
