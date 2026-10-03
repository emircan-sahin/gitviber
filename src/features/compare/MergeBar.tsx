import { ChevronDown, GitMerge } from "lucide-react";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { api, type RepoStatus } from "@/lib/api";
import { plural } from "@/lib/format";
import type { ComparePoint } from "@/lib/repo/selection";
import { cn } from "@/lib/utils";
import { useAsyncValue } from "@/hooks/useAsyncValue";
import { useGitAction } from "@/hooks/useGitAction";
import { mergeInto } from "@/features/topbar/useRepoActions";

/** Merging Compare into the checked-out branch: how much comes in, and what would conflict, found before it's done. */
export function MergeBar({ head, headSha, tips, status, refresh }: { head: ComparePoint; headSha: string; tips: string; status: RepoStatus | null; refresh: () => unknown }) {
  const into = status?.branch;
  // Not into itself, nor while a merge or rebase waits on its own conflicts.
  const asks = !!into && !!status?.head && status.head !== headSha && !status.operation;
  const check = useAsyncValue(asks ? () => api.mergeCheck(head.ref).then((c) => ({ ref: head.ref, sha: headSha, c }), () => null) : null, [head.ref, headSha, tips, asks], null);
  const { busy, run } = useGitAction({ refresh });
  if (!asks || check?.sha !== headSha || !check.c.incoming) return null;
  const { incoming, unrelated, conflicts } = check.c;
  const go = (how: "ff" | "no-ff" | "squash") => void mergeInto(run, head.ref, how);
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
