import { ask } from "@tauri-apps/plugin-dialog";
import { Check, CircleDashed, ExternalLink, FolderGit2, GitBranch, GitPullRequest, GitPullRequestClosed, Loader2, MinusCircle, RefreshCw, X } from "lucide-react";
import { useCallback, useEffect, useRef } from "react";
import { PageFind } from "@/components/FindBox";
import { Button } from "@/components/ui/button";
import { accessFor, api, errorMessage, fullName, github, type MergeMethod, type Pull, type PullCheck, repoOf } from "@/lib/api";
import { listIsBehind, useGitHubData } from "@/lib/github/githubCache";
import type { Selection } from "@/lib/repo/selection";
import { cn } from "@/lib/utils";
import { isoToUnix, plural, relativeTime } from "@/lib/format";
import { openOnGitHub } from "@/lib/github/url";
import { FileIcon } from "@/components/FileIcon";
import { pullSource } from "./pullSource";
import { PullStateIcon, PullStatePill } from "@/features/github/shared/StateBadges";
import { notifyPullsChanged } from "@/features/github/shared/changed";
import { CopyLinkButton } from "@/features/github/shared/LinkMenu";
import { openWorktreeDialog } from "@/features/worktrees/WorktreeDialogs";
import { LineCounts, PathLabel, StatusLetter } from "@/components/StatusBadge";
import { PullMarkdown } from "@/features/github/shared/GitHubMarkdown";
import { Section } from "@/features/github/shared/Section";
import { useGitAction } from "@/hooks/useGitAction";
import { useGitHubAccount } from "@/features/github/shared/useGitHubAccount";
import { MergeBox, ReviewButton } from "./PullActions";
import { METHODS, REVIEWS } from "./actionLabels";

