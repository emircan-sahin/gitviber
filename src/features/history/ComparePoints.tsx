import { ArrowLeftRight, GitCompareArrows, X } from "lucide-react";
import { useMemo } from "react";
import { api, errorMessage, type FileChange } from "@/lib/api";
import type { Selection } from "@/lib/repo/selection";
import { useAsyncValue } from "@/hooks/useAsyncValue";
import { sumLines } from "@/features/changes/changeList";
import { SectionBtn } from "@/features/changes/ChangeRows";
import { useReviewFiles } from "@/features/changes/BranchReview";
import { FileList } from "./CompareHistory";
import type { Points } from "@/lib/repo/compareMark";

type Rows = (Selection & { file: FileChange })[];

/** Every file `points` differ in, read again on every change on disk (`revision`) when the working tree is one side. */
export function ComparePoints({ points, revision, activeKey, onOpen, onSwap, onClose }: {
  points: Points;
  revision: number;
  activeKey: string | null;
  onOpen: (s: Selection, pin?: boolean) => void;
  onSwap: () => void;
  onClose: () => void;
}) {
  const { base, head } = points;
  const key = `${base.sha}..${head?.sha ?? ""}`;
  const worktree = useReviewFiles(head ? null : base.sha, api.compareWorktree, revision, true);
  const { review } = worktree;
  const worktreeRows = useMemo<Rows | null>(() => review && review.files.map((file) => ({ kind: "branch", base: review.base, label: base.label, file, fixed: true })), [review, base.label]);
  // Two commits never change: read once.
  const range = useAsyncValue<{ key: string; rows: Rows } | { key: string; error: string } | null>(
    head
      ? () =>
          api.rangeFiles(base.sha, head.sha).then(
            (files) => {
              const range = { label: `${base.label}..${head.label}`, base: base.sha, head: head.sha };
              return { key, rows: files.map((file) => ({ kind: "pr-file" as const, range, file })) };
            },
            (e) => ({ key, error: errorMessage(e) }),
          )
      : null,
    [key],
    null,
  );
  const shown: { rows: Rows } | { error: string } | null = head
    ? range?.key === key
      ? range
      : null
    : worktree.error
      ? { error: worktree.error }
      : worktreeRows && { rows: worktreeRows };
  const { add, del } = sumLines(shown && "rows" in shown ? shown.rows.map((r) => r.file) : []);
  const button = "flex size-4 shrink-0 items-center justify-center rounded-sm text-subtle hover:bg-hover hover:text-foreground focus-visible:bg-hover focus-visible:text-foreground";

  return (
    <div className="flex h-full flex-col">
      <div className="flex shrink-0 items-center gap-1.5 border-b border-border px-2.5 py-1 text-[11px] text-muted-foreground">
        <GitCompareArrows className="size-3 shrink-0 text-subtle" />
        <span className="min-w-0 truncate">
          <span className="font-mono text-foreground">{base.label}</span> → <span className={head ? "font-mono text-foreground" : "text-foreground"}>{head?.label ?? "working tree"}</span>
        </span>
        {head && (
          <button aria-label="Swap sides" title="Swap sides" onClick={onSwap} className={`ml-auto ${button}`}>
            <ArrowLeftRight className="size-3" />
          </button>
        )}
        <button aria-label="Stop comparing" title="Stop comparing" onClick={onClose} className={`${head ? "" : "ml-auto "}${button}`}>
          <X className="size-3" />
        </button>
      </div>
      {shown && "rows" in shown && shown.rows.length > 0 && (
        <div className="flex h-6 shrink-0 items-center gap-2 border-b border-border px-4 text-[10.5px] text-subtle">
          <span className="font-semibold tracking-[0.08em] uppercase">Changed files</span>
          <span className="font-mono text-muted-foreground">{shown.rows.length}</span>
          {head && <SectionBtn onClick={() => onOpen({ kind: "changes", list: "range", range: { label: `${base.label}..${head.label}`, base: base.sha, head: head.sha } }, true)}>Open All</SectionBtn>}
          <span className="ml-auto font-mono">
            <span className="text-added">+{add}</span> <span className="text-removed">-{del}</span>
          </span>
        </div>
      )}
      <div className="min-h-0 flex-1 overflow-y-auto py-0.5">
        {!shown ? (
          <div className="py-1 pl-4 text-[11.5px] text-subtle">Loading…</div>
        ) : "error" in shown ? (
          <div className="px-4 py-2 text-[11.5px] text-muted-foreground">{shown.error}</div>
        ) : !shown.rows.length ? (
          <div className="py-1 pl-4 text-[11.5px] text-subtle">No differences</div>
        ) : (
          <FileList rows={shown.rows} activeKey={activeKey} onOpen={onOpen} />
        )}
      </div>
    </div>
  );
}
