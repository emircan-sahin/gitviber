import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { useCi } from "@/lib/github/ci";
import { api, type Commit, errorMessage, type GraphRefs, type RepoStatus, type Worktree } from "@/lib/api";
import { type GraphRow, graphRows } from "@/lib/git/commitGraph";
import { pointerMoved } from "@/lib/ui/pointer";
import type { Selection } from "@/lib/repo/selection";
import { failed, toast } from "@/lib/app/toast";
import { useListNav } from "@/lib/ui/useListNav";
import { folderName } from "@/lib/path";
import { useGitAction } from "@/hooks/useGitAction";
import { type Actions, commitUrl, type RefMenu } from "./commitActions";
import { CommitDrag } from "./commitDrag";
import { CommitMenu, PickedMenu } from "./CommitMenu";
import { CommitFileMenu } from "./CommitFileMenu";
import { MessageDialog, NameDialog } from "./CommitDialogs";
import { type DropAt, firstParentLine, reorderBefore } from "./edits";
import { useHistoryEdits } from "./useHistoryEdits";
import { usePickedCommits } from "./usePickedCommits";
import type { Points } from "@/lib/repo/compareMark";
import { CommitRow, type Reveal } from "./CommitRow";

interface Props {
  commits: Commit[];
  status: RepoStatus | null;
  /** Remote-tracking branch names (origin/main…), to group decorations. */
  remotes: Set<string>;
  /** origin's page on GitHub; null: not on GitHub. */
  webUrl: string | null;
  hasMore: boolean;
  loadMore: () => Promise<void>;
  refresh: () => Promise<void>;
  activeKey: string | null;
  onOpen: (s: Selection, pin?: boolean) => void;
  onHover: (s: Selection) => void;
  /** HEAD, when the list is another branch's history (a fork's original) rather than HEAD's. */
  headSha?: string;
  /** Where these commits live on GitHub, when that's not origin: a fork's original has them all. */
  web?: string;
  /** That repository, owner/name, for its checks. */
  ciTarget?: string;
  /** What an empty list says. */
  empty?: string;
  /** Draw branches and merges; off for search results, whose neighbours aren't parent and child. */
  graph?: boolean;
  /** Blame's link: open this commit and this file in it, once per `id` (each click is new). */
  reveal?: Reveal | null;
  /** This repo's worktrees: a commit can be picked onto the branch checked out in another. */
  worktrees?: Worktree[];
  /** Opens a worktree in this window. */
  onOpenRepo?: (path: string) => void;
  /** Every branch is listed: HEAD's keeps the first lane even below newer branches' tips. */
  pinHead?: boolean;
  /** Scroll to this commit and open it; `onJumped` says it's done, so the request can go. */
  jump?: string | null;
  onJumped?: () => void;
  /** The all-branches graph's choices, offered on the refs a commit is decorated with. */
  refMenu?: RefMenu;
  /** Only these refs' badges show, as the all-branches graph walks only them. */
  showRefs?: GraphRefs;
  /** Shows what changed between two commits, or one and the working tree, in place of the list. */
  onComparePoints?: (points: Points) => void;
  /** Narrows the history to an author's commits (the search box). */
  onAuthor?: (name: string) => void;
}

