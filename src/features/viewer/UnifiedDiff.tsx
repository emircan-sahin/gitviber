import { ChevronsUpDown, Plus } from "lucide-react";
import { useMemo, useRef, useState } from "react";
import type { DiffPair, DiffRow } from "@/lib/api";
import { type TokenLine, tokenLookup, useHighlight } from "@/lib/editor/highlight";
import { copyNarrowed, indentUnit, widen, widenColumn } from "@/lib/editor/indent";
import { languageFor } from "@/lib/editor/language";
import { type Gap, usefulEmphasis } from "@/lib/git/diffHunks";
import { anchorAt, findNote, noteLines, placeNotes, type ReviewNote } from "@/lib/review/notes";
import { addNote, useNotes } from "@/lib/review/noteStore";
import { useSettings } from "@/lib/settings";
import { cn } from "@/lib/utils";
import { Composer } from "@/features/review/Composer";
import { NoteCard } from "@/features/review/NoteThreads";
import { Tokens, useCodeStyle } from "./codeLines";
import { FOLD_REVEAL } from "./editorOptions";
import type { FileMemo } from "./stackedMemo";

/** A file's changes in unified form: old and new line numbers, then the line, colored as the code view colors it. */
export function UnifiedDiff({
  pair,
  rows,
  path,
  oldPath,
  memo,
  onReveal,
}: {
  pair: DiffPair;
  rows: (DiffRow | Gap)[];
  path: string;
  oldPath: string;
  memo: FileMemo;
  /** Opens some of a fold's lines. */
  onReveal: (gap: Gap) => void;
}) {
  const s = useSettings();
  const box = useRef<HTMLDivElement>(null);
  const style = useCodeStyle();
  const { original: a, modified: b } = pair;
  const lang = useMemo(() => languageFor(path, b.exists ? b.text : a.text), [path, a.text, b.text, b.exists]);
  // The file's own lines (notes are anchored by them), and as shown: indentation widened as the code view widens it.
  const oldLines = useMemo(() => a.text.split(/\r?\n/), [a.text]);
  const newLines = useMemo(() => b.text.split(/\r?\n/), [b.text]);
  const unit = useMemo(() => indentUnit(b.text, a.text), [a.text, b.text]);
  const [oldWide, newWide] = useMemo(() => [widen(a.text, unit), widen(b.text, unit)], [a.text, b.text, unit]);
  const [oldShown, newShown] = useMemo(() => [oldWide.split(/\r?\n/), newWide.split(/\r?\n/)], [oldWide, newWide]);
  const oldHl = useHighlight(a.exists ? oldWide : null, lang, s.codeTheme);
  const newHl = useHighlight(b.exists ? newWide : null, lang, s.codeTheme);
  const [oldTok, newTok] = useMemo(() => [tokenLookup(oldHl), tokenLookup(newHl)], [oldHl, newHl]);
  // Room for the longest line number on each side.
  const digits = `${String(Math.max(oldLines.length, newLines.length)).length + 1}ch`;

  // Review notes, under the last of their lines wherever this version has them ("o:12" old line 12, "n:12" new).
  const notes = useNotes();
  const linesOf = (old: boolean) => (old ? (a.exists ? oldLines : null) : b.exists ? newLines : null);
  const placed = useMemo(() => {
    const at = new Map<string, ReviewNote[]>();
    for (const p of placeNotes(notes, { path, oldPath, oldLines: a.exists ? oldLines : null, newLines: b.exists ? newLines : null })) {
      const key = `${p.old ? "o" : "n"}:${p.end}`;
      at.set(key, [...(at.get(key) ?? []), p.note]);
    }
    return at;
  }, [notes, path, oldPath, oldLines, newLines, a.exists, b.exists]);
  // A note being written: its lines as picked, followed as the file changes. Kept in the file's
  // memo, so scrolling far away or switching tabs doesn't lose it.
  const [draft, setDraftState] = useState(memo.draft ?? null);
  const setDraft = (d: FileMemo["draft"] | null) => {
    memo.draft = d ?? undefined;
    setDraftState(d ?? null);
    // A closed note box hands the keys back to the view, for J/K and C.
    if (!d) box.current?.closest<HTMLElement>("[data-code-scroll]")?.focus({ preventScroll: true });
  };
  const drafted = useMemo(() => {
    if (!draft) return null;
    const start = findNote(draft.old ? oldLines : newLines, draft.anchor) ?? draft.anchor.start;
    const end = start + draft.anchor.end - draft.anchor.start;
    return { ...draft, anchor: { ...draft.anchor, start, end }, key: `${draft.old ? "o" : "n"}:${end}` };
  }, [draft, oldLines, newLines]);
  // ⇧-click stretches the note being written to the line clicked.
  const note = (old: boolean, line: number, stretch: boolean) => {
    const lines = linesOf(old);
    if (!lines) return;
    const [from, to] = stretch && drafted?.old === old ? [drafted.anchor.start, drafted.anchor.end] : [line, line];
    setDraft({ old, anchor: anchorAt(lines, Math.min(from, line), Math.max(to, line)), body: memo.draft?.body ?? "" });
  };

  return (
    <div ref={box} className={cn("py-1 select-text", s.wordWrap ? "[overflow-wrap:anywhere]" : "overflow-x-auto")} style={{ ...style, color: (newHl ?? oldHl)?.data.fg }} onCopy={copyNarrowed(unit)}>
      <div className={cn(!s.wordWrap && "min-w-max")}>
        {rows.map((r) => {
          if ("gap" in r) return <GapRow key={`g${r.o}:${r.n}`} count={r.gap} onClick={() => onReveal(r)} />;
          const raw = r.k === 2 ? (oldLines[r.o - 1] ?? "") : (newLines[r.n - 1] ?? "");
          const text = r.k === 2 ? (oldShown[r.o - 1] ?? "") : (newShown[r.n - 1] ?? "");
          const tokens = r.k === 2 ? oldTok(r.o - 1, text) : newTok(r.n - 1, text);
          // Word changes are git's columns in the file's line: moved along with its widened indentation.
          const ranges = r.k ? usefulEmphasis(raw, r.e).map(([x, y]) => [widenColumn(raw, x, unit), widenColumn(raw, y, unit)] as [number, number]) : [];
          const keys = [r.k !== 1 && `o:${r.o}`, r.k !== 2 && `n:${r.n}`].filter((k): k is string => !!k);
          const old = r.k === 2;
          return (
            <div key={`${r.o}:${r.n}`}>
              <LineRow row={r} text={text} tokens={tokens} ranges={ranges} digits={digits} wrap={s.wordWrap} onNote={(stretch) => note(old, old ? r.o : r.n, stretch)} />
              {keys.flatMap((k) => placed.get(k) ?? []).map((n) => (
                <div key={n.id} className="sticky left-0 max-w-3xl py-0.5">
                  <NoteCard note={n} />
                </div>
              ))}
              {drafted && keys.includes(drafted.key) && (
                <div className="sticky left-0 max-w-3xl py-0.5">
                  <Composer
                    label={`Note on ${noteLines(drafted.anchor)}${drafted.old ? " (old)" : ""}`}
                    initial={memo.draft?.body}
                    onDraft={(body) => memo.draft && (memo.draft.body = body)}
                    onCancel={() => setDraft(null)}
                    onSubmit={async (body) => {
                      addNote({ ...drafted.anchor, path: drafted.old ? oldPath : path, body: body.trim(), old: drafted.old || undefined });
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

function LineRow({ row, text, tokens, ranges, digits, wrap, onNote }: { row: DiffRow; text: string; tokens: TokenLine | undefined; ranges: [number, number][]; digits: string; wrap: boolean; onNote: (stretch: boolean) => void }) {
  const tone = TONES[row.k];
  return (
    <div data-line={row.k} className={cn("group/line relative flex min-h-[1lh]", tone.line)}>
      {/* Off the Tab order, as there's one per line: the keyboard's way is Add Review Note (C). */}
      <button
        data-add-note
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
        <Tokens text={text} tokens={tokens} ranges={ranges} emphasis={tone.emph} />
      </span>
    </div>
  );
}

function GapRow({ count, onClick }: { count: number; onClick: () => void }) {
  const more = Math.min(count, FOLD_REVEAL);
  return (
    <button
      onClick={onClick}
      title={count > more ? `Show ${more} more lines` : undefined}
      className="sticky left-0 flex min-h-[1lh] w-full items-center gap-1.5 bg-panel px-3 text-left font-sans text-[11px] text-subtle outline-none select-none hover:bg-elevated hover:text-foreground focus-visible:bg-elevated focus-visible:text-foreground"
    >
      <ChevronsUpDown className="size-3" />
      {count === 1 ? "1 unchanged line" : `${count} unchanged lines`}
    </button>
  );
}
