import { ask } from "@tauri-apps/plugin-dialog";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { useCi } from "@/lib/github/ci";
import { api, type Commit, errorMessage, type GraphRefs, type HistoryEdit, type RepoStatus, type Worktree } from "@/lib/api";
import { type GraphRow, graphRows } from "@/lib/git/commitGraph";
import { pointerMoved } from "@/lib/ui/pointer";
import type { Selection } from "@/lib/repo/selection";
import { failed, toast } from "@/lib/app/toast";
import { useListNav } from "@/lib/ui/useListNav";
import { folderName } from "@/lib/path";
import { useGitAction } from "@/hooks/useGitAction";
import { type Actions, commitUrl, dropsPushed, PUSHED_WARNING, type RefMenu } from "./commitActions";
import { CommitMenu } from "./CommitMenu";
import { MessageDialog, NameDialog } from "./CommitDialogs";
import { CommitRow, type Reveal } from "./CommitRow";

interface Props {
  commits: Commit[];
  status: RepoStatus | null;
  /** Remote-tracking branch names (origin/main…), to group decorations. */
  remotes: Set<string>;
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
}

export function HistoryPanel({ commits, status, remotes, hasMore, loadMore, refresh, activeKey, onOpen, onHover, headSha, web, ciTarget, empty = "No commits yet.", graph = true, reveal = null, worktrees = [], onOpenRepo, pinHead = false, jump = null, onJumped, refMenu, showRefs }: Props) {
  const [open, setOpen] = useState<string | null>(reveal?.sha ?? null);
  useEffect(() => {
    if (reveal) setOpen(reveal.sha);
  }, [reveal]);
  const scroller = useRef<HTMLDivElement>(null);
  const anchor = useRef<{ el: HTMLElement; top: number } | null>(null);
  const [webUrl, setWebUrl] = useState<string | null>(null);
  const [naming, setNaming] = useState<{ kind: "branch" | "tag"; commit: Commit } | null>(null);
  const [messaging, setMessaging] = useState<{ kind: "reword" | "squash"; commit: Commit } | null>(null);

  // `remotes` is rebuilt on every git refresh, including the one `git remote set-url` causes.
  useEffect(() => {
    api.githubWebUrl().then(setWebUrl, () => setWebUrl(null));
  }, [remotes]);

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
  // Only commits GitHub has have checks: origin's, or all of a fork's original.
  const ci = useCi(ciTarget ?? null, commits.filter((c) => c.onOrigin || !!web).slice(0, 100).map((c) => c.sha));

  // Everything from the edit's oldest commit on is made again: pushed ones would need a force-push.
  const rewrite = async (edit: HistoryEdit, c: Commit) => {
    const oldest = edit.kind === "squash" || (edit.kind === "move" && !edit.up) ? bySha(c.parents[0]) : c;
    const from = oldest?.parents[0] ?? (oldest ? null : c.parents[0]);
    const drops = from ? await dropsPushed(from) : false;
    if (drops === null) return;
    const verb = { reword: "Reword", squash: edit.kind === "squash" && edit.message === null ? "Fixup" : "Squash", drop: "Drop", move: "Move" }[edit.kind];
    const warnings = [...(edit.kind === "drop" ? [`Drop "${c.subject}"? Its changes leave the branch.`] : []), ...(drops ? [PUSHED_WARNING] : [])];
    if (warnings.length && !(await ask(warnings.join("\n\n"), { title: `${verb} commit`, kind: "warning", okLabel: verb }))) return;
    const done = { reword: "Commit reworded", squash: `Squashed ${c.shortSha} into its parent`, drop: `Dropped ${c.shortSha}`, move: `Moved ${c.shortSha}` }[edit.kind];
    await run(verb, () => api.rewrite(head, edit), done);
  };
  const actions: Actions = {
    status,
    headSha: head,
    webUrl: web ?? webUrl,
    locked: !!busy || picking || !!status?.operation,
    run,
    runNet,
    name: (kind, commit) => setNaming({ kind, commit }),
    rewrite,
    message: (kind, commit) => setMessaging({ kind, commit }),
    refresh,
    everyOnWeb: !!web,
    pickTargets: worktrees.filter((w) => !w.current && !w.bare && !w.prunable && w.branch),
    pickInto,
    remotes,
    refMenu,
    showRefs,
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
  const nav = useListNav({ activeKey, loadMore: hasMore ? more : null });

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
      <div role="tree" aria-label="History" {...nav}>
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
          reveal={reveal?.sha === c.sha ? reveal : null}
          onToggle={(el) => toggle(c.sha, el)}
          activeKey={activeKey}
          onOpen={onOpen}
          onHover={onHover}
          url={commitUrl(c, actions)}
          ci={ci[c.sha]}
          menu={<CommitMenu commit={c} head={c.sha === head} actions={actions} />}
        />
      ))}
      </div>
      {hasMore && (
        <div className="p-2">
          <Button variant="secondary" size="sm" className="w-full" onClick={more}>
            Load more
          </Button>
        </div>
      )}
      {naming && <NameDialog {...naming} onClose={() => setNaming(null)} run={run} />}
      {messaging && (
        <MessageDialog {...messaging} parent={bySha(messaging.commit.parents[0])} onClose={() => setMessaging(null)} onSubmit={(message) => rewrite({ kind: messaging.kind, sha: messaging.commit.sha, message }, messaging.commit)} />
      )}
    </div>
  );
}