export function HistoryPanel({ commits, status, remotes, webUrl, hasMore, loadMore, refresh, activeKey, onOpen, onHover, headSha, web, ciTarget, empty = "No commits yet.", graph = true, reveal = null, worktrees = [], onOpenRepo, pinHead = false, jump = null, onJumped, refMenu, showRefs, onComparePoints, onAuthor }: Props) {
  const [open, setOpen] = useState<string | null>(reveal?.sha ?? null);
  useEffect(() => {
    if (reveal) setOpen(reveal.sha);
  }, [reveal]);
  const scroller = useRef<HTMLDivElement>(null);
  const anchor = useRef<{ el: HTMLElement; top: number } | null>(null);
  const [naming, setNaming] = useState<{ kind: "branch" | "tag"; commit: Commit } | null>(null);

  // An action that stops on conflicts: Workspace then brings Changes into view.
  const { busy, run, runNet } = useGitAction({ refresh });
  // A pick into another worktree, which records no undo entry here.
  const [picking, setPicking] = useState(false);

  // git runs in that worktree, and the entry lands in its undo history, not this one's. A pick
  // stopped on conflicts waits there, for its own Changes panel to finish.
  const pickInto = async (w: Worktree, c: Commit) => {
    const branch = w.branch ?? folderName(w.path);
    const where = folderName(w.path);
    const go = onOpenRepo && { label: "Switch to worktree", run: () => onOpenRepo(w.path) };
    setPicking(true);
    try {
      if (await api.cherryPickInto(w.path, c.sha)) toast("info", `Cherry-pick onto ${branch} stopped on conflicts`, `It waits in ${where}: switch there to resolve them and continue.`, go);
      else toast("success", `Cherry-picked ${c.shortSha} onto ${branch}`, `In ${where}; undo it from there.`, go);
    } catch (e) {
      toast("error", `Cherry-pick onto ${branch} failed`, errorMessage(e));
    } finally {
      setPicking(false);
      await refresh();
    }
  };

  // HEAD's own history starts at the HEAD the user sees.
  const head = headSha ?? commits[0]?.sha ?? "";
  const bySha = (sha: string | undefined) => commits.find((x) => x.sha === sha);
  const { selection, many, pickedSet, click, onMove, onEscape, clear } = usePickedCommits(commits, open);
  const { rewrite, squash, reword, messaging, closeMessage, submit } = useHistoryEdits({ commits, head, graph, run });
  // Only commits GitHub has have checks: origin's, or all of a fork's original.
  const ci = useCi(ciTarget ?? null, commits.filter((c) => c.onOrigin || !!web).slice(0, 100).map((c) => c.sha));

  const actions: Actions = {
    status,
    headSha: head,
    webUrl: web ?? webUrl,
    locked: !!busy || picking || !!status?.operation,
    run,
    runNet,
    name: (kind, commit) => setNaming({ kind, commit }),
    rewrite,
    reword,
    squash,
    refresh,
    openAll: (c) => onOpen({ kind: "changes", list: "commit", commit: c, url: commitUrl(c, { webUrl: web ?? webUrl, everyOnWeb: !!web }) }, true),
    everyOnWeb: !!web,
    pickTargets: worktrees.filter((w) => !w.current && !w.bare && !w.prunable && w.branch),
    pickInto,
    pickMany: (cs) => {
      const target = status?.branch ?? "HEAD";
      return run("Cherry-pick", () => api.cherryPickMany(cs.map((c) => c.sha).reverse()), `Cherry-picked ${cs.length} commits onto ${target}`);
    },
    remotes,
    refMenu,
    showRefs,
    comparePoints: onComparePoints,
    filterAuthor: onAuthor,
  };

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

  // A drag takes the picked commits it starts on, else its own.
  const drag = (sha: string) => {
    if (many && pickedSet.has(sha)) return selection.map((c) => c.sha);
    clear();
    return [sha];
  };
  const drop = (shas: string[], at: DropAt) => {
    const moved = commits.filter((c) => shas.includes(c.sha));
    const target = bySha(at.sha);
    if (!target || !moved.length) return;
    if (at.where === "onto") return squash(moved, target.sha, true);
    const before = reorderBefore(commits.map((c) => c.sha), new Set(shas), at);
    if (before === undefined) return;
    void rewrite({ kind: "reorder", shas, before }, moved);
  };

  // Without the graph, one line joins each row to the next.
  const rows = useMemo(() => {
    if (!graph) return graphRows(commits.map((c, i) => ({ sha: c.sha, parents: i + 1 < commits.length ? [commits[i + 1].sha] : [] })));
    return graphRows(commits, pinHead && commits.some((c) => c.sha === head) ? head : undefined);
  }, [commits, graph, pinHead, head]);

  // A jump waits for its commit to be listed: the caller loads pages until it is.
  useEffect(() => {
    if (!jump) return;
    const row = scroller.current?.querySelector(`[data-row="commit:${jump}"]`);
    if (!row) return;
    row.scrollIntoView({ block: "center" });
    setOpen(jump);
    onJumped?.();
  }, [jump, commits, onJumped]);
  // Hovering a commit brings its branch forward, and a merge's merged-in one. Only the lines
  // involved change: rewriting a stylesheet restyled the whole app on each row a scroll carried by.
  const lit = useRef<{ row: GraphRow; lines: Element[] } | null>(null);
  const light = (row: GraphRow | null) => {
    const list = scroller.current;
    if (!list || (lit.current?.row ?? null) === row) return;
    for (const el of lit.current?.lines ?? []) el.removeAttribute("data-lit");
    const ids = row ? new Set([row.id, ...row.out.map((l) => l.id)]) : [];
    const lines = [...ids].flatMap((id) => [...list.querySelectorAll(`[data-lane="${id}"]`)]);
    for (const el of lines) el.setAttribute("data-lit", "");
    lit.current = row ? { row, lines } : null;
    list.toggleAttribute("data-dim", !!row);
  };
  // Rows scrolling under a still pointer get a mousemove too; only a real move lights a row.
  const point = (row: GraphRow, e: React.MouseEvent) => pointerMoved(e) && light(row);

  const more = () => loadMore().catch(failed("Could not load history"));
  const nav = useListNav({ activeKey, loadMore: hasMore ? more : null, onMove });
  // Only the branch's own history as the graph draws it: not matches, nor another branch's commits.
  const draggable = graph && !headSha && !actions.locked;
  // Nor a merged-in side branch's, which an edit can't name.
  const line = useMemo(() => firstParentLine(commits, head), [commits, head]);

  if (!commits.length) {
    return <div className="px-6 pt-20 text-center text-[12px] text-subtle">{empty}</div>;
  }

  return (
    <div
      ref={scroller}
      onScroll={() => light(null)}
      onMouseLeave={() => light(null)}
      className="h-full overflow-x-hidden overflow-y-auto py-1 [&[data-dim]_[data-lane]:not([data-lit])]:opacity-25"
    >
      <CommitDrag drag={drag} canDrop={(sha) => line.has(sha)} onDrop={drop}>
        {({ dragged, at, justDragged }) => (
          <div
            role="tree"
            aria-label="History"
            aria-multiselectable
            {...nav}
            onKeyDown={(e) => {
              // Esc lets the picked commits go; without any, it isn't this list's to take.
              if (e.key === "Escape" && onEscape()) {
                e.preventDefault();
                e.stopPropagation();
              } else nav.onKeyDown(e);
            }}
          >
            {commits.map((c, i) => (
              <CommitRow
                key={c.sha}
                commit={c}
                remotes={remotes}
                graph={rows[i]}
                isHead={c.sha === head}
                showRefs={showRefs}
                onPoint={point}
                open={open === c.sha}
                picked={pickedSet.has(c.sha)}
                draggable={draggable && line.has(c.sha)}
                dragged={dragged.has(c.sha)}
                dropAt={at?.sha === c.sha ? at.where : undefined}
                reveal={reveal?.sha === c.sha ? reveal : null}
                onClick={(e) => {
                  if (!justDragged() && !click(c, e)) toggle(c.sha, e.currentTarget);
                }}
                onMenu={() => !pickedSet.has(c.sha) && clear()}
                onAuthor={onAuthor && (() => onAuthor(c.authorName))}
                activeKey={activeKey}
                onOpen={onOpen}
                onHover={onHover}
                url={commitUrl(c, actions)}
                ci={ci[c.sha]}
                fileMenu={(f, sel) => <CommitFileMenu commit={c} file={f} sel={sel} refresh={refresh} onOpen={onOpen} />}
                menu={many && pickedSet.has(c.sha) ? <PickedMenu commits={selection} all={!hasMore && selection.length === commits.length} actions={actions} /> : <CommitMenu commit={c} head={c.sha === head} actions={actions} />}
              />
            ))}
          </div>
        )}
      </CommitDrag>
      {hasMore && (
        <div className="p-2">
          <Button variant="secondary" size="sm" className="w-full" onClick={more}>
            Load more
          </Button>
        </div>
      )}
      {naming && <NameDialog {...naming} onClose={() => setNaming(null)} run={run} />}
      {messaging && <MessageDialog messaging={messaging} onClose={closeMessage} onSubmit={(message) => void submit(messaging, message)} />}
    </div>
  );
}
