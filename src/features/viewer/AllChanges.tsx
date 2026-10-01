// A whole Changes list as one scroll of diffs, as GitHub's Files changed shows a pull request.
// Monaco is one editor per view (MonacoView), so these are drawn as HTML: only the files near the
// screen load, highlight and render, and a file scrolled far away keeps just its height.
import { Check, ChevronDown, ChevronsDownUp, ChevronsUpDown, Files, Plus } from "lucide-react";
import { type RefObject, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Tip } from "@/components/ui/tooltip";
import { FileIcon } from "@/components/FileIcon";
import { LineCounts, PathLabel, StatusPill } from "@/components/StatusBadge";
import type { DiffPair, DiffRow, RepoStatus } from "@/lib/api";
import { type TokenLine, tokenLookup, useHighlight } from "@/lib/editor/highlight";
import { languageFor } from "@/lib/editor/language";
import { emphasized, type Gap, shownRows, usefulEmphasis } from "@/lib/git/diffHunks";
import { matchesCommand } from "@/lib/commands/keybindings";
import { type ChangeList, type Selection, selectionKey, selectionPath } from "@/lib/repo/selection";
import { diffWhitespace, useSettings } from "@/lib/settings";
import { cn } from "@/lib/utils";
import { sumLines } from "@/features/changes/changeList";
import type { BranchChange } from "@/features/changes/BranchReview";
import { useCodeStyle } from "./ConflictView";
import { CONTEXT } from "./editorOptions";
import { usePair } from "./diffPairs";
import { mediaKind } from "./MediaView";
import { placeholderFor } from "./Viewer";
import { findLines, noteLines, type ReviewNote } from "@/lib/review/notes";
import { addNote, useNotes } from "@/lib/review/noteStore";
import { Composer } from "@/features/github/pulls/ReviewThreads";
import { NoteCard } from "@/features/review/NoteThreads";

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
  return <DiffLines pair={pair} rows={rows} path={sel.file.path} oldPath={sel.file.oldPath ?? sel.file.path} />;
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

