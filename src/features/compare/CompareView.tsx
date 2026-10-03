// Two points of history against each other, as a screen of its own: the commits one has that the
// other lacks, the files between them (one scroll of diffs, or a file at a time), and merging.
import { ArrowLeftRight, ChevronDown, ExternalLink, Files, GitMerge } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuTrigger } from "@/components/ui/context-menu";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Segmented } from "@/components/ui/segmented";
import { Tip } from "@/components/ui/tooltip";
import { FileIcon } from "@/components/FileIcon";
import { LineCounts, PathLabel, StatusLetter } from "@/components/StatusBadge";
import { Windowed } from "@/components/Windowed";
import { api, type Branch, type Commit, type Comparison, errorMessage, LOG_PAGE, type RepoStatus } from "@/lib/api";
import { copyText } from "@/lib/app/clipboard";
import { githubCompareUrl, mergeName } from "@/lib/git/comparePoints";
import { openOnGitHub } from "@/lib/github/url";
import { plural, relativeTime } from "@/lib/format";
import { compareLabel, type ComparePoint, type Selection } from "@/lib/repo/selection";
import { cn } from "@/lib/utils";
import { useAsyncValue } from "@/hooks/useAsyncValue";
import { useGitAction } from "@/hooks/useGitAction";
import { sumLines } from "@/features/changes/changeList";
import { commitUrl } from "@/features/history/commitActions";
import { mergeInto } from "@/features/topbar/useRepoActions";
import { PointPicker } from "./PointPicker";

type CompareSelection = Extract<Selection, { kind: "compare" }>;
type Loaded = { key: string; comparison: Comparison } | { key: string; error: string };

const ROW = 26;

interface Props {
  sel: CompareSelection;
  branches: Branch[];
  status: RepoStatus | null;
  /** origin's page on GitHub; null: not on GitHub. */
  webUrl: string | null;
  /** The screen's own tab, to be shown with other points. */
  onChange: (sel: CompareSelection) => void;
  onOpen: (s: Selection, pin?: boolean) => void;
  refresh: () => unknown;
}

export function CompareView({ sel, branches, status, webUrl, onChange, onOpen, refresh }: Props) {
  const { base, head, mergeBase } = sel;
  const key = `${base.ref}\0${head.ref}\0${mergeBase}`;
  // Where the branches stand: a fetch or a commit moves them, and the comparison is read again.
  const tips = `${status?.head}\0${branches.map((b) => `${b.name}:${b.sha}`).join(",")}`;
  const read = useAsyncValue<Loaded | null>(
    () =>
      api.compare(base.ref, head.ref, mergeBase).then(
        (comparison): Loaded => ({ key, comparison }),
        (e): Loaded => ({ key, error: errorMessage(e) }),
      ),
    [key, tips],
    null,
  );
  const shown = read?.key === key ? read : null;
  const comparison = shown && "comparison" in shown ? shown.comparison : null;
  const set = (next: Partial<CompareSelection>) => onChange({ ...sel, ...next });
  const current = status?.branch ?? null;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex min-h-9 shrink-0 flex-wrap items-center gap-x-2 gap-y-1 border-b border-border py-1 pr-2 pl-3 text-[12px]">
        <PointPicker side="Base" value={base} branches={branches} current={current} onPick={(p) => set({ base: p })} />
        <Tip label="Swap sides">
          <Button variant="ghost" size="icon-sm" aria-label="Swap sides" onClick={() => set({ base: head, head: base })}>
            <ArrowLeftRight />
          </Button>
        </Tip>
        <PointPicker side="Compare" value={head} branches={branches} current={current} onPick={(p) => set({ head: p })} />
        <div className="ml-auto">
          <Tip label={mergeBase ? "Only what Compare changed since the two parted, as a pull request shows it. Switch to Direct for every difference between them." : "Every difference between the two as they stand now, which also undoes what Base gained since they parted. Switch to Merge base for a pull request's view."}>
            <div>
              <Segmented
                value={mergeBase ? "merge-base" : "direct"}
                onChange={(v) => set({ mergeBase: v === "merge-base" })}
                options={[
                  { value: "merge-base", label: "Merge base" },
                  { value: "direct", label: "Direct" },
                ]}
              />
            </div>
          </Tip>
        </div>
      </div>
      {!shown ? (
        <Centered>Loading…</Centered>
      ) : "error" in shown ? (
        <Centered>{shown.error}</Centered>
      ) : (
        <Result sel={sel} comparison={shown.comparison} tips={tips} status={status} webUrl={webUrl} onOpen={onOpen} refresh={refresh} />
      )}
      {/* Kept for the screen reader while a new comparison loads. */}
      <span className="sr-only" aria-live="polite">
        {comparison ? `${plural(comparison.ahead, "commit")} ahead, ${plural(comparison.behind, "commit")} behind, ${plural(comparison.files.length, "file")} changed` : ""}
      </span>
    </div>
  );
}

