// A whole Changes list as one scroll of diffs, as GitHub's Files changed shows a pull request.
// Monaco is one editor per view (MonacoView), so these are drawn as HTML: only the files near the
// screen load, highlight and render, and a file scrolled far away keeps just its height.
import { Check, ChevronDown, ChevronsDownUp, ChevronsUpDown, Files } from "lucide-react";
import { type RefObject, useEffect, useLayoutEffect, useMemo, useReducer, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Tip } from "@/components/ui/tooltip";
import { FileIcon } from "@/components/FileIcon";
import { LineCounts, PathLabel, StatusPill } from "@/components/StatusBadge";
import type { RepoStatus } from "@/lib/api";
import { type Gap, shownRows } from "@/lib/git/diffHunks";
import { useCommands } from "@/lib/commands/keybindings";
import { codeWantsFocus } from "@/lib/ui/panels";
import { type ChangeList, type Selection, selectionKey, selectionPath } from "@/lib/repo/selection";
import { diffWhitespace, useSettings } from "@/lib/settings";
import { cn } from "@/lib/utils";
import { sumLines } from "@/features/changes/changeList";
import type { BranchChange } from "@/features/changes/BranchReview";
import { CONTEXT, FOLD_MIN, FOLD_REVEAL } from "./editorOptions";
import { usePair } from "./diffPairs";
import { mediaKind } from "./MediaView";
import { diffNote, placeholderFor } from "./placeholders";
import { UnifiedDiff } from "./UnifiedDiff";
import { type FileMemo, fileMemo, scrolls } from "./stackedMemo";

type ListFile = Selection & { kind: "unstaged" | "staged" | "branch" };

/** More lines than this in one file wait for a click, as GitHub's large diffs do: a lockfile would hold up the rest. */
const LARGE = 1500;
/** How far off screen a file starts loading, so it's drawn by the time it scrolls in. */
const AHEAD = "1200px 0px";

interface Props {
  list: ChangeList;
  status: RepoStatus | null;
  /** The branch review's files as last loaded; null while there's none. */
  branchRows: BranchChange[] | null;
  revision: number;
  viewed: (sel: Selection) => boolean;
  toggleViewed: (sel: Selection) => void;
  onOpen: (s: Selection) => void;
}

function filesOf(list: ChangeList, status: RepoStatus | null, branchRows: BranchChange[] | null): ListFile[] | null {
  if (list === "branch") return branchRows;
  if (!status) return null;
  // Nested repos have no diff, as in the list.
  return list === "staged" ? status.staged.map((file) => ({ kind: "staged", file })) : status.unstaged.filter((f) => !f.nested).map((file) => ({ kind: "unstaged", file }));
}

/** The stacked view on show: the review keys walk its files instead of opening one. */
interface StackedView {
  step(dir: 1 | -1): void;
  toggleViewed(): void;
}
let onShow: StackedView | null = null;
export const stackedView = () => onShow;

/** The files' sections in `el`, the stacked view's scroller. */
const blocksIn = (el: HTMLElement | null) => [...(el?.querySelectorAll<HTMLElement>("[data-file]") ?? [])];

/** The index of the file whose header is at the top of `el`'s view; -1: none. */
function topBlock(el: HTMLElement | null) {
  const all = blocksIn(el);
  const top = (el?.scrollTop ?? 0) + 1;
  let i = 0;
  while (i + 1 < all.length && all[i + 1].offsetTop <= top) i++;
  return all.length ? i : -1;
}

