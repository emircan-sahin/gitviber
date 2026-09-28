// A PR's commits, as GitHub's Commits tab lists them: by day, oldest first. Picking one (or a run
// of them with ⇧) narrows Files changed to what they changed.
import { ask } from "@/lib/app/ask";
import { ChevronLeft, ChevronRight, Cherry, Copy, ExternalLink, GitCommitHorizontal } from "lucide-react";
import { Fragment, useEffect, useMemo, useState } from "react";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuTrigger } from "@/components/ui/context-menu";
import { Tip } from "@/components/ui/tooltip";
import { CiBadge } from "@/components/CiBadge";
import { api, type Commit, type Pull, repoOf } from "@/lib/api";
import { copyText } from "@/lib/app/clipboard";
import { useCi } from "@/lib/github/ci";
import { byDay, pickLabel } from "@/lib/github/pullCommits";
import { saveSeenCommits, seenCommits } from "@/lib/github/seenCommits";
import { openOnGitHub } from "@/lib/github/url";
import { useListNav } from "@/lib/ui/useListNav";
import { plural } from "@/lib/format";
import { cn } from "@/lib/utils";
import type { GitRun } from "@/hooks/useGitAction";
import { CommitTime } from "@/features/history/CommitRow";
import { Section } from "@/features/github/shared/Section";
import type { CommitPick } from "./useCommitPick";

/**
 * Which commits are new: pushed since you last looked. A PR opened for the first time has none; a
 * commit stops being new once picked, as a file's viewed mark is set by looking at it.
 */
function useSeen(url: string, commits: Commit[], picked: Commit[] | null) {
  const [seen, setSeen] = useState(() => seenCommits(url));
  const shas = commits.map((c) => c.sha);
  const list = shas.join(",");
  const lookedAt = picked?.map((c) => c.sha).join(",") ?? "";
  useEffect(() => {
    if (!shas.length) return;
    const add = lookedAt ? lookedAt.split(",") : [];
    const next = seen ? new Set([...seen, ...add]) : new Set(shas);
    if (next.size !== seen?.size) setSeen(next);
    saveSeenCommits(url, shas, next);
  }, [url, list, lookedAt]);
  return (sha: string) => !!seen && !seen.has(sha);
}

export function PullCommits({ pull, commits, total, pick, busy, act }: { pull: Pull; commits: Commit[]; total: number; pick: CommitPick; busy: boolean; act: GitRun }) {
  const { picked } = pick;
  const isNew = useSeen(pull.url, commits, picked);
  const ci = useCi(
    repoOf(pull.url),
    commits.map((c) => c.sha),
  );
  const [open, setOpen] = useState<Set<string>>(() => new Set());
  // The pick scrolls into view by the keys that make it, not the page with it: activeKey stays unset.
  const nav = useListNav({ activeKey: null });
  const days = useMemo(() => byDay(commits), [commits]);
  const fresh = commits.filter((c) => isNew(c.sha)).length;

  // ⇧↑/⇧↓ widen or narrow the run, as ⇧-click does; Esc shows every commit's changes again.
  const onKeyDown = (e: React.KeyboardEvent) => {
    const row = e.target instanceof HTMLElement && e.target.matches("[data-row]") ? e.target : null;
    if (row && !e.altKey && !e.metaKey && !e.ctrlKey) {
      if (e.shiftKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
        const rows = [...(nav.ref.current?.querySelectorAll<HTMLElement>("[data-row]") ?? [])];
        const to = rows[rows.indexOf(row) + (e.key === "ArrowUp" ? -1 : 1)];
        if (to) {
          to.focus();
          to.scrollIntoView({ block: "nearest" });
          pick.run(pick.anchor ?? row.dataset.row!, to.dataset.row!);
        }
        e.preventDefault();
        return;
      }
      if (e.key === "Escape" && picked) {
        pick.clear();
        e.preventDefault();
        return;
      }
    }
    nav.onKeyDown(e);
  };

  return (
    <Section title="Commits" aside={fresh ? `${fresh} new · ${commits.length}` : `${commits.length}`}>
      {total > commits.length && <div className="border-b border-border px-3 py-1.5 text-[11.5px] text-subtle">The latest {commits.length} of {total} commits.</div>}
      {/* A long list scrolls on its own, so Files changed stays right under it. */}
      <div ref={nav.ref} onKeyDown={onKeyDown} onFocus={nav.onFocus} data-list-nav="" role="listbox" aria-label="Commits" aria-multiselectable className="max-h-[360px] overflow-y-auto outline-none">
        {days.map(({ day, list }) => (
          <Fragment key={list[0].sha}>
            <div className="sticky top-0 z-10 flex h-6 items-center border-b border-border bg-panel px-3 text-[11px] text-subtle">Commits on {day}</div>
            {list.map((c) => {
              const on = !!picked?.includes(c);
              const body = open.has(c.sha);
              return (
                <Fragment key={c.sha}>
                  <ContextMenu>
                    <ContextMenuTrigger asChild>
                      <div
                        role="option"
                        aria-selected={on}
                        tabIndex={-1}
                        data-row={c.sha}
                        onClick={(e) => pick.pick(c.sha, e.shiftKey)}
                        className={cn(
                          "relative flex h-7 cursor-pointer items-center gap-2 border-b border-border px-3 text-[12px] outline-none select-none last:border-0 focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset data-[state=open]:bg-hover",
                          on ? "bg-primary/15" : "hover:bg-hover focus:bg-hover",
                        )}
                      >
                        {on && <span className="absolute inset-y-0 left-0 w-0.5 bg-primary" />}
                        <GitCommitHorizontal className="size-3.5 shrink-0 text-subtle" />
                        <span className="min-w-0 truncate text-foreground/90">{c.subject}</span>
                        {c.body && (
                          <button
                            tabIndex={-1}
                            aria-label={body ? "Hide message" : "Show message"}
                            aria-expanded={body}
                            onClick={(e) => {
                              e.stopPropagation();
                              setOpen((s) => new Set(body ? [...s].filter((x) => x !== c.sha) : [...s, c.sha]));
                            }}
                            className={cn("shrink-0 rounded-sm px-1 font-mono text-[10px] leading-3.5 text-muted-foreground hover:bg-active hover:text-foreground", body ? "bg-active" : "bg-hover")}
                          >
                            …
                          </button>
                        )}
                        {isNew(c.sha) && <span className="shrink-0 rounded-full bg-primary/15 px-1.5 text-[10px] leading-4 font-medium text-primary">New</span>}
                        <span className="ml-auto flex shrink-0 items-center gap-2 pl-2 text-[11px] text-subtle">
                          <span className="max-w-36 truncate">{c.authorName}</span>
                          <CommitTime commit={c} />
                          <CiBadge state={ci[c.sha]} />
                          <Tip label="Copy SHA">
                            <button
                              tabIndex={-1}
                              onClick={(e) => {
                                e.stopPropagation();
                                void copyText(c.sha, "SHA copied");
                              }}
                              className="rounded-sm px-0.5 font-mono hover:bg-active hover:text-foreground"
                            >
                              {c.shortSha}
                            </button>
                          </Tip>
                        </span>
                      </div>
                    </ContextMenuTrigger>
                    <PullCommitMenu pull={pull} commit={c} busy={busy} act={act} />
                  </ContextMenu>
                  {body && <div className="border-b border-border bg-panel/50 last:border-0 py-2 pr-3 pl-[34px] font-mono text-[11.5px] whitespace-pre-wrap text-muted-foreground select-text">{c.body}</div>}
                </Fragment>
              );
            })}
          </Fragment>
        ))}
      </div>
    </Section>
  );
}

