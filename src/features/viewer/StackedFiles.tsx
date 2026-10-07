// A stacked diff's files (Open All, a guided review): one scroll of diffs, as GitHub's Files changed
// shows a pull request. Monaco is one editor per view (MonacoView), so these are drawn as HTML: only
// the files near the screen load, highlight and render, and a file scrolled far away keeps just its height.
import { Check, ChevronDown } from "lucide-react";
import { type RefObject, useEffect, useMemo, useReducer, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Tip } from "@/components/ui/tooltip";
import { FileIcon } from "@/components/FileIcon";
import { LineCounts, PathLabel, StatusPill } from "@/components/StatusBadge";
import type { RepoStatus } from "@/lib/api";
import { type Gap, shownRows } from "@/lib/git/diffHunks";
import { useCommands } from "@/lib/commands/keybindings";
import { type Selection, selectionKey } from "@/lib/repo/selection";
import { diffWhitespace, useSettings } from "@/lib/settings";
import { cn } from "@/lib/utils";
import { CONTEXT, FOLD_MIN, FOLD_REVEAL } from "./editorOptions";
import { usePair } from "./diffPairs";
import { mediaKind } from "./MediaView";
import { diffNote, placeholderFor } from "./placeholders";
import { UnifiedDiff } from "./UnifiedDiff";
import { type FileMemo, fileMemo } from "./stackedMemo";
import { GuideNote } from "@/features/review/GuideNote";

export type ListFile = Selection & { kind: "unstaged" | "staged" | "branch" | "commit" | "pr-file" };

/** A guided review's notes on a file: on the whole of it, and on lines (`old`: the old side's numbering). */
export interface Annotations {
  file: { text: string; critical: boolean }[];
  lines: { old: boolean; line: number; text: string; critical: boolean }[];
}

/** More lines than this in one file wait for a click, as GitHub's large diffs do: a lockfile would hold up the rest. */
const LARGE = 1500;
/** How far off screen a file starts loading, so it's drawn by the time it scrolls in. */
const AHEAD = "1200px 0px";
const NONE: ReadonlySet<number> = new Set();

/** The stacked view on show: the review keys walk its files instead of opening one. */
interface StackedView {
  step(dir: 1 | -1): void;
  toggleViewed(): void;
}
let onShow: StackedView | null = null;
export const stackedView = () => onShow;

/** The files' sections in `el`, the stacked view's scroller. */
export const blocksIn = (el: HTMLElement | null) => [...(el?.querySelectorAll<HTMLElement>("[data-file]") ?? [])];

/** How far down a bar pinned over the view (a guide's narrow navigator, `--stick` on a file's block) holds the sticky headers. */
const stickOf = (block: HTMLElement | undefined) => (block && parseFloat(getComputedStyle(block).getPropertyValue("--stick"))) || 0;

/** The index of the file whose header is at the top of `el`'s view; -1: none. */
export function topBlock(el: HTMLElement | null) {
  const all = blocksIn(el);
  const top = (el?.scrollTop ?? 0) + stickOf(all[0]) + 1;
  let i = 0;
  while (i + 1 < all.length && all[i + 1].offsetTop <= top) i++;
  return all.length ? i : -1;
}

/**
 * The revision `sel`'s diff is read at. What it depends on: its own content (a submodule's: its
 * commit and whether it's dirty), and the index under unstaged changes or HEAD under staged ones.
 * Each file is read again only when that changes, not on every change on disk.
 */
export function fileRevision(sel: ListFile, m: FileMemo, status: RepoStatus | null, revision: number) {
  const { file } = sel;
  const sig = [file.status, file.oid ?? `${file.additions}:${file.deletions}`, file.submodule, sel.kind === "unstaged" ? file.indexOid : sel.kind === "staged" ? status?.head : ""].join(":");
  if (m.sig !== sig) {
    Object.assign(m, { sig, rev: revision });
    // Line numbers of the version before: unfolding them in this one opens other lines.
    delete m.revealed;
  }
  return m.rev!;
}

interface Options {
  root: string;
  status: RepoStatus | null;
  revision: number;
  viewed: (sel: ListFile) => boolean;
  toggleViewed: (sel: ListFile) => void;
  /** Review notes can be written on the files (`at`: on a commit's version); null: none. */
  notes: { at?: string } | null;
  onOpen: (sel: ListFile) => void;
}

/**
 * A stacked view's files: `block` draws one, `scroller` is the element they scroll in (positioned,
 * so a file's offsetTop is its place in it). While the view is mounted it takes the review keys:
 * J/K walk its files, V marks the one on top and C writes a note on the line under the pointer,
 * else the first changed one in view.
 */