export function AllChanges({ list, status, branchRows, revision, viewed, toggleViewed, onOpen }: Props) {
  const files = useMemo(() => filesOf(list, status, branchRows), [list, status, branchRows]);
  const root = status?.root ?? "";
  const memo = (sel: ListFile) => fileMemo(`${root}\0${selectionKey(sel)}`);
  // Memos change outside React: this draws what they now say.
  const [, redraw] = useReducer((n: number) => n + 1, 0);
  // Opened or closed by hand, else closed once viewed (staged ones always count as viewed, and stay open).
  const isOpen = (sel: ListFile) => !(memo(sel).shut ?? (sel.kind !== "staged" && viewed(sel)));
  const scroller = useRef<HTMLDivElement>(null);

  // What a file's diff depends on: its own content, and the index under unstaged changes or HEAD
  // under staged ones. Each file is read again only when that changes, not on every change on disk.
  const stagedOids = useMemo(() => new Map(status?.staged.map((f) => [f.path, f.oid])), [status]);
  const revisionOf = (sel: ListFile) => {
    const { file } = sel;
    const sig = [file.status, file.oid ?? `${file.additions}:${file.deletions}`, sel.kind === "unstaged" ? stagedOids.get(file.path) : sel.kind === "staged" ? status?.head : ""].join(":");
    const m = memo(sel);
    if (m.sig !== sig) Object.assign(m, { sig, rev: revision });
    return m.rev!;
  };

  // Back at the file that was at the top, as far into it, once the files are there; each keeps its
  // height from before, so it's the same place.
  const scrollKey = `${root}\0${list}`;
  const restored = useRef(false);
  useLayoutEffect(() => {
    const el = scroller.current;
    if (restored.current || !el || !files?.length) return;
    restored.current = true;
    const at = scrolls.get(scrollKey);
    const block = at && blocksIn(el).find((b) => b.dataset.file === at.file);
    if (block) el.scrollTop = block.offsetTop + at.offset;
  });
  useLayoutEffect(() => {
    const el = scroller.current;
    return () => {
      const block = blocksIn(el)[topBlock(el)];
      if (el && block && restored.current) scrolls.set(scrollKey, { file: block.dataset.file!, offset: el.scrollTop - block.offsetTop });
    };
  }, [scrollKey]);

  const blocks = () => blocksIn(scroller.current);
  const current = () => topBlock(scroller.current);
  const scrollTo = (el: HTMLElement | undefined) => {
    if (el && scroller.current) scroller.current.scrollTop = el.offsetTop;
  };
  const markViewed = (sel: ListFile) => {
    const key = selectionKey(sel);
    delete memo(sel).shut;
    toggleViewed(sel);
    // Closing the file being read would leave the view somewhere in the next one: keep its header in view.
    const el = blocks().find((b) => b.dataset.file === key);
    requestAnimationFrame(() => el && scroller.current && el.offsetTop < scroller.current.scrollTop && scrollTo(el));
  };
  const setAll = (open: boolean) => {
    for (const f of files ?? []) memo(f).shut = !open;
    redraw();
  };

  // J/K walk the files here, V marks the one on top and C writes a note on the line under the
  // pointer, else the first changed one in view: Workspace hands this view the review keys.
  const step = (dir: 1 | -1) => {
    if (files?.length) scrollTo(blocks()[Math.max(0, Math.min(files.length - 1, current() + dir))]);
  };
  const toggleTop = () => {
    const sel = files?.[current()];
    if (sel && sel.kind !== "staged") markViewed(sel);
  };
  const addNote = () => {
    const el = scroller.current;
    if (!el) return;
    const { top, bottom } = el.getBoundingClientRect();
    // Below the file's sticky header, which covers the line at the very top.
    const seen = (row: HTMLElement) => row.getBoundingClientRect().top >= top + 32 && row.getBoundingClientRect().bottom <= bottom;
    const rows = [...el.querySelectorAll<HTMLElement>("[data-line]")];
    const row = el.querySelector<HTMLElement>("[data-line]:hover") ?? rows.find((r) => r.dataset.line !== "0" && seen(r)) ?? rows.find(seen);
    row?.querySelector<HTMLButtonElement>("button[data-add-note]")?.click();
  };
  const keys = useRef({ step, toggleTop, addNote });
  keys.current = { step, toggleTop, addNote };
  useEffect(() => {
    const view: StackedView = { step: (dir) => keys.current.step(dir), toggleViewed: () => keys.current.toggleTop() };
    onShow = view;
    return () => {
      if (onShow === view) onShow = null;
    };
  }, []);
  useCommands({ "review.addNote": files?.length ? () => keys.current.addNote() : undefined });
  // Opened from the code view (a tab switch, quick open): it takes the keys, as a file would.
  useEffect(() => {
    if (codeWantsFocus()) scroller.current?.focus();
  }, []);

  const { add, del } = sumLines((files ?? []).map((f) => f.file));
  const reviewed = (files ?? []).filter(viewed).length;
  const empty = !files ? (list === "branch" ? "Review the branch from Changes to see its files here." : "Loading…") : files.length ? null : list === "staged" ? "Nothing staged." : "No changes.";

  return (
    <>
      <div className="flex h-9 shrink-0 items-center gap-2 border-b border-border pr-2 pl-3 text-[12px]">
        <Files className="size-4 shrink-0 text-subtle" />
        <span className="truncate font-medium">{selectionPath({ kind: "changes", list })}</span>
        {!!files?.length && (
          <>
            <span className="shrink-0 text-muted-foreground">{files.length === 1 ? "1 file" : `${files.length} files`}</span>
            <LineCounts file={{ additions: add, deletions: del }} />
            {list !== "staged" && (
              <span className="shrink-0 text-muted-foreground">
                <span className={cn("font-semibold", reviewed === files.length ? "text-added" : "text-foreground")}>{reviewed}</span>/{files.length} reviewed
              </span>
            )}
            <div className="ml-auto flex shrink-0 items-center gap-1">
              <Tip label="Expand all files">
                <Button variant="ghost" size="icon-sm" aria-label="Expand all files" onClick={() => setAll(true)}>
                  <ChevronsUpDown />
                </Button>
              </Tip>
              <Tip label="Collapse all files">
                <Button variant="ghost" size="icon-sm" aria-label="Collapse all files" onClick={() => setAll(false)}>
                  <ChevronsDownUp />
                </Button>
              </Tip>
            </div>
          </>
        )}
      </div>
      <div ref={scroller} data-code-scroll tabIndex={0} className="relative min-h-0 flex-1 overflow-y-auto outline-none">
        {empty ? (
          <div className="flex h-full items-center justify-center p-6 text-[12.5px] text-muted-foreground">{empty}</div>
        ) : (
          files!.map((sel) => {
            const open = isOpen(sel);
            return (
              <FileBlock
                key={selectionKey(sel)}
                sel={sel}
                memo={memo(sel)}
                revision={revisionOf(sel)}
                open={open}
                viewed={viewed(sel)}
                onToggleOpen={() => {
                  memo(sel).shut = open;
                  redraw();
                }}
                onToggleViewed={() => markViewed(sel)}
                onOpen={() => onOpen(sel)}
              />
            );
          })
        )}
      </div>
    </>
  );
}

