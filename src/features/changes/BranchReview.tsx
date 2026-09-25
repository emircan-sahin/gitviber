import { GitCompareArrows, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Select } from "@/components/ui/select";
import { Windowed } from "@/components/Windowed";
import { api, type Branch, errorMessage, type FileChange } from "@/lib/api";
import { shortRef } from "@/lib/git/refs";
import { type Selection, selectionKey } from "@/lib/repo/selection";
import { useListNav } from "@/lib/ui/useListNav";
import { RefOptions } from "@/features/branches/BaseSelect";
import { sumLines } from "./changeList";
import { ReviewSummary, Row } from "./ChangeRows";

export type BranchChange = Selection & { kind: "branch" };

/** A branch review as loaded: the merge base, and the files from it to the working tree. */
export type ReviewFiles = { base: string; files: FileChange[] };

/**
 * HEAD's branch against `ref` (a full ref; "" while none is picked): its commits and uncommitted
 * work as one list, read again on every change on disk (`revision`) while `live`.
 */
export function useBranchReview(ref: string | null, revision: number, live: boolean) {
  const [state, setState] = useState<{ ref: string; review: ReviewFiles | null; error: string | null } | null>(null);
  const wanted = useRef(ref);
  wanted.current = ref;
  // A read costs a few statuses: one at a time, and changes during it make one more once it lands.
  const running = useRef(false);
  const again = useRef(false);
  const load = useRef(() => {});
  load.current = () => {
    const at = wanted.current;
    if (!at) return;
    if (running.current) {
      again.current = true;
      return;
    }
    running.current = true;
    const land = (review: ReviewFiles | null, error: string | null) => {
      running.current = false;
      setState({ ref: at, review, error });
      if (again.current) {
        again.current = false;
        load.current();
      }
    };
    api.branchReview(at).then(
      (r) => land(r, null),
      (e) => land(null, errorMessage(e)),
    );
  };
  useEffect(() => {
    if (live) load.current();
  }, [ref, revision, live]);
  const shown = ref && state?.ref === ref ? state : null;
  const review = shown?.review ?? null;
  const rows = useMemo<BranchChange[]>(() => (review && ref ? review.files.map((file) => ({ kind: "branch", base: review.base, label: shortRef(ref), file })) : []), [review, ref]);
  return { review, rows, error: shown?.error ?? null, loading: !!ref && !shown };
}

const ROW_HEIGHT = 26;

interface Props {
  /** The full ref reviewed against; "" until one is picked. */
  base: string;
  data: ReturnType<typeof useBranchReview>;
  branches: Branch[];
  activeKey: string | null;
  onOpen: (s: Selection, pin?: boolean) => void;
  onHover: (s: Selection) => void;
  viewed: (s: Selection) => boolean;
  setViewed: (s: Selection[], on: boolean) => void;
  onBase: (ref: string) => void;
  onClose: () => void;
}

/** Changes in place of the uncommitted list: what the branch changed since it left `base`, committed or not. */
export function BranchReview({ base, data, branches, activeKey, onOpen, onHover, viewed, setViewed, onBase, onClose }: Props) {
  const nav = useListNav({ activeKey });
  const { rows, error, loading } = data;
  const { add, del } = sumLines(rows.map((r) => r.file));
  const reviewed = rows.filter(viewed).length;
  const tabStop = rows.some((r) => selectionKey(r) === activeKey) ? activeKey : rows[0] && selectionKey(rows[0]);
  const listed = branches.some((b) => `refs/${b.remote ? "remotes" : "heads"}/${b.name}` === base);
  const empty = !base ? "Pick a branch to review against." : (error ?? (loading ? "Loading…" : rows.length ? null : `No changes since ${shortRef(base)}.`));

  return (
    <div className="flex h-full flex-col">
      <div className="flex shrink-0 items-center gap-1.5 border-b border-border px-2.5 py-1 text-[11px] text-muted-foreground">
        <GitCompareArrows className="size-3 shrink-0 text-subtle" />
        <span className="shrink-0">Review against</span>
        <Select aria-label="Branch to review against" value={base} onChange={(e) => onBase(e.target.value)} className="h-5 min-w-0 flex-1 px-1 font-mono text-[11px]">
          {!listed && <option value={base}>{base ? shortRef(base) : "Pick a branch…"}</option>}
          <RefOptions branches={branches} />
        </Select>
        <button
          aria-label="Back to uncommitted changes"
          title="Back to uncommitted changes"
          onClick={onClose}
          className="flex size-4 shrink-0 items-center justify-center rounded-sm text-subtle hover:bg-hover hover:text-foreground focus-visible:bg-hover focus-visible:text-foreground"
        >
          <X className="size-3" />
        </button>
      </div>
      {rows.length > 0 && <ReviewSummary files={rows.length} add={add} del={del} reviewed={reviewed} />}
      <div ref={nav.ref} onKeyDown={nav.onKeyDown} onFocus={nav.onFocus} data-list-nav="" className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto pb-2 outline-none">
        {empty ? (
          <div className="px-4 py-6 text-center text-[12px] text-muted-foreground">{empty}</div>
        ) : (
          <Windowed
            count={rows.length}
            height={ROW_HEIGHT}
            keep={[activeKey, tabStop].map((k) => rows.findIndex((r) => selectionKey(r) === k))}
            render={(i) => {
              const sel = rows[i];
              const key = selectionKey(sel);
              const isViewed = viewed(sel);
              return (
                <Row
                  key={key}
                  sel={sel}
                  active={activeKey === key}
                  selected={activeKey === key}
                  dim={false}
                  tabStop={tabStop === key}
                  viewed={isViewed}
                  checkLabel={`Mark as ${isViewed ? "not viewed" : "viewed"}`}
                  onClick={() => onOpen(sel)}
                  onOpen={onOpen}
                  onHover={onHover}
                  onToggleViewed={() => setViewed([sel], !isViewed)}
                />
              );
            }}
          />
        )}
      </div>
    </div>
  );
}