function Centered({ children }: { children: React.ReactNode }) {
  return <div className="flex min-h-0 flex-1 items-center justify-center p-6 text-center text-[12.5px] text-muted-foreground">{children}</div>;
}

function Result({ sel, comparison: c, tips, status, webUrl, onOpen, refresh }: { sel: CompareSelection; comparison: Comparison; tips: string; status: RepoStatus | null; webUrl: string | null; onOpen: Props["onOpen"]; refresh: () => unknown }) {
  const { base, head, mergeBase } = sel;
  const label = compareLabel(sel);
  // What opening the files all at once, or one, shows: from the merge base, or from Base itself.
  const range = useMemo(() => ({ label, base: c.from, head: c.head }), [label, c.from, c.head]);
  const { add, del } = sumLines(c.files);
  const url = githubCompareUrl(webUrl, base, head, mergeBase);

  if (c.base === c.head) {
    return (
      <Centered>
        <div>
          <div className="text-foreground">
            <b className="font-mono">{head.label}</b> and <b className="font-mono">{base.label}</b> are at the same commit.
          </div>
          <div className="mt-1 text-subtle">Pick two points that differ to compare them.</div>
        </div>
      </Centered>
    );
  }

  return (
    <>
      <div className="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1.5 border-b border-border px-3 py-2 text-[12px]">
        <div className="min-w-0 text-muted-foreground">
          <span className="font-mono text-foreground">{head.label}</span> is{" "}
          {c.ahead || c.behind ? (
            <>
              {c.ahead > 0 && <b className="font-semibold text-foreground">{c.ahead} ahead</b>}
              {c.ahead > 0 && c.behind > 0 && " and "}
              {c.behind > 0 && <b className="font-semibold text-foreground">{c.behind} behind</b>}
            </>
          ) : (
            <b className="font-semibold text-foreground">up to date with</b>
          )}{" "}
          <span className="font-mono text-foreground">{base.label}</span>
          {c.files.length > 0 && (
            <>
              <span className="mx-1.5 text-subtle">·</span>
              {plural(c.files.length, "file")} <LineCounts file={{ additions: add, deletions: del }} />
            </>
          )}
        </div>
        <div className="ml-auto flex items-center gap-1.5">
          {url && (
            <Button variant="ghost" size="sm" onClick={() => openOnGitHub(url)}>
              <ExternalLink /> Open on GitHub
            </Button>
          )}
          <Button variant="secondary" size="sm" disabled={!c.files.length} onClick={() => onOpen({ kind: "changes", list: "range", range }, true)}>
            <Files /> Open All
          </Button>
        </div>
        {c.unrelated && <div className="basis-full text-[11.5px] text-modified">These have no commit in common, so what's listed is every difference between them.</div>}
        <MergeBar head={head} headSha={c.head} tips={tips} status={status} refresh={refresh} />
      </div>
      <Panes sel={sel} comparison={c} range={range} webUrl={webUrl} onOpen={onOpen} />
    </>
  );
}

/** Merging Compare into the checked-out branch: how much comes in, and what would conflict, found before it's done. */
function MergeBar({ head, headSha, tips, status, refresh }: { head: ComparePoint; headSha: string; tips: string; status: RepoStatus | null; refresh: () => unknown }) {
  const into = status?.branch;
  // Not into itself, nor while a merge or rebase waits on its own conflicts.
  const asks = !!into && !!status?.head && status.head !== headSha && !status.operation;
  const check = useAsyncValue(asks ? () => api.mergeCheck(head.ref).then((c) => ({ ref: head.ref, sha: headSha, c }), () => null) : null, [head.ref, headSha, tips, asks], null);
  const { busy, run } = useGitAction({ refresh });
  if (!asks || check?.sha !== headSha || !check.c.incoming) return null;
  const { incoming, unrelated, conflicts } = check.c;
  const go = (how: "ff" | "no-ff" | "squash") => void mergeInto(run, mergeName(head), how);
  const list = conflicts?.slice(0, 3).join(", ");
  return (
    <div className="flex basis-full flex-wrap items-center gap-2 text-[11.5px]">
      <div className="inline-flex">
        <Button size="sm" disabled={!!busy || unrelated} onClick={() => go("ff")} className="rounded-r-none">
          <GitMerge /> Merge into {into}
        </Button>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button size="icon-sm" aria-label="Other ways to merge" disabled={!!busy || unrelated} className="w-5 rounded-l-none border-l border-primary-foreground/25">
              <ChevronDown />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start">
            <DropdownMenuItem onSelect={() => go("ff")}>Merge into {into}</DropdownMenuItem>
            <DropdownMenuItem onSelect={() => go("no-ff")}>Merge into {into} (No Fast-forward)</DropdownMenuItem>
            <DropdownMenuItem onSelect={() => go("squash")}>Squash and Merge into {into}</DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      <span className={cn("min-w-0", conflicts?.length || unrelated ? "text-modified" : "text-muted-foreground")}>
        {unrelated
          ? "No commit in common: git won't merge them."
          : conflicts?.length
            ? `${plural(conflicts.length, "file")} would conflict: ${list}${conflicts.length > 3 ? ", …" : ""}`
            : `This merges ${plural(incoming, "commit")} from ${head.label} into ${into}${conflicts ? ", with no conflicts" : ""}.`}
      </span>
    </div>
  );
}

/** ↑/↓ move between a list's rows, which take Enter themselves. */
function arrows(e: React.KeyboardEvent<HTMLElement>) {
  if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
  const rows = [...e.currentTarget.querySelectorAll<HTMLElement>("[data-row]")];
  const to = rows[rows.indexOf(e.target as HTMLElement) + (e.key === "ArrowDown" ? 1 : -1)];
  if (!to) return;
  to.focus();
  to.scrollIntoView({ block: "nearest" });
  e.preventDefault();
}

/** Commits and files side by side when there's the room, one above the other when there isn't. */
function Panes({ sel, comparison: c, range, webUrl, onOpen }: { sel: CompareSelection; comparison: Comparison; range: { label: string; base: string; head: string }; webUrl: string | null; onOpen: Props["onOpen"] }) {
  // Ahead first, as it's what a pull request is made of; Behind when that's empty.
  const [picked, setPicked] = useState<"ahead" | "behind" | null>(null);
  const tab = picked ?? (!c.ahead && c.behind ? "behind" : "ahead");
  const { base, head } = sel;
  return (
    <div className="@container min-h-0 flex-1">
      <div className="flex h-full flex-col @3xl:flex-row">
        <section aria-label="Commits" className="flex min-h-0 flex-1 flex-col border-b border-border @3xl:border-r @3xl:border-b-0">
          <div className="flex h-8 shrink-0 items-center gap-2 border-b border-border px-3">
            <Segmented
              value={tab}
              onChange={setPicked}
              options={[
                { value: "ahead", label: `Ahead ${c.ahead}` },
                { value: "behind", label: `Behind ${c.behind}` },
              ]}
            />
            <span className="min-w-0 truncate text-[11px] text-subtle">{tab === "ahead" ? `in ${head.label}, not ${base.label}` : `in ${base.label}, not ${head.label}`}</span>
          </div>
          {/* Keyed: the other side's list starts over, and a comparison moved on reads its own. */}
          <Commits key={`${tab}:${c.base}:${c.head}`} base={tab === "ahead" ? c.base : c.head} head={tab === "ahead" ? c.head : c.base} count={tab === "ahead" ? c.ahead : c.behind} empty={tab === "ahead" ? `${head.label} has nothing ${base.label} lacks.` : `${base.label} has nothing ${head.label} lacks.`} webUrl={webUrl} onOpen={onOpen} />
        </section>
        <section aria-label="Changed files" className="flex min-h-0 flex-1 flex-col">
          <div className="flex h-8 shrink-0 items-center gap-2 px-3 text-[10.5px] text-subtle">
            <span className="font-semibold tracking-[0.08em] uppercase">Changed files</span>
            <span className="font-mono text-muted-foreground">{c.files.length}</span>
            <span className="min-w-0 truncate text-[11px]">{sel.mergeBase && !c.unrelated ? `${head.label} since it parted from ${base.label}` : `${base.label} → ${head.label}`}</span>
          </div>
          <ChangedFiles comparison={c} range={range} onOpen={onOpen} sel={sel} />
        </section>
      </div>
    </div>
  );
}

function ChangedFiles({ comparison: c, range, onOpen, sel }: { comparison: Comparison; range: { label: string; base: string; head: string }; onOpen: Props["onOpen"]; sel: CompareSelection }) {
  if (!c.files.length)
    return (
      <div className="px-4 py-6 text-center text-[12px] text-muted-foreground">
        {c.ahead || c.behind ? (sel.mergeBase ? `${sel.head.label} changes no files since it parted from ${sel.base.label}.` : "Both sides hold the same files.") : "No differences."}
      </div>
    );
  return (
    <div role="listbox" aria-label="Changed files" onKeyDown={arrows} className="min-h-0 flex-1 overflow-y-auto pb-2">
      <Windowed
        count={c.files.length}
        height={ROW}
        keep={[]}
        render={(i) => {
          const f = c.files[i];
          const row: Selection = { kind: "pr-file", range, file: f };
          return (
            <div
              key={f.path}
              role="option"
              aria-selected={false}
              data-row={f.path}
              tabIndex={i === 0 ? 0 : -1}
              onClick={() => onOpen(row, true)}
              onKeyDown={(e) => e.key === "Enter" && onOpen(row, true)}
              className="flex h-[26px] cursor-pointer items-center gap-2 pr-2 pl-4 text-[12px] outline-none hover:bg-hover focus-visible:bg-hover focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset"
            >
              <FileIcon path={f.path} />
              <PathLabel path={f.path} className="flex-1" />
              <LineCounts file={f} />
              <StatusLetter status={f.status} />
            </div>
          );
        }}
      />
    </div>
  );
}

/** The commits `head` has that `base` doesn't (commit ids), a page at a time. Opening one shows its changes. */
function Commits({ base, head, count, empty, webUrl, onOpen }: { base: string; head: string; count: number; empty: string; webUrl: string | null; onOpen: Props["onOpen"] }) {
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
        const open = () => onOpen({ kind: "changes", list: "commit", commit: c, url: commitUrl(c, { webUrl, everyOnWeb: false }) }, true);
        return (
          <ContextMenu key={c.sha}>
            <ContextMenuTrigger asChild>
              <div
                role="listitem"
                data-row={c.sha}
                tabIndex={i === 0 ? 0 : -1}
                title="Open this commit's changes"
                onClick={open}
                onKeyDown={(e) => e.key === "Enter" && open()}
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
              <ContextMenuItem onSelect={open}>
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