export function PullView({ pull, onOpen }: { pull: Pull; onOpen: (s: Selection) => void }) {
  // The repository the PR is in: origin, or a fork's parent.
  const target = repoOf(pull.url);
  const detail = useGitHubData(
    `pr:${pull.url}`,
    useCallback(() => github.detail(target, pull.number), [target, pull.number]),
  );
  const d = detail.data ?? null;
  const error = detail.error === undefined ? null : errorMessage(detail.error);
  // Merged or closed on GitHub: the list still shows it as it was.
  useEffect(() => {
    if (d && listIsBehind("pulls:", d)) notifyPullsChanged();
  }, [d]);
  // And the other way: the tab took a newer copy from a list refresh than this page shows.
  const { refresh } = detail;
  useEffect(() => {
    if (d && pull.updatedAt > d.updatedAt) refresh(true);
  }, [pull.updatedAt]);
  // GitHub computes mergeability in the background and answers null until it's done; the
  // regular recheck is minutes away, so ask again soon, backing off, then leave it to ⟳.
  const polls = useRef(0);
  const checking = d?.state === "open" && d.mergeable === null;
  useEffect(() => {
    if (!checking) polls.current = 0;
    if (!checking || polls.current >= 5) return;
    const t = setTimeout(() => {
      polls.current++;
      refresh(true);
    }, 2000 * 2 ** polls.current);
    return () => clearTimeout(t);
  }, [checking, d]);
  // Fetching the PR's commits can take a moment; the page shows without waiting for it.
  // The result depends only on the two commits, so it's rarely worth recomputing.
  const files = useGitHubData(
    d && `files:${pull.url}:${d.baseSha}:${d.headSha}`,
    useCallback(() => (d ? github.files(target, d) : Promise.reject(new Error("no pull request"))), [target, d]),
    600_000,
  );
  // Normally already loaded, by the PRs panel.
  const { account } = useGitHubAccount();
  const load = () => detail.refresh(true);

  // A checkout moves HEAD; the repo watcher refreshes the rest of the window.
  const { busy, run: act } = useGitAction({
    conflicts: "Resolve them in Changes, then push.",
    onDone: () => {
      notifyPullsChanged();
      return load();
    },
  });

  const p = d ?? pull;
  // The branch lives on origin: checked out under its name, and a pushed fix updates the PR.
  // Unknown until the account loads, and a guess could fetch another repo's same-named branch.
  const origin = account?.origin ? fullName(account.origin.repo) : null;
  const sameRepo = !!origin && p.headRepo?.toLowerCase() === origin.toLowerCase();
  const access = accessFor(account, p.url);
  const own = account?.login === p.author;
  // GitHub lets a PR's author and triagers close and reopen it; merging needs write access.
  const canClose = !!access?.push || !!access?.triage || own;
  // But an author can't reopen what a maintainer closed.
  const canReopen = !!access?.push || !!access?.triage || (own && d?.closedBy === account?.login);
  const inOrigin = !!access && access === account?.origin;
  const source = pullSource(p, account);
  const checkoutBranch = source?.branch ?? p.headRef;
  // A fix pushed to origin updates the PR only when its branch lives there.
  const canResolve = sameRepo && !!access;

  const merge = async (method: MergeMethod) => {
    const ok = await ask(`${METHODS[method]}: #${p.number} into ${p.baseRef}?`, { title: "Merge pull request", okLabel: "Merge" });
    if (ok) await act("Merge", () => github.merge(target, p.number, method), `Merged #${p.number}`);
  };

  const setOpen = async (open: boolean) => {
    if (!open && !(await ask(`Close #${p.number} without merging?`, { title: "Close pull request", okLabel: "Close" }))) return;
    await act(open ? "Reopen" : "Close", () => github.setOpen(target, p.number, open), `${open ? "Reopened" : "Closed"} #${p.number}`);
  };

  // GitHub can't merge it: bring the conflicts home. Check out the PR branch and merge
  // the base into it; the conflicts then open in Changes like any local merge.
  // In a fork's original, the base comes from the remote pointing at it (added if missing).
  const resolveLocally = async () => {
    const from = inOrigin ? `origin/${p.baseRef}` : `${target}'s ${p.baseRef}`;
    const ok = await ask(`Check out ${p.headRef} and merge ${from} into it? Conflicts will open in Changes; push when done.`, {
      title: "Resolve conflicts locally",
      okLabel: "Start",
    });
    if (!ok) return;
    await act(
      "Merge",
      async () => {
        await github.checkout(target, p.number, p.headRef, sameRepo);
        if (inOrigin) {
          await api.fetch();
          return api.merge(`origin/${p.baseRef}`);
        }
        const remote = (await github.originalRemote(target, true)) ?? (await github.addOriginalRemote());
        return api.merge(`${remote}/${p.baseRef}`);
      },
      `Merged ${p.baseRef} into ${p.headRef}; review and push`,
    );
  };

  if (error && !d) {
    return <div className="p-8 text-center text-[12.5px] text-muted-foreground">{error}</div>;
  }

  return (
    // Focusable so the keyboard can scroll it (focusPanel("code") lands here).
    <div data-code-scroll tabIndex={0} className="min-h-0 flex-1 overflow-y-auto outline-none">
      <PageFind />
      <div className="mx-auto max-w-4xl px-6 py-5">
        <div className="flex items-start gap-3">
          <PullStateIcon pull={p} className="mt-1.5 size-4" />
          <div className="min-w-0 flex-1">
            <h1 className="text-[17px] leading-snug font-semibold select-text">
              {p.title} <span className="font-normal text-subtle">#{p.number}</span>
            </h1>
            <div className="mt-1.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-[12px] text-muted-foreground">
              <PullStatePill pull={p} />
              {/* GitHub's wording: a merge is its merger's, known once the detail is in. */}
              <span className="text-foreground/85">{p.state === "merged" ? (d?.mergedBy ?? p.author) : p.author}</span>
              <span>{p.state === "merged" ? "merged" : p.state === "closed" ? "wanted to merge" : "wants to merge"}</span>
              <BranchChip name={p.headRef} />
              <span>into</span>
              <BranchChip name={p.baseRef} />
              {d && (
                <>
                  <span className="text-subtle">·</span>
                  <span>
                    {plural(d.commits, "commit")}
                  </span>
                  <LineCounts file={{ additions: d.additions, deletions: d.deletions }} />
                </>
              )}
            </div>
          </div>
        </div>

        <div className="mt-4 flex flex-wrap items-center gap-1.5">
          {p.state === "open" && (
            <Button variant="secondary" size="sm" disabled={!!busy || !account} onClick={() => act("Checkout", () => github.checkout(target, p.number, p.headRef, sameRepo), `Switched to ${checkoutBranch}`)}>
              <GitBranch /> Checkout
            </Button>
          )}
          {/* Reviewing without leaving this worktree's branch: an agent, or you, works in the new one. */}
          {p.state === "open" && (
            <Button variant="secondary" size="sm" disabled={!!busy || !source} onClick={() => source && openWorktreeDialog({ kind: "new", pull: source })}>
              <FolderGit2 /> Check out in new worktree…
            </Button>
          )}
          {p.state === "open" && <ReviewButton pull={p} own={own} counts={!!access?.push} busy={!!busy} onSubmit={(event, body) => act("Review", () => github.review(target, p.number, event, body), REVIEWS[event].done)} />}
          {p.state === "open" && canClose && (
            <Button variant="secondary" size="sm" disabled={!!busy} onClick={() => setOpen(false)}>
              <GitPullRequestClosed /> Close
            </Button>
          )}
          {p.state === "closed" && canReopen && (
            <Button variant="secondary" size="sm" disabled={!!busy} onClick={() => setOpen(true)}>
              <GitPullRequest /> Reopen
            </Button>
          )}
          <Button variant="secondary" size="sm" onClick={() => openOnGitHub(p.url)}>
            <ExternalLink /> Open on GitHub
          </Button>
          <CopyLinkButton url={p.url} />
          <Button variant="ghost" size="icon-sm" onClick={load} disabled={!!busy || detail.loading}>
            <RefreshCw className={cn(detail.loading && "animate-spin")} />
          </Button>
          {busy && (
            <span className="flex items-center gap-1.5 text-[12px] text-muted-foreground">
              <Loader2 className="size-3.5 animate-spin" /> {busy}…
            </span>
          )}
          {/* Cached data stays on screen; a failed refresh (a rate limit, say) is noted beside it. */}
          {error && !busy && (
            <span className="min-w-0 truncate text-[12px] text-removed" title={error}>
              Couldn't refresh: {error}
            </span>
          )}
        </div>

        {d && p.state === "open" && <MergeBox detail={d} busy={!!busy} canMerge={!!access?.push} canResolve={canResolve} onMerge={merge} onResolve={resolveLocally} />}

        {d && (d.checks.length > 0 || d.checksError) && (
          <Section title="Checks" aside={d.checks.length > 0 ? checkSummary(d.checks) : undefined}>
            {d.checksError && <div className="px-3 py-2 text-[12px] text-removed">Could not load all checks: {d.checksError}</div>}
            {d.checks.map((c, i) => (
              <div key={`${c.name}${i}`} className="flex h-7 items-center gap-2 px-3 text-[12px]">
                <CheckIcon state={c.state} />
                <span className="truncate">{c.name}</span>
                {c.url?.startsWith("https://github.com/") && (
                  <button onClick={() => github.openUrl(c.url!)} className="ml-auto text-[11px] text-subtle hover:text-foreground focus-visible:text-foreground">
                    Details
                  </button>
                )}
              </div>
            ))}
          </Section>
        )}

        <Section title="Files changed" aside={files.data ? `${files.data.files.length}` : d ? `${d.changedFiles}` : undefined}>
          {!files.data &&
            (files.error !== undefined && !files.loading ? (
              <div className="px-3 py-2 text-[12px] text-removed">Could not load the PR's files: {errorMessage(files.error)}</div>
            ) : (
              <div className="flex items-center gap-2 px-3 py-2 text-[12px] text-subtle">
                <Loader2 className="size-3.5 animate-spin" /> Fetching the PR's commits…
              </div>
            ))}
          {files.data?.files.map((f) => (
            <button
              key={f.path}
              onClick={() => onOpen({ kind: "pr-file", range: { number: p.number, pullUrl: p.url, base: files.data!.base, head: files.data!.head }, file: f })}
              className="flex h-7 w-full cursor-pointer items-center gap-2 px-3 text-left text-[12px] outline-none hover:bg-hover focus-visible:bg-hover"
            >
              <FileIcon path={f.path} />
              <PathLabel path={f.path} className="flex-1" />
              <LineCounts file={f} />
              <StatusLetter status={f.status} />
            </button>
          ))}
        </Section>

        <Section title="Description">
          <PullMarkdown pull={pull} idPrefix="pr-d-" text={d?.body || ""} empty={d ? "No description provided." : ""} />
        </Section>

        {d && d.comments.length > 0 && (
          <Section title="Conversation" aside={`${d.comments.length}`}>
            {d.comments.map((c, i) => (
              <div key={i} className="border-b border-border px-3 py-2.5 last:border-0">
                <div className="flex items-center gap-2 text-[11.5px]">
                  <span className="font-semibold text-foreground">{c.author}</span>
                  {c.review && <ReviewBadge state={c.review} />}
                  <span className="text-subtle">{relativeTime(isoToUnix(c.createdAt))}</span>
                </div>
                {c.body && <PullMarkdown pull={pull} idPrefix={`pr-c${i}-`} text={c.body} className="mt-1.5 px-0 py-0" />}
              </div>
            ))}
          </Section>
        )}
      </div>
    </div>
  );
}

