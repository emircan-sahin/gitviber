import { Files } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuTrigger } from "@/components/ui/context-menu";
import { api, type Commit, errorMessage, LOG_PAGE } from "@/lib/api";
import { copyText } from "@/lib/app/clipboard";
import { relativeTime } from "@/lib/format";
import type { Selection } from "@/lib/repo/selection";
import { commitUrl } from "@/features/history/commitActions";
import { arrows } from "./arrows";

/** The commits `head` has that `base` doesn't (commit ids), a page at a time. Opening one shows its changes. */
export function CommitList({ base, head, count, empty, webUrl, onOpen }: { base: string; head: string; count: number; empty: string; webUrl: string | null; onOpen: (s: Selection, pin?: boolean) => void }) {
  const [log, setLog] = useState<{ commits: Commit[]; hasMore: boolean; error: string | null } | null>(null);
  const current = useRef(log);
  current.current = log;
  useEffect(() => {
    let live = true;
    api.logBetween(base, head, 0, LOG_PAGE).then(
      (commits) => live && setLog({ commits, hasMore: commits.length === LOG_PAGE, error: null }),
      (e) => live && setLog({ commits: [], hasMore: false, error: errorMessage(e) }),
    );
    return () => {
      live = false;
    };
  }, [base, head]);
  const [more, setMore] = useState(false);
  const loadMore = async () => {
    const l = current.current;
    if (!l || more) return;
    setMore(true);
    try {
      const next = await api.logBetween(base, head, l.commits.length, LOG_PAGE);
      setLog((x) => x && { ...x, commits: [...x.commits, ...next], hasMore: next.length === LOG_PAGE });
    } catch (e) {
      setLog((x) => x && { ...x, error: errorMessage(e) });
    } finally {
      setMore(false);
    }
  };

  if (!count) return <div className="px-4 py-6 text-center text-[12px] text-muted-foreground">{empty}</div>;
  if (!log) return <div className="px-4 py-3 text-[11.5px] text-subtle">Loading…</div>;
  if (log.error) return <div className="px-4 py-3 text-[11.5px] text-muted-foreground">{log.error}</div>;
  return (
    <div role="list" aria-label="Commits" onKeyDown={arrows} className="min-h-0 flex-1 overflow-y-auto pb-2">
      {log.commits.map((c, i) => {
        const open = (pin: boolean) => onOpen({ kind: "changes", list: "commit", commit: c, url: commitUrl(c, { webUrl, everyOnWeb: false }) }, pin);
        return (
          <ContextMenu key={c.sha}>
            <ContextMenuTrigger asChild>
              <div
                role="listitem"
                data-row={c.sha}
                tabIndex={i === 0 ? 0 : -1}
                title="Open this commit's changes"
                onClick={() => open(false)}
                onDoubleClick={() => open(true)}
                onKeyDown={(e) => e.key === "Enter" && open(true)}
                className="flex cursor-pointer items-start gap-2.5 py-1.5 pr-3 pl-4 outline-none hover:bg-hover focus-visible:bg-hover focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset"
              >
                <span className="mt-[5px] size-[9px] shrink-0 rounded-full border-2 border-subtle bg-sidebar" />
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[12px] leading-4 text-foreground/90">{c.subject}</div>
                  <div className="mt-0.5 flex min-w-0 items-center gap-1.5 text-[10.5px] text-subtle">
                    <span className="min-w-0 truncate">{c.authorName}</span>
                    <span>·</span>
                    <span className="shrink-0" title={new Date(c.committedAt * 1000).toLocaleString()}>
                      {relativeTime(c.committedAt)}
                    </span>
                    <span className="ml-auto shrink-0 font-mono">{c.shortSha}</span>
                  </div>
                </div>
              </div>
            </ContextMenuTrigger>
            <ContextMenuContent>
              <ContextMenuItem onSelect={() => open(true)}>
                <Files /> Open All Changes in Commit
              </ContextMenuItem>
              <ContextMenuItem onSelect={() => copyText(c.sha, "SHA copied")}>Copy SHA</ContextMenuItem>
            </ContextMenuContent>
          </ContextMenu>
        );
      })}
      {log.hasMore && (
        <div className="p-2">
          <Button variant="secondary" size="sm" className="w-full" disabled={more} onClick={() => void loadMore()}>
            Load more
          </Button>
        </div>
      )}
    </div>
  );
}