/** A file's changes in unified form: old and new line numbers, then the line, colored as the code view colors it. */
function DiffLines({ pair, rows, path, oldPath }: { pair: DiffPair; rows: (DiffRow | Gap)[]; path: string; oldPath: string }) {
  const s = useSettings();
  const style = useCodeStyle();
  const { original: a, modified: b } = pair;
  const lang = useMemo(() => languageFor(path, b.exists ? b.text : a.text), [path, a.text, b.text, b.exists]);
  const oldLines = useMemo(() => a.text.split(/\r?\n/), [a.text]);
  const newLines = useMemo(() => b.text.split(/\r?\n/), [b.text]);
  const oldHl = useHighlight(a.exists ? a.text : null, lang, s.codeTheme);
  const newHl = useHighlight(b.exists ? b.text : null, lang, s.codeTheme);
  const [oldTok, newTok] = useMemo(() => [tokenLookup(oldHl), tokenLookup(newHl)], [oldHl, newHl]);
  // Room for the longest line number on each side.
  const digits = `${String(Math.max(oldLines.length, newLines.length)).length + 1}ch`;

  // Review notes, under the last of their lines wherever this version has them ("o:12" old line 12, "n:12" new).
  const notes = useNotes();
  const linesOf = (old: boolean) => (old ? (a.exists ? oldLines : null) : b.exists ? newLines : null);
  const placed = useMemo(() => {
    const at = new Map<string, ReviewNote[]>();
    for (const n of notes) {
      const lines = linesOf(!!n.old);
      const start = !n.resolved && n.path === (n.old ? oldPath : path) && lines ? findLines(lines, n.code, n.start) : null;
      if (start === null) continue;
      const key = `${n.old ? "o" : "n"}:${start + n.code.length - 1}`;
      at.set(key, [...(at.get(key) ?? []), n]);
    }
    return at;
  }, [notes, path, oldPath, oldLines, newLines, a.exists, b.exists]);
  // A note being written: its lines as picked, followed as the file changes.
  const [draft, setDraft] = useState<{ old: boolean; start: number; code: string[] } | null>(null);
  const drafted = useMemo(() => {
    if (!draft) return null;
    const start = findLines(draft.old ? oldLines : newLines, draft.code, draft.start) ?? draft.start;
    const end = start + draft.code.length - 1;
    return { ...draft, start, end, key: `${draft.old ? "o" : "n"}:${end}` };
  }, [draft, oldLines, newLines]);
  // ⇧-click stretches the note being written to the line clicked.
  const note = (old: boolean, line: number, stretch: boolean) => {
    const lines = linesOf(old);
    if (!lines) return;
    const [from, to] = stretch && drafted?.old === old ? [drafted.start, drafted.end] : [line, line];
    const [start, end] = [Math.min(from, line), Math.max(to, line)];
    setDraft({ old, start, code: lines.slice(start - 1, end) });
  };

  return (
    <div className={cn("py-1 select-text", s.wordWrap ? "[overflow-wrap:anywhere]" : "overflow-x-auto")} style={{ ...style, color: (newHl ?? oldHl)?.data.fg }}>
      <div className={cn(!s.wordWrap && "min-w-max")}>
        {rows.map((r, i) => {
          if ("gap" in r) return <GapRow key={i} count={r.gap} />;
          const text = r.k === 2 ? (oldLines[r.o - 1] ?? "") : (newLines[r.n - 1] ?? "");
          const tokens: TokenLine | undefined = r.k === 2 ? oldTok(r.o - 1, text) : newTok(r.n - 1, text);
          const keys = [r.k !== 1 && `o:${r.o}`, r.k !== 2 && `n:${r.n}`].filter((k): k is string => !!k);
          const old = r.k === 2;
          return (
            <div key={i}>
              <LineRow row={r} text={text} tokens={tokens} digits={digits} wrap={s.wordWrap} onNote={(stretch) => note(old, old ? r.o : r.n, stretch)} />
              {keys.flatMap((k) => placed.get(k) ?? []).map((n) => (
                <div key={n.id} className="sticky left-0 max-w-3xl py-0.5">
                  <NoteCard note={n} />
                </div>
              ))}
              {drafted && keys.includes(drafted.key) && (
                <div className="sticky left-0 max-w-3xl py-0.5">
                  <Composer
                    label={`Note on ${noteLines(drafted)}${drafted.old ? " (old)" : ""}`}
                    onCancel={() => setDraft(null)}
                    onSubmit={async (body) => {
                      addNote({ path: drafted.old ? oldPath : path, start: drafted.start, end: drafted.end, code: drafted.code, body: body.trim(), old: drafted.old || undefined });
                      setDraft(null);
                    }}
                  />
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

const TONES = {
  0: { line: "", gutter: "text-subtle", emph: "", sign: " " },
  1: { line: "bg-add-bg", gutter: "bg-add-gutter text-muted-foreground", emph: "bg-add-emph", sign: "+" },
  2: { line: "bg-del-bg", gutter: "bg-del-gutter text-muted-foreground", emph: "bg-del-emph", sign: "−" },
} as const;

function LineRow({ row, text, tokens, digits, wrap, onNote }: { row: DiffRow; text: string; tokens: TokenLine | undefined; digits: string; wrap: boolean; onNote: (stretch: boolean) => void }) {
  const tone = TONES[row.k];
  const pieces = emphasized(text, tokens, row.k ? usefulEmphasis(text, row.e) : []);
  return (
    <div className={cn("group/line relative flex min-h-[1lh]", tone.line)}>
      {/* Off the Tab order, as there's one per line: the keyboard writes notes in the file's own diff. */}
      <button
        tabIndex={-1}
        aria-label="Add review note"
        title="Add review note (⇧-click: through this line)"
        onClick={(e) => onNote(e.shiftKey)}
        className="absolute top-0 left-0 z-[1] flex h-[1lh] w-[2.2ch] items-center justify-center rounded-sm bg-primary text-primary-foreground opacity-0 group-hover/line:opacity-100 focus-visible:opacity-100 [&_svg]:size-3"
      >
        <Plus />
      </button>
      <span className={cn("shrink-0 pr-1 text-right select-none", tone.gutter)} style={{ width: digits }}>
        {row.k !== 1 ? row.o : ""}
      </span>
      <span className={cn("shrink-0 pr-1 text-right select-none", tone.gutter)} style={{ width: digits }}>
        {row.k !== 2 ? row.n : ""}
      </span>
      <span aria-hidden className="w-[3ch] shrink-0 text-center text-subtle select-none">
        {tone.sign}
      </span>
      <span className={cn("min-w-0 flex-1 pr-4", wrap ? "whitespace-pre-wrap" : "whitespace-pre")}>
        {pieces.map(([t, color, fs, emph], i) => (
          <span key={i} className={emph ? tone.emph : undefined} style={{ color: color || undefined, fontStyle: fs & 1 ? "italic" : undefined, fontWeight: fs & 2 ? 600 : undefined }}>
            {t}
          </span>
        ))}
      </span>
    </div>
  );
}

function GapRow({ count }: { count: number }) {
  return (
    <div className="flex min-h-[1lh] items-center bg-panel px-3 font-sans text-[11px] text-subtle select-none">
      {count === 1 ? "1 unchanged line" : `${count} unchanged lines`}
    </div>
  );
}