/** Whether `el` is on screen or about to be, in the view's scroller. */
function useNear(el: RefObject<HTMLElement | null>) {
  const [near, setNear] = useState(false);
  useEffect(() => {
    const node = el.current;
    if (!node) return;
    const io = new IntersectionObserver(([e]) => setNear(e.isIntersecting), { root: node.closest("[data-code-scroll]"), rootMargin: AHEAD });
    io.observe(node);
    return () => io.disconnect();
  }, [el]);
  return near;
}

function FileBlock({
  sel,
  memo,
  revision,
  open,
  viewed,
  onToggleOpen,
  onToggleViewed,
  onOpen,
}: {
  sel: ListFile;
  memo: FileMemo;
  revision: number;
  open: boolean;
  viewed: boolean;
  onToggleOpen: () => void;
  onToggleViewed: () => void;
  onOpen: () => void;
}) {
  const { file } = sel;
  const box = useRef<HTMLElement>(null);
  const body = useRef<HTMLDivElement>(null);
  const near = useNear(box);
  const s = useSettings();
  const line = Math.round(s.codeFontSize * s.lineHeight);
  const shown = open && near;
  useEffect(() => {
    const el = body.current;
    if (!shown || !el) return;
    const ro = new ResizeObserver(() => (memo.height = el.offsetHeight));
    ro.observe(el);
    return () => ro.disconnect();
  }, [shown, memo]);
  const estimate = (Math.min(LARGE, (file.additions ?? 0) + (file.deletions ?? 0)) + 2 * CONTEXT + 1) * line;
  return (
    <section ref={box} data-file={selectionKey(sel)} aria-label={file.path} className="border-b border-border">
      <div className="sticky top-0 z-10 flex h-8 items-center gap-2 border-b border-border bg-panel pr-2 pl-1.5 text-[12px]">
        <button aria-expanded={open} aria-label={open ? `Collapse ${file.path}` : `Expand ${file.path}`} onClick={onToggleOpen} className="flex size-5 shrink-0 items-center justify-center rounded-sm text-subtle hover:bg-hover hover:text-foreground focus-visible:bg-hover focus-visible:text-foreground">
          <ChevronDown className={cn("size-3.5 transition-transform", !open && "-rotate-90")} />
        </button>
        <FileIcon path={file.path} />
        <Tip label="Open this file's diff">
          <button onClick={onOpen} className="min-w-0 rounded-sm outline-none hover:underline focus-visible:ring-1 focus-visible:ring-ring">
            <PathLabel path={file.path} />
          </button>
        </Tip>
        {file.oldPath && <span className="truncate text-[11.5px] text-subtle">← {file.oldPath}</span>}
        <LineCounts file={file} />
        <StatusPill status={file.status} />
        <Tip label={sel.kind === "staged" ? "Unstage" : viewed ? "Mark as not viewed" : "Mark as viewed"}>
          <Button
            variant={viewed ? "default" : "secondary"}
            size="sm"
            aria-pressed={viewed}
            onClick={onToggleViewed}
            className={cn("ml-auto", viewed && "bg-added-fill text-on-status hover:bg-added-fill/85 focus-visible:bg-added-fill/85")}
          >
            <Check /> Viewed
          </Button>
        </Tip>
      </div>
      {open &&
        (shown ? (
          <div ref={body}>
            <FileDiff sel={sel} memo={memo} revision={revision} estimate={memo.height ?? estimate} onOpen={onOpen} />
          </div>
        ) : (
          <div style={{ height: memo.height ?? estimate }} />
        ))}
    </section>
  );
}

