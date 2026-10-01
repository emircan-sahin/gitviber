// A whole Changes list as one scroll of diffs, as GitHub's Files changed shows a pull request.
// Monaco is one editor per view (MonacoView), so these are drawn as HTML: only the files near the
// screen load, highlight and render, and a file scrolled far away keeps just its height.
import { Check, ChevronDown, ChevronsDownUp, ChevronsUpDown, Files } from "lucide-react";
import { type RefObject, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Tip } from "@/components/ui/tooltip";
import { FileIcon } from "@/components/FileIcon";
import { LineCounts, PathLabel, StatusPill } from "@/components/StatusBadge";
import type { RepoStatus } from "@/lib/api";
import { shownRows } from "@/lib/git/diffHunks";
import { matchesCommand } from "@/lib/commands/keybindings";
import { type ChangeList, type Selection, selectionKey, selectionPath } from "@/lib/repo/selection";
import { diffWhitespace, useSettings } from "@/lib/settings";
import { cn } from "@/lib/utils";
import { sumLines } from "@/features/changes/changeList";
import type { BranchChange } from "@/features/changes/BranchReview";
import { CONTEXT } from "./editorOptions";
import { usePair } from "./diffPairs";
import { mediaKind } from "./MediaView";
import { placeholderFor } from "./placeholders";
import { UnifiedDiff } from "./UnifiedDiff";

type ListFile = Selection & { kind: "unstaged" | "staged" | "branch" };

/** More lines than this in one file wait for a click, as GitHub's large diffs do: a lockfile would hold up the rest. */
const LARGE = 1500;
/** How far off screen a file starts loading, so it's drawn by the time it scrolls in. */
const AHEAD = "1200px 0px";

// Where each list was left, for the life of the app (tab switches remount the view).
const scrolls = new Map<ChangeList, number>();

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

export function AllChanges({ list, status, branchRows, revision, viewed, toggleViewed, onOpen }: Props) {
  const files = useMemo(() => filesOf(list, status, branchRows), [list, status, branchRows]);
  // Files opened or closed by hand; the rest are closed once viewed (staged ones always count as viewed, and stay open).
  const [shut, setShut] = useState<Map<string, boolean>>(() => new Map());
  const isOpen = (sel: ListFile) => !(shut.get(selectionKey(sel)) ?? (sel.kind !== "staged" && viewed(sel)));
  const scroller = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    const el = scroller.current;
    if (!el) return;
    el.scrollTop = scrolls.get(list) ?? 0;
    return () => void scrolls.set(list, el.scrollTop);
  }, [list]);

  const blocks = () => [...(scroller.current?.querySelectorAll<HTMLElement>("[data-file]") ?? [])];
  /** The file whose header is at the top of the view. */
  const current = () => {
    const top = (scroller.current?.scrollTop ?? 0) + 1;
    const all = blocks();
    let i = 0;
    while (i + 1 < all.length && all[i + 1].offsetTop <= top) i++;
    return all.length ? i : -1;
  };
  const scrollTo = (el: HTMLElement | undefined) => {
    if (el && scroller.current) scroller.current.scrollTop = el.offsetTop;
  };
  const markViewed = (sel: ListFile) => {
    const key = selectionKey(sel);
    setShut((m) => {
      if (!m.has(key)) return m;
      const next = new Map(m);
      next.delete(key);
      return next;
    });
    toggleViewed(sel);
    // Closing the file being read would leave the view somewhere in the next one: keep its header in view.
    const el = blocks().find((b) => b.dataset.file === key);
    requestAnimationFrame(() => el && scroller.current && el.offsetTop < scroller.current.scrollTop && scrollTo(el));
  };
  const setAll = (open: boolean) => setShut(new Map(files?.map((f) => [selectionKey(f), !open])));

  // J/K walk the files here and V marks the one on top, while this view has the keys.
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (!files?.length || (e.target as HTMLElement).closest("textarea, input")) return;
    const step = matchesCommand("review.nextFile", e.nativeEvent) ? 1 : matchesCommand("review.prevFile", e.nativeEvent) ? -1 : 0;
    const i = current();
    if (step) scrollTo(blocks()[Math.max(0, Math.min(files.length - 1, i + step))]);
    else if (matchesCommand("review.toggleViewed", e.nativeEvent) && files[i] && files[i].kind !== "staged") markViewed(files[i]);
    else return;
    e.preventDefault();
    e.stopPropagation();
  };

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
      <div ref={scroller} data-code-scroll tabIndex={0} onKeyDown={onKeyDown} className="relative min-h-0 flex-1 overflow-y-auto outline-none">
        {empty ? (
          <div className="flex h-full items-center justify-center p-6 text-[12.5px] text-muted-foreground">{empty}</div>
        ) : (
          files!.map((sel) => {
            const key = selectionKey(sel);
            const open = isOpen(sel);
            return (
              <FileBlock
                key={key}
                sel={sel}
                revision={revision}
                open={open}
                viewed={viewed(sel)}
                onToggleOpen={() => setShut((m) => new Map(m).set(key, open))}
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

function FileBlock({ sel, revision, open, viewed, onToggleOpen, onToggleViewed, onOpen }: { sel: ListFile; revision: number; open: boolean; viewed: boolean; onToggleOpen: () => void; onToggleViewed: () => void; onOpen: () => void }) {
  const { file } = sel;
  const box = useRef<HTMLElement>(null);
  const body = useRef<HTMLDivElement>(null);
  // The diff's height when it was last drawn: it keeps its room while it's off screen, so nothing above the view moves.
  const height = useRef<number | null>(null);
  const near = useNear(box);
  const s = useSettings();
  const line = Math.round(s.codeFontSize * s.lineHeight);
  const shown = open && near;
  useEffect(() => {
    const el = body.current;
    if (!shown || !el) return;
    const ro = new ResizeObserver(() => (height.current = el.offsetHeight));
    ro.observe(el);
    return () => ro.disconnect();
  }, [shown]);
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
      {open && (shown ? <div ref={body}><FileDiff sel={sel} revision={revision} estimate={estimate} onOpen={onOpen} /></div> : <div style={{ height: height.current ?? estimate }} />)}
    </section>
  );
}

function FileDiff({ sel, revision, estimate, onOpen }: { sel: ListFile; revision: number; estimate: number; onOpen: () => void }) {
  const s = useSettings();
  const { pair, error } = usePair(sel, revision, diffWhitespace(s));
  const [large, setLarge] = useState(false);
  const rows = useMemo(() => (pair ? shownRows(pair.rows, CONTEXT) : []), [pair]);
  if (error) return <Message text={`Could not load: ${error}`} />;
  if (!pair) return <div style={{ height: estimate }} />;
  if (mediaKind(sel.file.path)) return <Message text="Open the file to compare its versions" action={{ label: "Open", run: onOpen }} />;
  const special = placeholderFor(pair, false, sel.file);
  if (special) return <Message text={special} />;
  if (rows.length > LARGE && !large) return <Message text={`A large diff, ${rows.length} lines`} action={{ label: "Show", run: () => setLarge(true) }} />;
  return <UnifiedDiff pair={pair} rows={rows} path={sel.file.path} oldPath={sel.file.oldPath ?? sel.file.path} />;
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
