import { ChevronDown, GitMerge, MessageSquare } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import type { MergeMethod, PullDetail, ReviewEvent } from "@/lib/api";
import { cn } from "@/lib/utils";
import { MarkdownInput } from "@/features/github/shared/MarkdownInput";
import type { MarkdownHome } from "@/features/github/shared/GitHubMarkdown";
import { METHODS, REVIEWS } from "./actionLabels";

export function MergeBox({
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

/** GitHub's "Review changes": a verdict plus a note, which GitHub requires unless approving. */
/** `counts`: write access. Anyone may review, but GitHub only counts a writer's verdict toward merging. */
export function ReviewButton({
  pull,
  own,
  counts,
  busy,
  onSubmit,
}: {
  pull: MarkdownHome;
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
        <MarkdownInput pull={pull} autoFocus value={body} onChange={(e) => setBody(e.target.value)} placeholder="Leave a comment (markdown)" rows={4} className="text-[12px]" />
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
