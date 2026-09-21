import { Cloud, Tag } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { api, type Commit, errorMessage, type FileChange } from "@/lib/api";
import { type Selection, selectionKey } from "@/lib/selection";
import { toast } from "@/lib/toast";
import { cn, relativeTime } from "@/lib/utils";
import { FileIcon } from "./FileIcon";
import { LineCounts, PathLabel, StatusLetter } from "./StatusBadge";

interface Props {
  commits: Commit[];
  /** Remote-tracking branch names (origin/main…), to group decorations. */
  remotes: Set<string>;
  hasMore: boolean;
  loadMore: () => Promise<void>;
  activeKey: string | null;
  onOpen: (s: Selection, pin?: boolean) => void;
  onHover: (s: Selection) => void;
}

export function HistoryPanel({ commits, remotes, hasMore, loadMore, activeKey, onOpen, onHover }: Props) {
  const [open, setOpen] = useState<string | null>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const anchor = useRef<{ el: HTMLElement; top: number } | null>(null);

  // Opening a commit collapses the one above it; WebKit has no scroll anchoring, so without
  // this the clicked row jumps up by the collapsed file list, often out of view.
  useLayoutEffect(() => {
    const a = anchor.current;
    anchor.current = null;
    if (a && scroller.current) scroller.current.scrollTop += a.el.getBoundingClientRect().top - a.top;
  }, [open]);

  const toggle = (sha: string, el: HTMLElement) => {
    anchor.current = { el, top: el.getBoundingClientRect().top };
    setOpen(open === sha ? null : sha);
  };

  if (!commits.length) {
    return <div className="px-6 pt-20 text-center text-[12px] text-subtle">No commits yet.</div>;
  }

  return (
    <div ref={scroller} className="h-full overflow-x-hidden overflow-y-auto py-1">
      {commits.map((c, i) => (
        <CommitRow
          key={c.sha}
          commit={c}
          remotes={remotes}
          first={i === 0}
          last={i === commits.length - 1}
          open={open === c.sha}
          onToggle={(el) => toggle(c.sha, el)}
          activeKey={activeKey}
          onOpen={onOpen}
          onHover={onHover}
        />
      ))}
      {hasMore && (
        <div className="p-2">
          <Button variant="secondary" size="sm" className="w-full" onClick={() => loadMore().catch((e) => toast("error", "Could not load history", errorMessage(e)))}>
            Load more
          </Button>
        </div>
      )}
    </div>
  );
}