export function useStackedFiles({ root, status, revision, viewed, toggleViewed, notes, onOpen }: Options) {
  const scroller = useRef<HTMLDivElement>(null);
  const memo = (sel: ListFile) => fileMemo(`${root}\0${selectionKey(sel)}`);
  // Memos change outside React: this draws what they now say.
  const [, redraw] = useReducer((n: number) => n + 1, 0);
  // Opened or closed by hand, else closed once viewed (staged ones always count as viewed, and stay open).
  const isOpen = (sel: ListFile) => !(memo(sel).shut ?? (sel.kind !== "staged" && viewed(sel)));
  // The files drawn this render, by their blocks' keys, for V.
  const drawn = new Map<string, ListFile>();

  const blocks = () => blocksIn(scroller.current);
  const scrollTo = (el: HTMLElement | undefined) => {
    if (el && scroller.current) scroller.current.scrollTop = el.offsetTop - stickOf(el);
  };
  const markViewed = (sel: ListFile) => {
    const key = selectionKey(sel);
    delete memo(sel).shut;
    toggleViewed(sel);
    // Closing the file being read would leave the view somewhere in the next one: keep its header in view.
    const el = blocks().find((b) => b.dataset.file === key);
    requestAnimationFrame(() => el && scroller.current && el.offsetTop < scroller.current.scrollTop + stickOf(el) && scrollTo(el));
  };
  const setAll = (sels: ListFile[], open: boolean) => {
    for (const f of sels) memo(f).shut = !open;
    redraw();
  };

  const step = (dir: 1 | -1) => {
    const all = blocks();
    if (all.length) scrollTo(all[Math.max(0, Math.min(all.length - 1, topBlock(scroller.current) + dir))]);
  };
  const toggleTop = () => {
    const sel = drawn.get(blocks()[topBlock(scroller.current)]?.dataset.file ?? "");
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
  useCommands({ "review.addNote": notes ? () => keys.current.addNote() : undefined });

  const block = (sel: ListFile, annotations?: Annotations) => {
    const key = selectionKey(sel);
    const open = isOpen(sel);
    drawn.set(key, sel);
    return (
      <FileBlock
        key={key}
        sel={sel}
        memo={memo(sel)}
        revision={fileRevision(sel, memo(sel), status, revision)}
        notes={notes}
        annotations={annotations}
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
  };
  return { scroller, block, setAll };
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

/** One file of a stacked diff: its header, and its diff while it's open and near the screen (else just its height). */
function FileBlock({
  sel,
  memo,
  revision,
  notes,
  annotations,
  open,
  viewed,
  onToggleOpen,
  onToggleViewed,
  onOpen,
}: {
  sel: ListFile;
  memo: FileMemo;
  revision: number;
  notes: { at?: string } | null;
  annotations?: Annotations;
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
      <div className="sticky top-[var(--stick,0px)] z-10 flex h-8 items-center gap-2 border-b border-border bg-panel pr-2 pl-1.5 text-[12px]">
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
      {open && !!annotations?.file.length && (
        <div className="px-2 pt-1">
          {annotations.file.map((n, i) => (
            <GuideNote key={i} {...n} />
          ))}
        </div>
      )}
      {open &&
        (shown ? (
          <div ref={body}>
            <FileDiff sel={sel} memo={memo} revision={revision} notes={notes} annotations={annotations?.lines} estimate={memo.height ?? estimate} onOpen={onOpen} />
          </div>
        ) : (
          <div style={{ height: memo.height ?? estimate }} />
        ))}
    </section>
  );
}

function FileDiff({
  sel,
  memo,
  revision,
  notes,
  annotations,
  estimate,
  onOpen,
}: {
  sel: ListFile;
  memo: FileMemo;
  revision: number;
  notes: { at?: string } | null;
  annotations?: Annotations["lines"];
  estimate: number;
  onOpen: () => void;
}) {
  const s = useSettings();
  const { pair, error } = usePair(sel, revision, diffWhitespace(s), false);
  const [large, setLarge] = useState(!!memo.large);
  // Folded as the code view folds unchanged lines, each fold opening FOLD_REVEAL lines a click.
  // What's open lives in the memo, which forgets it once the file changes.
  const [, redraw] = useReducer((n: number) => n + 1, 0);
  const revealed = memo.revealed ?? NONE;
  const rows = useMemo(() => {
    if (!pair) return [];
    // An unchanged line with a guide's note on it stays unfolded.
    const noted = new Set(revealed);
    for (const a of annotations ?? []) for (const r of pair.rows) if (r.k === 0 && (a.old ? r.o : r.n) === a.line) noted.add(r.n);
    return shownRows(pair.rows, CONTEXT, FOLD_MIN, noted);
  }, [pair, revealed, annotations]);
  const reveal = (gap: Gap) => {
    const next = new Set(revealed);
    for (let n = gap.n; n < gap.n + Math.min(gap.gap, FOLD_REVEAL); n++) next.add(n);
    memo.revealed = next;
    redraw();
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
      <UnifiedDiff pair={pair} rows={rows} path={sel.file.path} oldPath={sel.file.oldPath ?? sel.file.path} memo={memo} onReveal={reveal} notes={notes} annotations={annotations} />
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
