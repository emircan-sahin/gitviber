import { ask } from "@tauri-apps/plugin-dialog";
import { Check, ChevronDown, CircleDashed, ExternalLink, GitBranch, GitMerge, Loader2, MinusCircle, RefreshCw, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { api, errorMessage, github, type MergeMethod, type Pull, type PullCheck, type PullDetail, type PullFiles } from "@/lib/api";
import type { Selection } from "@/lib/selection";
import { toast } from "@/lib/toast";
import { cn, relativeTime } from "@/lib/utils";
import { FileIcon } from "./FileIcon";
import { isoToUnix, notifyPullsChanged, PullStateIcon } from "./PullsPanel";
import { LineCounts, PathLabel, StatusLetter } from "./StatusBadge";

const METHODS: Record<MergeMethod, string> = { merge: "Create a merge commit", squash: "Squash and merge", rebase: "Rebase and merge" };

/** owner/name from a PR's html url, to tell same-repo PRs from forks. */
const repoOf = (url: string) => url.replace("https://github.com/", "").split("/pull/")[0];

export function PullView({ pull, onOpen }: { pull: Pull; onOpen: (s: Selection) => void }) {
  const [detail, setDetail] = useState<PullDetail | null>(null);
  const [files, setFiles] = useState<PullFiles | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const d = await github.detail(pull.number);
      setDetail(d);
      setError(null);
      // Fetching the PR's commits can take a moment; the page shows without waiting for it.
      github
        .files(d)
        .then(setFiles)
        .catch((e) => toast("error", "Could not load PR files", errorMessage(e)));
    } catch (e) {
      setError(errorMessage(e));
    }
  }, [pull.number]);

  useEffect(() => {
    load();
  }, [load]);

  const act = async (label: string, fn: () => Promise<unknown>, done: string) => {
    setBusy(label);
    try {
      const stopped = await fn();
      toast(stopped === true ? "info" : "success", stopped === true ? `${label} stopped on conflicts` : done, stopped === true ? "Resolve them in Changes, then push." : undefined);
      notifyPullsChanged();
      await load();
    } catch (e) {
      toast("error", `${label} failed`, errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  const d = detail;
  const p = d ?? pull;
  const sameRepo = p.headRepo === repoOf(p.url);

  const merge = async (method: MergeMethod) => {
    const ok = await ask(`${METHODS[method]}: #${p.number} into ${p.baseRef}?`, { title: "Merge pull request", okLabel: "Merge" });
    if (ok) await act("Merge", () => github.merge(p.number, method), `Merged #${p.number}`);
  };

  // GitHub can't merge it: bring the conflicts home. Check out the PR branch and merge
  // the base into it; the conflicts then open in Changes like any local merge.
  const resolveLocally = async () => {
    const ok = await ask(`Check out ${p.headRef} and merge origin/${p.baseRef} into it? Conflicts will open in Changes; push when done.`, {
      title: "Resolve conflicts locally",
      okLabel: "Start",
    });
    if (!ok) return;
    await act(
      "Merge",
      async () => {
        await github.checkout(p.number, p.headRef, sameRepo);
        await api.fetch();
        return api.merge(`origin/${p.baseRef}`);
      },
      `Merged ${p.baseRef} into ${p.headRef}; review and push`,
    );
  };

  if (error) {
    return <div className="p-8 text-center text-[12.5px] text-muted-foreground">{error}</div>;
  }

  return (
    <div className="min-h-0 flex-1 overflow-y-auto">
      <div className="mx-auto max-w-4xl px-6 py-5">
        <div className="flex items-start gap-3">
          <PullStateIcon pull={p} className="mt-1.5 size-4" />
          <div className="min-w-0 flex-1">
            <h1 className="text-[17px] leading-snug font-semibold select-text">
              {p.title} <span className="font-normal text-subtle">#{p.number}</span>
            </h1>
            <div className="mt-1.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-[12px] text-muted-foreground">
              <StatePill pull={p} />
              <span className="text-foreground/85">{p.author}</span>
              <span>wants to merge</span>
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
            <Button variant="secondary" size="sm" disabled={!!busy} onClick={() => act("Checkout", () => github.checkout(p.number, p.headRef, sameRepo), `Switched to ${sameRepo ? p.headRef : `pr/${p.number}`}`)}>
              <GitBranch /> Checkout
            </Button>
          )}
          <Button variant="secondary" size="sm" onClick={() => github.openUrl(p.url).catch((e) => toast("error", "Could not open", errorMessage(e)))}>
            <ExternalLink /> Open on GitHub
          </Button>
          <Button variant="ghost" size="icon-sm" onClick={load} disabled={!!busy}>
            <RefreshCw />
          </Button>
          {busy && (
            <span className="flex items-center gap-1.5 text-[12px] text-muted-foreground">
              <Loader2 className="size-3.5 animate-spin" /> {busy}…
            </span>
          )}
        </div>

        {d && p.state === "open" && <MergeBox detail={d} busy={!!busy} sameRepo={sameRepo} onMerge={merge} onResolve={resolveLocally} />}

        {d && d.checks.length > 0 && (
          <Section title="Checks" aside={checkSummary(d.checks)}>
            {d.checks.map((c, i) => (
              <div key={`${c.name}${i}`} className="flex h-7 items-center gap-2 px-3 text-[12px]">
                <CheckIcon state={c.state} />
                <span className="truncate">{c.name}</span>
                {c.url?.startsWith("https://github.com/") && (
                  <button onClick={() => github.openUrl(c.url!)} className="ml-auto text-[11px] text-subtle hover:text-foreground">
                    Details
                  </button>
                )}
              </div>
            ))}
          </Section>
        )}

        <Section title="Files changed" aside={files ? `${files.files.length}` : d ? `${d.changedFiles}` : undefined}>
          {!files && (
            <div className="flex items-center gap-2 px-3 py-2 text-[12px] text-subtle">
              <Loader2 className="size-3.5 animate-spin" /> Fetching the PR's commits…
            </div>
          )}
          {files?.files.map((f) => (
            <div
              key={f.path}
              role="button"
              onClick={() => onOpen({ kind: "pr-file", range: { number: p.number, base: files.base, head: files.head }, file: f })}
              className="flex h-7 cursor-pointer items-center gap-2 px-3 text-[12px] hover:bg-hover"
            >
              <FileIcon path={f.path} />
              <PathLabel path={f.path} className="flex-1" />
              <LineCounts file={f} />
              <StatusLetter status={f.status} />
            </div>
          ))}
        </Section>

        <Section title="Description">
          <Markdownish text={d?.body || ""} empty="No description provided." />
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
                {c.body && <Markdownish text={c.body} className="mt-1.5 px-0 py-0" />}
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
  sameRepo,
  onMerge,
  onResolve,
}: {
  detail: PullDetail;
  busy: boolean;
  sameRepo: boolean;
  onMerge: (m: MergeMethod) => void;
  onResolve: () => void;
}) {
  const st = detail.mergeableState;
  const conflicts = detail.mergeable === false || st === "dirty";
  const [tone, title, note] =
    detail.mergeable === null
      ? ["border-border", "Checking mergeability…", "GitHub is computing whether this can merge. Refresh in a moment."]
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
        sameRepo ? (
          <Button size="sm" disabled={busy} onClick={onResolve}>
            <GitMerge /> Resolve locally
          </Button>
        ) : (
          <span className="max-w-56 text-right text-[11.5px] text-muted-foreground">From a fork: resolve it in the fork's repository.</span>
        )
      ) : (
        <div className="flex">
          <Button size="sm" className="rounded-r-none" disabled={busy || detail.mergeable === null} onClick={() => onMerge("merge")}>
            <GitMerge /> Merge
          </Button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button size="sm" className="w-6 rounded-l-none border-l border-black/20 px-0" disabled={busy || detail.mergeable === null}>
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

function Section({ title, aside, children }: { title: string; aside?: string; children: React.ReactNode }) {
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

/** PR text as written. Rendered as plain text on purpose: nothing from GitHub becomes HTML. */
function Markdownish({ text, empty, className }: { text: string; empty?: string; className?: string }) {
  if (!text.trim()) return <div className={cn("px-3 py-2.5 text-[12px] text-subtle italic", className)}>{empty}</div>;
  return <div className={cn("px-3 py-2.5 text-[12.5px] leading-relaxed whitespace-pre-wrap text-foreground/90 select-text", className)}>{text.replace(/\r\n/g, "\n")}</div>;
}