function checkSummary(checks: PullCheck[]) {
  const failed = checks.filter((c) => c.state === "failure" || c.state === "cancelled" || c.state === "timed_out").length;
  const pending = checks.filter((c) => c.state === "pending").length;
  return failed ? `${failed} failing` : pending ? `${pending} pending` : "all passed";
}

function CheckIcon({ state }: { state: string }) {
  if (state === "success") return <Check className="size-3.5 shrink-0 text-added" />;
  if (state === "pending") return <CircleDashed className="size-3.5 shrink-0 animate-spin text-modified [animation-duration:3s]" />;
  if (state === "failure" || state === "cancelled" || state === "timed_out" || state === "action_required") return <X className="size-3.5 shrink-0 text-removed" />;
  return <MinusCircle className="size-3.5 shrink-0 text-subtle" />;
}

function ReviewBadge({ state }: { state: string }) {
  const map: Record<string, [string, string]> = {
    APPROVED: ["approved", "text-added"],
    CHANGES_REQUESTED: ["requested changes", "text-removed"],
    COMMENTED: ["reviewed", "text-muted-foreground"],
    DISMISSED: ["dismissed", "text-subtle"],
  };
  const [label, cls] = map[state] ?? [state.toLowerCase(), "text-muted-foreground"];
  return <span className={cn("text-[11px]", cls)}>{label}</span>;
}

function BranchChip({ name }: { name: string }) {
  return <span className="rounded-sm bg-primary/12 px-1.5 py-px font-mono text-[11px] text-primary">{name}</span>;
}