/** Over Files changed while commits are picked: which, the ones either side, and the way back. */
export function PickBar({ pick }: { pick: CommitPick }) {
  const { picked } = pick;
  if (!picked) return null;
  const one = picked.length === 1;
  const arrow = (dir: -1 | 1) => {
    const Icon = dir < 0 ? ChevronLeft : ChevronRight;
    const label = `${dir < 0 ? "Previous" : "Next"} commit (${dir < 0 ? "←" : "→"})`;
    return (
      <Tip label={label}>
        <button aria-label={label} disabled={!pick.next(dir)} onClick={() => pick.step(dir)} className="flex size-5 items-center justify-center rounded-sm text-muted-foreground hover:bg-active hover:text-foreground disabled:opacity-40 disabled:hover:bg-transparent">
          <Icon className="size-3.5" />
        </button>
      </Tip>
    );
  };
  return (
    <div className="flex h-8 items-center gap-2 border-b border-border bg-primary/8 px-3 text-[11.5px]">
      <GitCommitHorizontal className="size-3.5 shrink-0 text-primary" />
      <span className="shrink-0 text-muted-foreground">Showing changes from {plural(picked.length, "commit")}</span>
      <span className="shrink-0 font-mono text-foreground/85">{pickLabel(picked)}</span>
      {one && <span className="min-w-0 truncate text-foreground/85">{picked[0].subject}</span>}
      <span className="ml-auto flex shrink-0 items-center gap-1">
        {one && arrow(-1)}
        {one && arrow(1)}
        <button onClick={pick.clear} className="ml-1 rounded-sm px-1.5 py-0.5 text-primary hover:bg-primary/12">
          Show all
        </button>
      </span>
    </div>
  );
}

function PullCommitMenu({ pull, commit: c, busy, act }: { pull: Pull; commit: Commit; busy: boolean; act: GitRun }) {
  const checkout = async () => {
    const ok = await ask(`Check out ${c.shortSha} without a branch (detached HEAD)? New commits made there belong to no branch until you create one.`, {
      title: "Checkout commit",
      kind: "warning",
      okLabel: "Checkout",
    });
    if (ok) await act("Checkout", () => api.checkoutCommit(c.sha), `Checked out ${c.shortSha}`);
  };
  return (
    <ContextMenuContent>
      <ContextMenuItem onSelect={() => copyText(c.sha, "SHA copied")}>
        <Copy /> Copy SHA
      </ContextMenuItem>
      <ContextMenuItem onSelect={() => openOnGitHub(`${pull.url}/commits/${c.sha}`)}>
        <ExternalLink /> Open on GitHub
      </ContextMenuItem>
      <ContextMenuSeparator />
      <ContextMenuItem disabled={busy} onSelect={checkout}>
        <GitCommitHorizontal /> Checkout commit
      </ContextMenuItem>
      {/* Your branch has it already (the PR's own, checked out): picking it would change nothing. */}
      <ContextMenuItem
        disabled={busy || !c.notInHead}
        onSelect={() => act("Cherry-pick", () => api.cherryPick(c.sha), `Cherry-picked ${c.shortSha}`, undefined, { conflicts: "Resolve them in Changes, then continue." })}
      >
        <Cherry /> Cherry-pick onto current branch
      </ContextMenuItem>
    </ContextMenuContent>
  );
}