function FileDiff({ sel, memo, revision, estimate, onOpen }: { sel: ListFile; memo: FileMemo; revision: number; estimate: number; onOpen: () => void }) {
  const s = useSettings();
  const { pair, error } = usePair(sel, revision, diffWhitespace(s), false);
  const [large, setLarge] = useState(!!memo.large);
  // Folded as the code view folds unchanged lines, each fold opening FOLD_REVEAL lines a click.
  const [revealed, setRevealed] = useState<ReadonlySet<number>>(() => memo.revealed ?? new Set());
  const rows = useMemo(() => (pair ? shownRows(pair.rows, CONTEXT, FOLD_MIN, revealed) : []), [pair, revealed]);
  const reveal = (gap: Gap) => {
    const next = new Set(revealed);
    for (let n = gap.n; n < gap.n + Math.min(gap.gap, FOLD_REVEAL); n++) next.add(n);
    setRevealed((memo.revealed = next));
  };
  if (error) return <Message text={`Could not load: ${error}`} />;
  if (!pair) return <div style={{ height: estimate }} />;
  if (mediaKind(sel.file.path)) return <Message text="Open the file to compare its versions" action={{ label: "Open", run: onOpen }} />;
  const special = placeholderFor(pair, false, sel.file);
  if (special) return <Message text={special} />;
  if (rows.length > LARGE && !large) return <Message text={`A large diff, ${rows.length} lines`} action={{ label: "Show", run: () => setLarge((memo.large = true)) }} />;
  const note = diffNote(pair, sel.file);
  return (
    <>
      {note && <div className="px-4 pt-1.5 text-[11.5px] text-subtle">{note}</div>}
      <UnifiedDiff pair={pair} rows={rows} path={sel.file.path} oldPath={sel.file.oldPath ?? sel.file.path} memo={memo} onReveal={reveal} />
    </>
  );
}

function Message({ text, action }: { text: string; action?: { label: string; run: () => void } }) {
  return (
    <div className="flex items-center gap-3 px-4 py-3 text-[12px] text-muted-foreground">
      {text}
      {action && (
        <Button variant="secondary" size="sm" onClick={action.run}>
          {action.label}
        </Button>
      )}
    </div>
  );
}
