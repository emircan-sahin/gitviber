import { ask } from "@tauri-apps/plugin-dialog";
import { Check, ChevronDown, CircleDashed, ExternalLink, GitBranch, GitMerge, GitPullRequest, GitPullRequestClosed, Image as ImageIcon, Loader2, MessageSquare, MinusCircle, RefreshCw, X } from "lucide-react";
import { type ComponentProps, useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Components } from "react-markdown";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Textarea } from "@/components/ui/textarea";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { accessFor, api, errorMessage, fullName, github, type MergeMethod, type Pull, type PullCheck, type PullDetail, repoOf, type ReviewEvent } from "@/lib/api";
import { listIsBehind, revalidate, useGitHubData } from "@/lib/githubCache";
import { isGitHubHosted, markdownLink } from "@/lib/markdown";
import type { Selection } from "@/lib/selection";
import { toast } from "@/lib/toast";
import { tracked, undoAction } from "@/lib/undo";
import { cn, relativeTime } from "@/lib/utils";
import { FileIcon } from "./FileIcon";
import { followLink, MarkdownBody } from "./MarkdownView";
import { CopyLinkButton, isoToUnix, notifyPullsChanged, openOnGitHub, PullStateIcon } from "./PullsPanel";
import { LineCounts, PathLabel, StatusLetter } from "./StatusBadge";

const METHODS: Record<MergeMethod, string> = { merge: "Create a merge commit", squash: "Squash and merge", rebase: "Rebase and merge" };

