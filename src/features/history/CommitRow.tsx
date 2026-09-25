import { Cloud, Tag } from "lucide-react";
import { Fragment, useEffect, useRef, useState } from "react";
import { ContextMenu, ContextMenuTrigger } from "@/components/ui/context-menu";
import { CiBadge } from "@/components/CiBadge";
import { api, type CiState, type Commit, errorMessage, type FileChange, type GraphRefs } from "@/lib/api";
import type { GraphRow, Lane } from "@/lib/git/commitGraph";
import { type Selection, selectionKey } from "@/lib/repo/selection";
import { toast } from "@/lib/app/toast";
import { cn } from "@/lib/utils";
import { fullDate, relativeTime } from "@/lib/format";
import { FileIcon } from "@/components/FileIcon";
import { LineCounts, PathLabel, StatusLetter } from "@/components/StatusBadge";
import { sumLines } from "@/features/changes/changeList";
import { groupRefs } from "./groupRefs";

/** Blame's link to a commit: open it and `path` (the file's name there). `id` is new per click. */
export interface Reveal {
  sha: string;
  path: string;
  id: number;
}

export function CommitRow({
  commit,
  remotes,
  graph,
  isHead,
  showRefs,
  onPoint,
  open,
  reveal,
  onToggle,
  activeKey,
  onOpen,
  onHover,
  url,
  ci,
  menu,
}: {
  commit: Commit;
  remotes: Set<string>;
  graph: GraphRow;
  isHead: boolean;
  showRefs: GraphRefs | undefined;
  onPoint: (row: GraphRow, e: React.MouseEvent) => void;
  open: boolean;
  reveal: Reveal | null;
  onToggle: (row: HTMLElement) => void;
  activeKey: string | null;
  onOpen: (s: Selection, pin?: boolean) => void;
  onHover: (s: Selection) => void;
  url: string | undefined;
  ci?: CiState;
  menu: React.ReactNode;
}) {
  const [files, setFiles] = useState<FileChange[] | null>(null);
  const revealed = useRef<number | null>(null);

  useEffect(() => {
    if (!open || files) return;
    let alive = true;
    api
      .commitFiles(commit.sha)
      .then((f) => {
        if (!alive) return;
        setFiles(f);
        // A blame click waiting on these files opens its own file (below).
        if (reveal && revealed.current !== reveal.id) return;
        // Jump straight into the file (a file's history: that one) so one click shows code.
        const file = f.find((x) => x.path === commit.file) ?? f[0];
        if (file) onOpen({ kind: "commit", commit, file, url });
      })
      .catch((e) => alive && toast("error", "Could not load commit", errorMessage(e)));
    // Collapsing (or opening another commit) cancels the auto-open of a late reply.
    return () => {
      alive = false;
    };
  }, [open, files, commit, url, onOpen, reveal]);

  // Each blame click opens its file, also on a row that's open already or was loaded before.
  useEffect(() => {
    if (!reveal || !open || !files || revealed.current === reveal.id) return;
    revealed.current = reveal.id;
    const file = files.find((x) => x.path === reveal.path) ?? files[0];
    if (file) onOpen({ kind: "commit", commit, file, url });
  }, [reveal, open, files, commit, url, onOpen]);

  const { add, del } = sumLines(files ?? []);
  const mark = commit.unpushed ? "Not pushed yet" : commit.notInHead ? "Not in your branch yet" : null;
  const dotLabel = isHead ? ["HEAD", mark].filter(Boolean).join(" · ") : (mark ?? undefined);

  return (
    <div className="relative" onMouseMove={(e) => onPoint(graph, e)}>
      <GraphLines row={graph} />
      <ContextMenu>
        <ContextMenuTrigger asChild>
          <div
            role="treeitem"
            aria-level={1}
            aria-expanded={open}
            tabIndex={-1}
            data-row={`commit:${commit.sha}`}
            onClick={(e) => onToggle(e.currentTarget)}
            className={cn(
              "relative flex cursor-pointer items-start gap-2.5 py-1.5 pr-2 pl-3 outline-none focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset data-[state=open]:bg-hover",
              open ? "bg-active" : "hover:bg-hover focus:bg-hover",
            )}
          >
            <span
              className={cn(
                "relative z-10 mt-[3px] size-[9px] shrink-0 rounded-full border-2",
                commit.unpushed ? "border-primary bg-primary" : commit.notInHead ? "border-added bg-added" : "border-subtle bg-sidebar",
                // Where you are, among every branch's commits.
                isHead && "outline-2 outline-offset-1 outline-primary",
              )}
              title={dotLabel}
              aria-label={dotLabel}
              role={dotLabel ? "img" : undefined}
              // In its lane and its colour, with the text after the row's last lane.
              style={{
                marginLeft: laneOf(graph.col) * LANE,
                marginRight: (lanesOf(graph) - 1 - laneOf(graph.col)) * LANE,
                borderColor: commit.unpushed || commit.notInHead || !graph.id ? undefined : laneColor(graph.id),
              }}
            />
            <div className="min-w-0 flex-1">
              <div className={cn("truncate text-[12px] leading-4", open ? "font-medium text-foreground" : "text-foreground/90")}>{commit.subject}</div>
              <div className="mt-0.5 flex min-w-0 items-center gap-1.5 text-[10.5px] text-subtle">
                <span className="min-w-0 truncate">{commit.authorName}</span>
                <span>·</span>
                <CommitTime commit={commit} />
                <CiBadge state={ci} className="ml-auto" />
                <span className={cn("shrink-0 font-mono", !ci && "ml-auto")}>{commit.shortSha}</span>
              </div>
              <RefBadges refs={commit.refs} remotes={remotes} show={showRefs} />
            </div>
          </div>
        </ContextMenuTrigger>
        {menu}
      </ContextMenu>
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
            const sel: Selection = { kind: "commit", commit, file: f, url };
            const key = selectionKey(sel);
            const active = activeKey === key;
            return (
              <div
                key={f.path}
                role="treeitem"
                aria-level={2}
                aria-selected={active}
                tabIndex={-1}
                data-row={key}
                onClick={() => onOpen(sel)}
                onDoubleClick={() => onOpen(sel, true)}
                onMouseEnter={() => onHover(sel)}
                className={cn(
                  "relative flex h-[26px] cursor-pointer items-center gap-2 pr-2 pl-8 text-[12px] outline-none focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset",
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
      )}
    </div>
  );
}

// Lane geometry, matching the dot above: 12px row padding, 9px dot, its centre 13.5px down.
const LANE = 10;
const LANE_X = 16.5;
const DOT_Y = 13.5;
const BEND_Y = DOT_Y + 10;
// Past this many lanes the rest are cut off; their commits sit on the last one shown.
const MAX_LANES = 8;
const laneOf = (i: number) => Math.min(i, MAX_LANES - 1);
const lanesOf = (row: GraphRow) => Math.min(row.width, MAX_LANES);
const laneX = (i: number) => LANE_X + laneOf(i) * LANE;
// Colour follows the branch, not the column. The list's own branch is grey like its dots, but not
// border-faint: branches have to be seen joining it.
const LANE_COLORS = ["var(--renamed)", "var(--modified)", "var(--primary)", "var(--conflict)", "var(--added)"];
const laneColor = (id: number) => (id === 0 ? "var(--subtle)" : LANE_COLORS[(id - 1) % LANE_COLORS.length]);

/** A row's share of the graph: lines passing by, ending at the dot, and leaving it for its parents. */
function GraphLines({ row }: { row: GraphRow }) {
  const x = laneX(row.col);
  const shown = (l: Lane) => l.col < MAX_LANES || l.col === row.col;
  const curve = (l: Lane, d: string, key: string) => <path key={key} data-lane={l.id} d={d} stroke={laneColor(l.id)} />;
  const line = (l: Lane, from: number | string, to: number | string, key: string) => (
    <line key={key} data-lane={l.id} x1={laneX(l.col)} x2={laneX(l.col)} y1={from} y2={to} stroke={laneColor(l.id)} />
  );
  return (
    <svg aria-hidden className="pointer-events-none absolute inset-y-0 left-0 h-full" width={laneX(lanesOf(row) - 1) + LANE / 2} fill="none">
      {row.through.filter(shown).map((l) => line(l, 0, "100%", `t${l.col}`))}
      {row.into.filter(shown).map((l) =>
        l.col === row.col ? line(l, 0, DOT_Y, `i${l.col}`) : curve(l, `M${laneX(l.col)} 0C${laneX(l.col)} ${DOT_Y} ${x} 0 ${x} ${DOT_Y}`, `i${l.col}`),
      )}
      {row.out.filter(shown).map((l) =>
        l.col === row.col ? (
          line(l, DOT_Y, "100%", `o${l.col}`)
        ) : (
          <Fragment key={`o${l.col}`}>
            {curve(l, `M${x} ${DOT_Y}C${x} ${BEND_Y} ${laneX(l.col)} ${DOT_Y} ${laneX(l.col)} ${BEND_Y}`, "c")}
            {line(l, BEND_Y, "100%", "b")}
          </Fragment>
        ),
      )}
    </svg>
  );
}

/**
 * When the commit landed on the branch, which is the order the list is in. A rebase or cherry-pick
 * keeps the date it was written, which then reads out of order: that one is in the tooltip.
 */
function CommitTime({ commit: c }: { commit: Commit }) {
  const moved = relativeTime(c.timestamp) !== relativeTime(c.committedAt);
  const title = moved
    ? `Committed ${relativeTime(c.committedAt)} by ${c.committerName} (${fullDate(c.committedAt)})\nAuthored ${relativeTime(c.timestamp)} by ${c.authorName} (${fullDate(c.timestamp)})`
    : fullDate(c.committedAt);
  return (
    <span className={cn("shrink-0", moved && "underline decoration-subtle/60 decoration-dotted underline-offset-2")} title={title}>
      {relativeTime(c.committedAt)}
    </span>
  );
}

function RefBadges({ refs, remotes, show }: { refs: string[]; remotes: Set<string>; show?: GraphRefs }) {
  const list = groupRefs(refs, remotes, show);
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
