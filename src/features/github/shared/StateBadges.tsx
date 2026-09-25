import { CircleCheck, CircleDot, CircleSlash, GitMerge, GitPullRequest, GitPullRequestClosed, GitPullRequestDraft } from "lucide-react";
import type { Issue, Pull } from "@/lib/api";
import { cn } from "@/lib/utils";

export function PullStateIcon({ pull, className }: { pull: Pick<Pull, "state" | "draft">; className?: string }) {
  if (pull.state === "merged") return <GitMerge className={cn("size-3.5 shrink-0 text-renamed", className)} />;
  if (pull.state === "closed") return <GitPullRequestClosed className={cn("size-3.5 shrink-0 text-removed", className)} />;
  if (pull.draft) return <GitPullRequestDraft className={cn("size-3.5 shrink-0 text-subtle", className)} />;
  return <GitPullRequest className={cn("size-3.5 shrink-0 text-added", className)} />;
}

export function IssueStateIcon({ issue, className }: { issue: Pick<Issue, "state" | "stateReason">; className?: string }) {
  if (issue.state === "open") return <CircleDot className={cn("size-3.5 shrink-0 text-added", className)} />;
  if (issue.stateReason === "not_planned") return <CircleSlash className={cn("size-3.5 shrink-0 text-subtle", className)} />;
  return <CircleCheck className={cn("size-3.5 shrink-0 text-renamed", className)} />;
}

export function PullStatePill({ pull }: { pull: Pick<Pull, "state" | "draft"> }) {
  return pull.state === "merged" ? (
    <StatePill label="Merged" className="bg-renamed-fill text-on-status" />
  ) : pull.state === "closed" ? (
    <StatePill label="Closed" className="bg-removed-fill text-on-status" />
  ) : pull.draft ? (
    <StatePill label="Draft" className={MUTED} />
  ) : (
    <StatePill label="Open" className="bg-added-fill text-on-status" />
  );
}

export function IssueStatePill({ issue }: { issue: Pick<Issue, "state" | "stateReason"> }) {
  return issue.state === "open" ? (
    <StatePill label="Open" className="bg-added-fill text-on-status" />
  ) : issue.stateReason === "not_planned" ? (
    <StatePill label="Closed as not planned" className={MUTED} />
  ) : (
    <StatePill label="Closed" className="bg-renamed-fill text-on-status" />
  );
}

const MUTED = "bg-elevated text-muted-foreground border border-border-strong";

function StatePill({ label, className }: { label: string; className: string }) {
  return <span className={cn("rounded-sm px-1.5 py-px text-[10.5px] font-semibold", className)}>{label}</span>;
}