function CommitRow({
  commit,
  remotes,
  first,
  last,
  open,
  onToggle,
  activeKey,
  onOpen,
  onHover,
}: {
  commit: Commit;
  remotes: Set<string>;
  first: boolean;
  last: boolean;
  open: boolean;
  onToggle: (row: HTMLElement) => void;
  activeKey: string | null;
  onOpen: (s: Selection, pin?: boolean) => void;
  onHover: (s: Selection) => void;
}) {
  const [files, setFiles] = useState<FileChange[] | null>(null);

  useEffect(() => {
    if (!open || files) return;
    let alive = true;
    api
      .commitFiles(commit.sha)
      .then((f) => {
        if (!alive) return;
        setFiles(f);
        // Jump straight into the first file so one click shows code.
        if (f[0]) onOpen({ kind: "commit", commit, file: f[0] });
      })
      .catch((e) => alive && toast("error", "Could not load commit", errorMessage(e)));
    // Collapsing (or opening another commit) cancels the auto-open of a late reply.
    return () => {
      alive = false;
    };
  }, [open, files, commit, onOpen]);

  const merge = commit.parents.length > 1;
  const add = files?.reduce((n, f) => n + (f.additions ?? 0), 0) ?? 0;
  const del = files?.reduce((n, f) => n + (f.deletions ?? 0), 0) ?? 0;

  return (
    <div className="relative">
      <div className={cn("absolute left-[15px] w-px bg-border-strong", first ? "top-3" : "top-0", last && !open ? "h-3" : "bottom-0")} />
      <div role="button" onClick={(e) => onToggle(e.currentTarget)} className={cn("relative flex cursor-pointer items-start gap-2.5 py-1.5 pr-2 pl-3", open ? "bg-active" : "hover:bg-hover")}>
        <span
          className={cn(
            "relative z-10 mt-[3px] size-[9px] shrink-0 rounded-full border-2",
            commit.unpushed ? "border-primary bg-primary" : merge ? "border-renamed bg-sidebar" : "border-subtle bg-sidebar",
          )}
          title={commit.unpushed ? "Not pushed yet" : undefined}
        />
        <div className="min-w-0 flex-1">
          <div className={cn("truncate text-[12px] leading-4", open ? "font-medium text-foreground" : "text-foreground/90")}>{commit.subject}</div>
          <div className="mt-0.5 flex min-w-0 items-center gap-1.5 text-[10.5px] text-subtle">
            <span className="min-w-0 truncate">{commit.authorName}</span>
            <span>·</span>
            <span className="shrink-0">{relativeTime(commit.timestamp)}</span>
            <span className="ml-auto shrink-0 font-mono">{commit.shortSha}</span>
          </div>
          <RefBadges refs={commit.refs} remotes={remotes} />
        </div>
      </div>
      {open && (
        <div className="relative border-y border-border bg-panel py-0.5">
          {!files && <div className="py-1 pl-8 text-[11.5px] text-subtle">Loading…</div>}
          {files && (
            <div className="flex h-6 items-center gap-2 pr-2 pl-8 text-[10.5px] text-subtle">
              <span className="font-semibold tracking-[0.08em] uppercase">Changed files</span>
              <span className="font-mono text-muted-foreground">{files.length}</span>
              <span className="ml-auto font-mono">
                <span className="text-added">+{add}</span> <span className="text-removed">-{del}</span>
              </span>
            </div>
          )}
          {files?.map((f) => {
            const sel: Selection = { kind: "commit", commit, file: f };
            const active = activeKey === selectionKey(sel);
            return (
              <div
                key={f.path}
                role="button"
                onClick={() => onOpen(sel)}
                onDoubleClick={() => onOpen(sel, true)}
                onMouseEnter={() => onHover(sel)}
                className={cn("relative flex h-[26px] cursor-pointer items-center gap-2 pr-2 pl-8 text-[12px]", active ? "bg-primary/15" : "hover:bg-hover")}
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
      )}
    </div>
  );
}

type Ref = { name: string; kind: "head" | "local" | "remote" | "tag"; synced: boolean };

/**
 * Groups decorations so they stay readable: a local branch and its remote twin on the same
 * commit (main + origin/main) become one "main ☁" badge; origin/HEAD is dropped.
 * `remotes` tells remote-tracking names apart, since local names can contain "/" too.
 */
function groupRefs(refs: string[], remotes: Set<string>): Ref[] {
  const head = refs.find((r) => r.startsWith("HEAD -> "))?.slice(8);
  const names = refs.map((r) => (r.startsWith("HEAD -> ") ? r.slice(8) : r));
  const isRemote = (r: string) => remotes.has(r) || (!remotes.size && r.startsWith("origin/"));
  const short = (r: string) => r.slice(r.indexOf("/") + 1);
  const out: Ref[] = [];
  for (const r of names) {
    if (r === "HEAD" || r.endsWith("/HEAD")) continue;
    if (r.startsWith("tag: ")) out.push({ name: r.slice(5), kind: "tag", synced: false });
    else if (isRemote(r)) {
      if (!names.includes(short(r))) out.push({ name: r, kind: "remote", synced: false });
    } else out.push({ name: r, kind: r === head ? "head" : "local", synced: names.some((x) => isRemote(x) && short(x) === r) });
  }
  return out;
}

function RefBadges({ refs, remotes }: { refs: string[]; remotes: Set<string> }) {
  const list = groupRefs(refs, remotes);
  if (!list.length) return null;
  return (
    <div className="mt-1 flex flex-wrap gap-1">
      {list.map((r) => (
        <span
          key={`${r.kind}:${r.name}`}
          title={r.synced ? `${r.name} (in sync with remote)` : r.name}
          className={cn(
            "flex max-w-full items-center gap-1 rounded-[3px] px-1.5 font-mono text-[10px] leading-[18px] font-medium",
            r.kind === "head" && "bg-primary text-white",
            r.kind === "local" && "bg-renamed/15 text-renamed",
            r.kind === "remote" && "border border-border-strong text-muted-foreground",
            r.kind === "tag" && "bg-modified/15 text-modified",
          )}
        >
          {r.kind === "tag" ? <Tag className="size-2.5 shrink-0" /> : r.kind === "remote" ? <Cloud className="size-2.5 shrink-0" /> : null}
          <span className="truncate">{r.name}</span>
          {r.synced && <Cloud className="size-2.5 shrink-0 opacity-75" />}
        </span>
      ))}
    </div>
  );
}