export function PullView({ pull, onOpen }: { pull: Pull; onOpen: (s: Selection) => void }) {
  const [busy, setBusy] = useState<string | null>(null);
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
  // Same cache entry as the PRs panel's, so this is normally already loaded.
  const account = useGitHubData("account", github.account, 600_000).data ?? null;
  const load = () => detail.refresh(true);

  /** true when the action went through. */
  const act = async (label: string, fn: () => Promise<unknown>, done: string) => {
    setBusy(label);
    try {
      const [stopped, entry] = await tracked(fn);
      // A checkout moves HEAD; the repo watcher refreshes the rest of the window.
      if (stopped === true) toast("info", `${label} stopped on conflicts`, "Resolve them in Changes, then push.");
      else toast("success", done, undefined, undoAction(entry, () => {}));
      notifyPullsChanged();
      await load();
      return true;
    } catch (e) {
      toast("error", `${label} failed`, errorMessage(e));
      return false;
    } finally {
      setBusy(null);
    }
  };

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
  // Where Checkout lands: the original's PRs get their own names (pr/<owner>/<n>).
  const checkoutBranch = sameRepo ? p.headRef : inOrigin ? `pr/${p.number}` : `pr/${repoOf(p.url).split("/")[0]}/${p.number}`;
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
      <div className="mx-auto max-w-4xl px-6 py-5">
        <div className="flex items-start gap-3">
          <PullStateIcon pull={p} className="mt-1.5 size-4" />
          <div className="min-w-0 flex-1">
            <h1 className="text-[17px] leading-snug font-semibold select-text">
              {p.title} <span className="font-normal text-subtle">#{p.number}</span>
            </h1>
            <div className="mt-1.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-[12px] text-muted-foreground">
              <StatePill pull={p} />
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
                    {d.commits} commit{d.commits === 1 ? "" : "s"}
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
          {p.state === "open" && <ReviewButton own={own} counts={!!access?.push} busy={!!busy} onSubmit={(event, body) => act("Review", () => github.review(target, p.number, event, body), REVIEWS[event].done)} />}
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
              onClick={() => onOpen({ kind: "pr-file", range: { number: p.number, base: files.data!.base, head: files.data!.head }, file: f })}
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

function MergeBox({
  detail,
  busy,
  canMerge,
  canResolve,
  onMerge,
  onResolve,
}: {
  detail: PullDetail;
  busy: boolean;
  /** Write access to the PR's repository. */
  canMerge: boolean;
  /** The PR's branch is on origin, where a pushed fix updates it. */
  canResolve: boolean;
  onMerge: (m: MergeMethod) => void;
  onResolve: () => void;
}) {
  const st = detail.mergeableState;
  const conflicts = detail.mergeable === false || st === "dirty";
  const [tone, title, note] =
    detail.mergeable === null
      ? ["border-border", "Checking mergeability…", "GitHub is computing whether this can merge. Refresh in a moment."]
      : detail.draft && !conflicts
        ? ["border-border", "This is a draft", "GitHub won't merge it until it's marked ready for review."]
        : conflicts
        ? ["border-conflict/60", `This branch has conflicts with ${detail.baseRef}`, "Resolve them locally: GitViber merges the base into this branch and opens the conflicts."]
        : st === "blocked"
          ? ["border-modified/60", "Merging is blocked", "Required reviews or checks haven't passed yet."]
          : st === "behind"
            ? ["border-modified/60", `This branch is behind ${detail.baseRef}`, "It can still merge, but the base has newer commits."]
            : st === "unstable"
              ? ["border-modified/60", "Checks are failing or pending", "You can still merge if the repository allows it."]
              : ["border-added/60", "Ready to merge", "No conflicts with the base branch."];
  return (
    <div className={cn("mt-4 flex items-center gap-3 rounded-md border bg-panel p-3", tone)}>
      <div className="min-w-0 flex-1">
        <div className="text-[12.5px] font-semibold">{title}</div>
        <div className="mt-0.5 text-[11.5px] text-muted-foreground">{note}</div>
      </div>
      {conflicts ? (
        // A fork's branch lives in another repo; pushing the fix here would go to the wrong place.
        canResolve ? (
          <Button size="sm" disabled={busy} onClick={onResolve}>
            <GitMerge /> Resolve locally
          </Button>
        ) : (
          <span className="max-w-56 text-right text-[11.5px] text-muted-foreground">From a fork: resolve it in the fork's repository.</span>
        )
      ) : !canMerge ? (
        <span className="max-w-56 text-right text-[11.5px] text-muted-foreground">Only people with write access can merge.</span>
      ) : (
        <div className="flex">
          <Button size="sm" className="rounded-r-none" disabled={busy || detail.mergeable === null || detail.draft} onClick={() => onMerge("merge")}>
            <GitMerge /> Merge
          </Button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button size="sm" className="w-6 rounded-l-none border-l border-black/20 px-0" disabled={busy || detail.mergeable === null || detail.draft}>
                <ChevronDown className="size-3" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-56">
              <DropdownMenuLabel>Merge method</DropdownMenuLabel>
              {(Object.keys(METHODS) as MergeMethod[]).map((m) => (
                <DropdownMenuItem key={m} onSelect={() => onMerge(m)}>
                  {METHODS[m]}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      )}
    </div>
  );
}

const REVIEWS: Record<ReviewEvent, { label: string; note: string; done: string }> = {
  COMMENT: { label: "Comment", note: "General feedback without explicit approval.", done: "Review submitted" },
  APPROVE: { label: "Approve", note: "Give your approval to merge these changes.", done: "Approved" },
  REQUEST_CHANGES: { label: "Request changes", note: "Feedback that must be addressed before merging.", done: "Changes requested" },
};

/** GitHub's "Review changes": a verdict plus a note, which GitHub requires unless approving. */
/** `counts`: write access. Anyone may review, but GitHub only counts a writer's verdict toward merging. */
function ReviewButton({
  own,
  counts,
  busy,
  onSubmit,
}: {
  own: boolean;
  counts: boolean;
  busy: boolean;
  onSubmit: (event: ReviewEvent, body: string) => Promise<boolean>;
}) {
  const [open, setOpen] = useState(false);
  const [pick, setPick] = useState<ReviewEvent>("APPROVE");
  // GitHub refuses approving or requesting changes on your own pull request.
  const event = own ? "COMMENT" : pick;
  const [body, setBody] = useState("");
  const ready = event === "APPROVE" || body.trim() !== "";
  const submit = async () => {
    setOpen(false);
    // A rejected review keeps its text for another try.
    if (await onSubmit(event, body.trim())) setBody("");
  };
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button variant="secondary" size="sm" disabled={busy}>
          <MessageSquare /> Review
        </Button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-80 p-3">
        <Textarea autoFocus value={body} onChange={(e) => setBody(e.target.value)} placeholder="Leave a comment (markdown)" rows={4} className="text-[12px]" />
        <div className="mt-2 space-y-1.5">
          {(Object.keys(REVIEWS) as ReviewEvent[]).map((e) => {
            const disabled = own && e !== "COMMENT";
            return (
              <label key={e} className={cn("flex items-start gap-2 text-[12px]", disabled ? "opacity-50" : "cursor-pointer")}>
                <input type="radio" name="review" checked={event === e} disabled={disabled} onChange={() => setPick(e)} className="mt-0.5 accent-primary" />
                <span>
                  <span className="font-medium">{REVIEWS[e].label}</span>
                  <span className="block text-[11px] text-muted-foreground">{disabled
                      ? "Not available on your own pull request."
                      : !counts && e !== "COMMENT"
                        ? `${REVIEWS[e].note} Without write access it won't count toward merging.`
                        : REVIEWS[e].note}</span>
                </span>
              </label>
            );
          })}
        </div>
        <div className="mt-3 flex justify-end">
          <Button size="sm" disabled={busy || !ready} onClick={submit}>
            Submit review
          </Button>
        </div>
      </PopoverContent>
    </Popover>
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

function StatePill({ pull }: { pull: Pick<Pull, "state" | "draft"> }) {
  const [label, cls] =
    pull.state === "merged"
      ? ["Merged", "bg-renamed-fill text-on-status"]
      : pull.state === "closed"
        ? ["Closed", "bg-removed-fill text-on-status"]
        : pull.draft
          ? ["Draft", "bg-elevated text-muted-foreground border border-border-strong"]
          : ["Open", "bg-added-fill text-on-status"];
  return <span className={cn("rounded-sm px-1.5 py-px text-[10.5px] font-semibold", cls)}>{label}</span>;
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

export function Section({ title, aside, children }: { title: string; aside?: string; children: React.ReactNode }) {
  return (
    <div className="mt-5 overflow-hidden rounded-md border border-border">
      <div className="flex h-8 items-center border-b border-border bg-panel px-3 text-[10.5px] font-semibold tracking-[0.08em] text-subtle uppercase">
        {title}
        {aside && <span className="ml-auto font-mono tracking-normal normal-case">{aside}</span>}
      </div>
      <div>{children}</div>
    </div>
  );
}

/** A PR or issue description or comment, rendered like GitHub does (see MarkdownBody for what's allowed). */
export function PullMarkdown({ pull, idPrefix, text, empty, className }: { pull: Pick<Pull, "url" | "number">; idPrefix: string; text: string; empty?: string; className?: string }) {
  // Keyed on the tab's PR, not its refreshed detail: new components would remount every image.
  const components = useMemo<Components>(() => {
    // Relative links in PR or issue text are relative to its page, as on github.com.
    const absolute = (href: string) => {
      try {
        return new URL(href, pull.url).href;
      } catch {
        return "";
      }
    };
    return {
      a: markdownLink((href) => followLink(href, (href) => followLink(absolute(href), () => {}), idPrefix)),
      img: ({ src, alt, width, height, title }) => (typeof src === "string" && absolute(src) ? <GitHubImage src={absolute(src)} pull={pull} alt={alt} width={width} height={height} title={title} /> : null),
    };
  }, [pull, idPrefix]);
  if (!text.trim()) return empty ? <div className={cn("px-3 py-2.5 text-[12px] text-subtle italic", className)}>{empty}</div> : null;
  return (
    <div className={cn("markdown px-3 py-2.5 select-text", className)}>
      <MarkdownBody text={text} components={components} idPrefix={idPrefix} repo={pull.url.split(/\/(?:pull|issues)\//)[0]} />
    </div>
  );
}

// Attachments uploaded to GitHub: github.com/user-attachments/assets/<id>, or the older
// github.com/<owner>/<repo>/assets/<n>/<id>.
const ATTACHMENT = /^https:\/\/github\.com\/(?:user-attachments\/assets|[^/]+\/[^/]+\/assets\/\d+)\/([0-9a-f-]{36})(?:[?#]|$)/i;

/**
 * Public repos' attachments load as they are. A private repo's need a github.com login the
 * webview doesn't have; on failure, swap in the signed link the API hands out instead.
 */
function GitHubImage({ src, pull, ...props }: { src: string; pull: Pick<Pull, "url" | "number"> } & Omit<ComponentProps<"img">, "src">) {
  const [signed, setSigned] = useState<string | null>(null);
  if (!isGitHubHosted(src)) return <ExternalImage src={src} {...props} />;
  const id = ATTACHMENT.exec(src)?.[1];
  const onError = () => {
    if (!id || signed) return;
    // Signed links expire after 5 minutes; reuse a lookup for 4.
    revalidate(`attachments:${pull.url}`, () => github.attachments(repoOf(pull.url), pull.number), 240_000)
      .then((urls) => urls[id] && setSigned(urls[id]))
      .catch(() => {});
  };
  return <img src={signed ?? src} onError={onError} {...props} />;
}

/**
 * An image hosted outside GitHub loads only on click: fetching it tells that host your IP
 * and when you read the PR. github.com hides both behind its image proxy; GitViber has none.
 */
function ExternalImage({ src, alt, ...props }: { src: string } & Omit<ComponentProps<"img">, "src">) {
  const [load, setLoad] = useState(false);
  if (load) return <img src={src} alt={alt} {...props} />;
  let host = src;
  try {
    host = new URL(src).hostname;
  } catch {
    // Shown as written.
  }
  return (
    <button
      type="button"
      title={src}
      onClick={() => setLoad(true)}
      className="inline-flex max-w-full items-center gap-1.5 rounded-md border border-border bg-panel px-2 py-1 align-middle text-[12px] text-muted-foreground hover:text-foreground focus-visible:text-foreground"
    >
      <ImageIcon className="size-3.5 shrink-0" />
      <span className="truncate">
        {alt ? `${alt} · ` : ""}Load image from {host}
      </span>
    </button>
  );
}
