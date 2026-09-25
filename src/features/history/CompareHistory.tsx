import { GitCompareArrows, X } from "lucide-react";
import { type ComponentProps, useEffect, useRef, useState } from "react";
import { api, type Commit, errorMessage, type FileChange, LOG_PAGE } from "@/lib/api";
import { type Selection, selectionKey } from "@/lib/repo/selection";
import { shortRef } from "@/lib/git/refs";
import { FileIcon } from "@/components/FileIcon";
import { LineCounts, PathLabel, StatusLetter } from "@/components/StatusBadge";
import { RepoPanes } from "@/components/RepoPanes";
import { cn } from "@/lib/utils";
import { useAsyncValue } from "@/hooks/useAsyncValue";
import { HistoryPanel } from "./HistoryPanel";

type ListProps = Omit<ComponentProps<typeof HistoryPanel>, "commits" | "hasMore" | "loadMore">;

/**
 * GitHub Desktop's Compare: what `with` (a full ref) has that HEAD doesn't, and the other way
 * round, each its own list. Read again whenever HEAD's history changes.
 */
export function CompareHistory({ with: ref, current, ours, onClose, ...props }: ListProps & { with: string; current: string; ours: Commit[]; onClose: () => void }) {
  const counts = useAsyncValue(() => api.compareCounts(ref).catch(() => null), [ref, ours], null);
  const name = shortRef(ref);
  const list = (incoming: boolean) => <CompareList key={`${ref}:${incoming}`} {...props} with={ref} incoming={incoming} ours={ours} />;

  return (
    <div className="flex h-full flex-col">
      <div className="flex shrink-0 items-center gap-1.5 border-b border-border px-2.5 py-1 text-[11px] text-muted-foreground">
        <GitCompareArrows className="size-3 shrink-0 text-subtle" />
        <span className="min-w-0 truncate">
          <span className="font-mono text-foreground">{current}</span> compared with <span className="font-mono text-foreground">{name}</span>
        </span>
        <button
          aria-label="Stop comparing"
          onClick={onClose}
          className="ml-auto flex size-4 shrink-0 items-center justify-center rounded-sm text-subtle hover:bg-hover hover:text-foreground focus-visible:bg-hover focus-visible:text-foreground"
        >
          <X className="size-3" />
        </button>
      </div>
      <div className="min-h-0 flex-1">
        <RepoPanes
          id="compare"
          panes={[
            { id: "incoming", title: "Behind", detail: `in ${name}, not ${current}`, badge: counts?.[1], scrolls: true, children: list(true) },
            { id: "outgoing", title: "Ahead", detail: `in ${current}, not ${name}`, badge: counts?.[0], scrolls: true, children: list(false) },
            { id: "files", title: "Files", detail: `what ${name} changed since they parted`, scrolls: true, children: <CompareFiles with={ref} ours={ours} activeKey={props.activeKey} onOpen={props.onOpen} /> },
          ]}
        />
      </div>
    </div>
  );
}

/** The files `with` changed since it and HEAD parted: a pull request of it, file by file. */
function CompareFiles({ with: ref, ours, activeKey, onOpen }: { with: string; ours: Commit[]; activeKey: string | null; onOpen: (s: Selection, pin?: boolean) => void }) {
  const found = useAsyncValue<{ base: string; head: string; files: FileChange[] } | { error: string } | null>(
    () => api.compareFiles(ref).catch((e) => ({ error: errorMessage(e) })),
    [ref, ours],
    null,
  );
  if (!found) return <div className="py-1 pl-4 text-[11.5px] text-subtle">Loading…</div>;
  if ("error" in found) return <div className="px-4 py-2 text-[11.5px] text-muted-foreground">{found.error}</div>;
  if (!found.files.length) return <div className="py-1 pl-4 text-[11.5px] text-subtle">No changes</div>;
  const range = { label: shortRef(ref), base: found.base, head: found.head };
  return (
    <div role="listbox" aria-label="Changed files">
      {found.files.map((f) => {
        const sel: Selection = { kind: "pr-file", range, file: f };
        const active = activeKey === selectionKey(sel);
        return (
          <div
            key={f.path}
            role="option"
            aria-selected={active}
            tabIndex={-1}
            onClick={() => onOpen(sel)}
            onDoubleClick={() => onOpen(sel, true)}
            className={cn(
              "relative flex h-[26px] cursor-pointer items-center gap-2 pr-2 pl-4 text-[12px] outline-none focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset",
              active ? "bg-primary/15" : "hover:bg-hover focus:bg-hover",
            )}
          >
            {active && <span className="absolute inset-y-0 left-0 w-0.5 bg-primary" />}
            <FileIcon path={f.path} />
            <PathLabel path={f.path} className="flex-1" />
            <LineCounts file={f} />
            <StatusLetter status={f.status} />
          </div>
        );
      })}
    </div>
  );
}

function CompareList({ with: ref, incoming, ours, ...props }: ListProps & { with: string; incoming: boolean; ours: Commit[] }) {
  const [log, setLog] = useState<{ commits: Commit[]; hasMore: boolean; error: string | null } | null>(null);
  const current = useRef(log);
  current.current = log;
  // A reread finishing during Load more: its page was fetched at the old list's offset.
  const seq = useRef(0);

  useEffect(() => {
    const id = ++seq.current;
    const limit = Math.max(LOG_PAGE, current.current?.commits.length ?? 0);
    api.logCompare(ref, incoming, 0, limit).then(
      (commits) => id === seq.current && setLog({ commits, hasMore: commits.length === limit, error: null }),
      (e) => id === seq.current && setLog({ commits: [], hasMore: false, error: errorMessage(e) }),
    );
  }, [ref, incoming, ours]);

  const loadMore = async () => {
    const l = current.current;
    if (!l) return;
    const id = seq.current;
    const more = await api.logCompare(ref, incoming, l.commits.length, LOG_PAGE);
    if (id !== seq.current) return;
    setLog((x) => {
      if (!x) return x;
      const seen = new Set(x.commits.map((c) => c.sha));
      return { ...x, commits: [...x.commits, ...more.filter((c) => !seen.has(c.sha))], hasMore: more.length === LOG_PAGE };
    });
  };

  if (log?.error) return <div className="px-4 py-6 text-center text-[12px] text-muted-foreground">{log.error}</div>;
  return (
    <HistoryPanel
      {...props}
      commits={log?.commits ?? []}
      hasMore={!!log?.hasMore}
      loadMore={loadMore}
      empty={log ? (incoming ? "Nothing here that your branch lacks." : "Nothing here that it lacks.") : "Loading…"}
      graph={false}
    />
  );
}
