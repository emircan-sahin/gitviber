import { useVirtualizer } from "@tanstack/react-virtual";
import { ChevronsUpDown } from "lucide-react";
import { forwardRef, memo, useCallback, useEffect, useImperativeHandle, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { DiffPair, DiffRow } from "@/lib/api";
import { showLanguage, type TokenLine, tokenLookup, useHighlight } from "@/lib/highlight";
import { languageFor } from "@/lib/language";
import { CODE_FONTS, useSettings } from "@/lib/settings";
import { cn } from "@/lib/utils";
import { lineWidth, TAB, type Wrap, wrapLine } from "@/lib/wrap";
import { type Lane, type Mark, OverviewRuler } from "./OverviewRuler";

export type CodeMode = "unified" | "split" | "file";
/** Which code column a line sits in: the full-width one, or a split half. */
type Side = "row" | "old" | "new";

type Marker = "add" | "mod";
type Item =
  | { t: "row"; r: DiffRow; marker?: Marker; delAbove?: boolean; delBelow?: boolean }
  | { t: "pair"; l: DiffRow | null; r: DiffRow | null }
  | { t: "gap"; id: number; count: number };

export interface CodeViewHandle {
  next(): void;
  prev(): void;
}

const CONTEXT = 3;
const FULL_LIMIT = 6000;
const CHUNK = 128;
// Scroll positions survive tab switches for the life of the app.
const scrollMemory = new Map<string, number>();

function splitLines(text: string) {
  const lines = text.split("\n");
  if (lines[lines.length - 1] === "") lines.pop();
  return lines.map((l) => (l.endsWith("\r") ? l.slice(0, -1) : l));
}

function buildUnified(rows: DiffRow[], collapse: boolean, expanded: Set<number>): Item[] {
  const hasChanges = rows.some((r) => r.k !== 0);
  if (!collapse || !hasChanges) return rows.map((r) => ({ t: "row", r }));
  // Distance to the nearest change decides what stays visible.
  const keep = new Array<boolean>(rows.length).fill(false);
  let last = -Infinity;
  for (let i = 0; i < rows.length; i++) {
    if (rows[i].k !== 0) last = i;
    keep[i] = i - last <= CONTEXT;
  }
  last = Infinity;
  for (let i = rows.length - 1; i >= 0; i--) {
    if (rows[i].k !== 0) last = i;
    keep[i] ||= last - i <= CONTEXT;
  }
  const items: Item[] = [];
  for (let i = 0; i < rows.length; ) {
    if (keep[i]) {
      items.push({ t: "row", r: rows[i++] });
      continue;
    }
    let j = i;
    while (j < rows.length && !keep[j]) j++;
    // Gaps are identified by their first original line, which survives edits elsewhere.
    if (expanded.has(rows[i].o) || j - i < 2) for (let x = i; x < j; x++) items.push({ t: "row", r: rows[x] });
    else items.push({ t: "gap", id: rows[i].o, count: j - i });
    i = j;
  }
  return items;
}

function toSplit(items: Item[]): Item[] {
  const out: Item[] = [];
  for (let i = 0; i < items.length; ) {
    const it = items[i];
    if (it.t !== "row" || it.r.k === 0) {
      out.push(it.t === "row" ? { t: "pair", l: it.r, r: it.r } : it);
      i++;
      continue;
    }
    const dels: DiffRow[] = [];
    const adds: DiffRow[] = [];
    while (i < items.length) {
      const c = items[i];
      if (c.t !== "row" || c.r.k === 0) break;
      (c.r.k === 2 ? dels : adds).push(c.r);
      i++;
    }
    for (let x = 0; x < Math.max(dels.length, adds.length); x++) out.push({ t: "pair", l: dels[x] ?? null, r: adds[x] ?? null });
  }
  return out;
}

/** Full file with VS Code-style change bars in the gutter. */
function buildFile(rows: DiffRow[], markers: boolean): Item[] {
  const items: Item[] = [];
  for (let i = 0; i < rows.length; ) {
    if (rows[i].k === 0) {
      items.push({ t: "row", r: rows[i++] });
      continue;
    }
    let j = i;
    let dels = 0;
    while (j < rows.length && rows[j].k !== 0) if (rows[j++].k === 2) dels++;
    const adds = rows.slice(i, j).filter((r) => r.k === 1);
    for (const r of adds) items.push({ t: "row", r, marker: markers ? (dels ? "mod" : "add") : undefined });
    if (markers && dels && !adds.length) {
      if (j < rows.length) {
        items.push({ t: "row", r: rows[j], delAbove: true });
        j++;
      } else {
        // Deleted at the end of the file: mark the last line instead.
        const last = items[items.length - 1];
        if (last?.t === "row") items[items.length - 1] = { ...last, delBelow: true };
      }
    }
    i = j;
  }
  return items;
}

interface Props {
  pair: DiffPair;
  path: string;
  mode: CodeMode;
  collapse: boolean;
  wrap: boolean;
  scrollKey: string;
}

export const CodeView = forwardRef<CodeViewHandle, Props>(function CodeView({ pair, path, mode, collapse, wrap, scrollKey }, ref) {
  const s = useSettings();
  const scrollRef = useRef<HTMLDivElement>(null);
  const [expanded, setExpanded] = useState<Set<number>>(() => new Set());

  const oldLines = useMemo(() => splitLines(pair.original.text), [pair.original.text]);
  const newLines = useMemo(() => splitLines(pair.modified.text), [pair.modified.text]);
  const lang = useMemo(() => languageFor(path, pair.modified.exists ? pair.modified.text : pair.original.text), [path, pair]);
  useEffect(() => {
    showLanguage(lang);
    return () => showLanguage(null);
  }, [lang]);
  // New side first: it's the one the first paint waits for, and the worker takes views in order.
  const newHl = useHighlight(pair.modified.text, lang, s.codeTheme);
  const oldHl = useHighlight(mode === "file" ? null : pair.original.text, lang, s.codeTheme);
  const oldTok = useMemo(() => tokenLookup(oldHl), [oldHl]);
  const newTok = useMemo(() => tokenLookup(newHl), [newHl]);

  const items = useMemo(() => {
    if (mode === "file") return buildFile(pair.rows, pair.original.exists && pair.modified.exists);
    const unified = buildUnified(pair.rows, collapse, expanded);
    return mode === "split" ? toSplit(unified) : unified;
  }, [pair, mode, collapse, expanded]);

  const lh = Math.round(s.codeFontSize * s.lineHeight);
  const gapH = lh + 12;
  // Unwrapped split halves scroll sideways each in its own column, kept in step, like VS Code's
  // two editors; one vertical scroll carries both, and computed heights keep them level.
  const columns = mode === "split" && !wrap;
  const digits = String(Math.max(oldLines.length, newLines.length, 1)).length;
  const maxLen = useMemo(() => {
    let m = 0;
    for (const l of newLines) m = Math.max(m, lineWidth(l));
    if (mode !== "file") for (const l of oldLines) m = Math.max(m, lineWidth(l));
    return m;
  }, [oldLines, newLines, mode]);

  // The view's width and the font's character width: with those every row's height is
  // computed, never measured, so the layout is known before anything is laid out.
  const probeRef = useRef<HTMLSpanElement>(null);
  const [box, setBox] = useState<{ width: number; cw: number } | null>(null);
  useLayoutEffect(() => {
    const sc = scrollRef.current!;
    const measure = () => {
      const width = sc.clientWidth;
      // A hidden or collapsed pane: keep the last layout rather than wrap every line to nothing.
      if (!width) return;
      const cw = probeRef.current!.getBoundingClientRect().width / PROBE.length;
      setBox((b) => (b?.width === width && b.cw === cw ? b : { width, cw }));
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(sc);
    // A web font (JetBrains Mono, Geist Mono) can arrive after the first measure.
    document.fonts.addEventListener("loadingdone", measure);
    return () => {
      ro.disconnect();
      document.fonts.removeEventListener("loadingdone", measure);
    };
  }, [s.codeFont, s.codeFontSize, s.ligatures]);

  // Columns a row's code gets before it wraps (0: it doesn't), from the gutters laid out below.
  const fit = (px: number) => (box ? Math.max(1, Math.floor((px - CODE_PAD) / box.cw + 1e-3)) : 0);
  const rowGutter: Gutter = mode === "file" ? "file" : "unified";
  const half = box ? box.width / 2 - gutterPx("half", digits, box.cw) : 0;
  // The wrapped split's right half is 1px narrower: its border.
  const cols = !box || !wrap ? [0, 0, 0] : mode === "split" ? [0, fit(half), fit(half - 1)] : [fit(box.width - gutterPx(rowGutter, digits, box.cw)), 0, 0];
  const [colsRow, colsOld, colsNew] = cols;
  // A split column's full width, and how much of it doesn't fit on screen.
  const colW = `calc(${gutterCss("half", digits)} + ${CODE_PAD}px + ${maxLen}ch)`;
  const reach = columns && box ? Math.max(0, Math.ceil(gutterPx("half", digits, box.cw) + CODE_PAD + maxLen * box.cw - box.width / 2)) : 0;
  const colRefs = { old: useRef<HTMLDivElement>(null), new: useRef<HTMLDivElement>(null) };
  const barRef = useRef<HTMLDivElement>(null);
  /** Brings both columns and the bar under them to `x`. */
  const scrollX = (e: React.UIEvent<HTMLDivElement>) => {
    const x = e.currentTarget.scrollLeft;
    for (const el of [colRefs.old.current, colRefs.new.current, barRef.current]) if (el && el !== e.currentTarget && el.scrollLeft !== x) el.scrollLeft = x;
  };
  const wrapAt = useMemo(() => {
    const byCols: Record<Side, number> = { row: colsRow, old: colsOld, new: colsNew };
    const caches: Record<Side, Map<string, Wrap>> = { row: new Map(), old: new Map(), new: new Map() };
    return (text: string, side: Side): Wrap | null => {
      const c = byCols[side];
      if (!c) return null;
      let w = caches[side].get(text);
      if (!w) caches[side].set(text, (w = wrapLine(text, c)));
      return w;
    };
  }, [colsRow, colsOld, colsNew]);

  /** Top offset of every item, and the content height at the end. */
  const prefix = useMemo(() => {
    const lines = (r: DiffRow | null, side: Side, old: boolean) => {
      if (!r) return 0;
      const text = (old ? oldLines[r.o - 1] : newLines[r.n - 1]) ?? "";
      return (wrapAt(text, side)?.at.length ?? 0) + 1;
    };
    const p = new Float64Array(items.length + 1);
    items.forEach((it, i) => {
      const h = it.t === "gap" ? gapH : it.t === "pair" ? lh * Math.max(lines(it.l, "old", true), lines(it.r, "new", false)) : lh * lines(it.r, "row", it.r.k === 2);
      p[i + 1] = p[i] + h;
    });
    return p;
  }, [items, oldLines, newLines, wrapAt, lh, gapH]);

  // Small/medium files stay fully in the DOM so scrolling is purely native (no rows
  // appearing late on fast flicks); only huge files fall back to virtualization.
  const virtual = items.length > FULL_LIMIT;
  const virtualizer = useVirtualizer({
    count: virtual ? items.length : 0,
    getScrollElement: () => scrollRef.current,
    estimateSize: (i) => prefix[i + 1] - prefix[i],
    overscan: 120,
  });
  // Sizes are computed, not measured: hand the virtualizer the new ones whenever they change.
  useLayoutEffect(() => {
    if (virtual) virtualizer.measure();
  }, [virtual, virtualizer, prefix]);

  // Show the file once, colored, instead of plain text that recolors a moment later.
  const [grace, setGrace] = useState(true);
  useEffect(() => {
    const t = setTimeout(() => setGrace(false), 250);
    return () => clearTimeout(t);
  }, []);
  const waiting = grace && lang !== "text" && !newHl;
  const shown = !!box && !waiting;

  const changeStarts = useMemo(() => {
    const starts: number[] = [];
    const isChange = (it: Item) =>
      it.t === "row"
        ? mode === "file"
          ? !!(it.marker || it.delAbove || it.delBelow)
          : it.r.k !== 0
        : it.t === "pair" && (it.l?.k !== 0 || it.r?.k !== 0);
    items.forEach((it, i) => isChange(it) && (i === 0 || !isChange(items[i - 1])) && starts.push(i));
    return starts;
  }, [items, mode]);

  const jumped = useRef<{ i: number; top: number } | null>(null);
  const scrollToItem = useCallback((i: number) => {
    const sc = scrollRef.current!;
    sc.scrollTop = prefix[Math.max(0, i - CONTEXT)];
    jumped.current = { i, top: sc.scrollTop };
  }, [prefix]);

  useImperativeHandle(
    ref,
    () => {
      // Still where the last jump left it: that change. Else the row CONTEXT lines below the top,
      // where a jump puts a change (by pixels: wrapped rows and gaps are taller than a line).
      const current = () => {
        const top = scrollRef.current!.scrollTop;
        const j = jumped.current;
        return j && Math.abs(j.top - top) < 1 ? j.i : itemAt(prefix, top + CONTEXT * lh);
      };
      return {
        next: () => {
          const i = changeStarts.find((x) => x > current());
          if (i != null) scrollToItem(i);
        },
        prev: () => {
          const i = [...changeStarts].reverse().find((x) => x < current());
          if (i != null) scrollToItem(i);
        },
      };
    },
    [changeStarts, scrollToItem, prefix, lh],
  );

  const ctx = useMemo(
    () => ({ oldLines, newLines, oldTok, newTok, fg: newHl?.data.fg ?? oldHl?.data.fg, digits, mode, wrapAt }),
    [oldLines, newLines, oldTok, newTok, newHl?.data.fg, oldHl?.data.fg, digits, mode, wrapAt],
  );
  const expand = useCallback((id: number) => setExpanded((e) => new Set(e).add(id)), []);
  const chunkCount = Math.ceil(items.length / CHUNK);
  const chunkTop = (c: number) => prefix[Math.min(items.length, c * CHUNK)];
  const around = (c: number) => new Set([c - 1, c, c + 1].filter((x) => x >= 0 && x < chunkCount));

  // Chunks mount lazily: those within two viewports right away, the rest one at a time while idle.
  // A new layout starts again from the chunks on screen (re-rendering every mounted row froze a
  // 1500-line file for 0.5–3s), and keeps the row at the top at the top.
  const filledRef = useRef<Set<number>>(new Set());
  const laidOut = useRef<{ items: Item[]; prefix: Float64Array } | null>(null);
  const pendingTop = useRef<number | null>(null);
  if (box && laidOut.current?.prefix !== prefix) {
    const old = laidOut.current;
    // A position not applied yet (content still hidden) is the one to carry over.
    const y = pendingTop.current ?? scrollRef.current!.scrollTop;
    let top: number;
    if (!old) {
      // First layout: the remembered position, else near the first change.
      top = scrollMemory.get(scrollKey) ?? prefix[Math.max(0, (changeStarts[0] ?? 0) - CONTEXT)];
    } else {
      const i = itemAt(old.prefix, y);
      const j = old.items === items ? i : sameItem(old.items[i], items);
      top = j < 0 ? y : prefix[j] + Math.min(y - old.prefix[i], prefix[j + 1] - prefix[j]);
    }
    laidOut.current = { items, prefix };
    pendingTop.current = top;
    jumped.current = null;
    filledRef.current = around(Math.floor(itemAt(prefix, top) / CHUNK));
  }
  const [, setFillTick] = useState(0);
  const lastScroll = useRef(0);

  /** Chunk indices overlapping [top − 2 viewports, bottom + 2 viewports], nearest first. */
  const nearChunks = useCallback(() => {
    const sc = scrollRef.current;
    if (!sc) return [];
    const mid = sc.scrollTop + sc.clientHeight / 2;
    const reach = sc.clientHeight * 2.5;
    const out: [number, number][] = [];
    for (let c = 0; c < chunkCount; c++) {
      const top = prefix[c * CHUNK];
      const bottom = prefix[Math.min(items.length, (c + 1) * CHUNK)];
      const dist = mid < top ? top - mid : mid > bottom ? mid - bottom : 0;
      if (dist <= reach) out.push([c, dist]);
    }
    return out.sort((x, y) => x[1] - y[1]).map((x) => x[0]);
  }, [prefix, chunkCount, items.length]);

  const fill = useCallback(
    (chunks: number[], max = Infinity) => {
      const fresh = chunks.filter((c) => c >= 0 && c < chunkCount && !filledRef.current.has(c)).slice(0, max);
      if (!fresh.length) return;
      fresh.forEach((c) => filledRef.current.add(c));
      setFillTick((t) => t + 1);
    },
    [chunkCount],
  );

  // After every render: land on the position a new layout asked for, then top up the buffer
  // around the viewport. Heights are exact, so a chunk filling in never moves anything.
  useLayoutEffect(() => {
    if (!shown) return;
    if (pendingTop.current != null) {
      scrollRef.current!.scrollTop = pendingTop.current;
      pendingTop.current = null;
    }
    if (!virtual) fill(nearChunks());
  });

  // Background fill while idle, one chunk per tick so a task never gets long. Restarted with
  // every layout: the interval stops once all chunks are in, and a new layout empties them.
  useEffect(() => {
    if (virtual || !shown) return;
    const id = setInterval(() => {
      let unfilled = false;
      for (let c = 0; c < chunkCount && !unfilled; c++) unfilled = !filledRef.current.has(c);
      if (!unfilled) return clearInterval(id);
      if (performance.now() - lastScroll.current < 250) return;
      const sc = scrollRef.current;
      const mid = sc ? Math.floor(itemAt(prefix, sc.scrollTop) / CHUNK) : 0;
      let best = -1;
      for (let c = 0; c < chunkCount; c++) if (!filledRef.current.has(c) && (best < 0 || Math.abs(c - mid) < Math.abs(best - mid))) best = c;
      if (best >= 0) fill([best]);
    }, 40);
    return () => clearInterval(id);
  }, [virtual, shown, chunkCount, fill, prefix]);

  const onScroll = (e: React.UIEvent<HTMLDivElement>) => {
    scrollMemory.set(scrollKey, e.currentTarget.scrollTop);
    lastScroll.current = performance.now();
    // Keep the buffer ahead of the scroll; one chunk per event keeps frames short.
    if (!virtual) fill(nearChunks(), 1);
  };


  const overview = useMemo(() => overviewMarks(items, mode === "file"), [items, mode]);

  /** The rows, or with `side` one column's half of each. */
  const list = (side?: "old" | "new") =>
    virtual ? (
      <div className="relative" style={{ height: prefix[items.length] }}>
        {virtualizer.getVirtualItems().map((v) => (
          <div key={v.key} data-i={v.index} className="absolute top-0 left-0 w-full" style={{ transform: `translateY(${prefix[v.index]}px)` }}>
            <ItemView item={items[v.index]} height={prefix[v.index + 1] - prefix[v.index]} side={side} ctx={ctx} onExpand={expand} />
          </div>
        ))}
      </div>
    ) : (
      Array.from({ length: chunkCount }, (_, c) =>
        filledRef.current.has(c) ? (
          <Chunk key={c} items={items} prefix={prefix} start={c * CHUNK} height={chunkTop(c + 1) - chunkTop(c)} side={side} ctx={ctx} onExpand={expand} />
        ) : (
          <div key={c} data-chunk={c} style={{ height: chunkTop(c + 1) - chunkTop(c) }} />
        ),
      )
    );
  const offsetOf = useCallback((i: number) => prefix[i], [prefix]);

  return (
    <div className="flex h-full">
      <div className="flex min-w-0 flex-1 flex-col">
        <div
          ref={scrollRef}
          onScroll={onScroll}
          className="code-scroll relative min-h-0 flex-1 overflow-auto bg-background"
          style={{
            fontFamily: CODE_FONTS[s.codeFont],
            fontSize: s.codeFontSize,
            lineHeight: `${lh}px`,
            fontVariantLigatures: s.ligatures ? "normal" : "none",
            tabSize: TAB,
            color: ctx.fg,
          }}
        >
          {shown &&
            (columns ? (
              <div className="relative grid grid-cols-2">
                {/* Not a column border: the columns stay equal, so they scroll equally far. */}
                <div className="pointer-events-none absolute inset-y-0 left-1/2 z-20 w-px bg-border-strong" />
                {(["old", "new"] as const).map((side) => (
                  <div
                    key={side}
                    ref={colRefs[side]}
                    onScroll={scrollX}
                    data-scrollbar="none"
                    className="overflow-x-auto overflow-y-hidden"
                  >
                    <div className="relative" style={{ width: `max(100%, ${colW})` }}>
                      {list(side)}
                    </div>
                  </div>
                ))}
              </div>
            ) : (
              <div className="relative" style={{ width: wrap ? "100%" : `max(100%, calc(${gutterCss(rowGutter, digits)} + ${maxLen}ch + 48px))` }}>
                {list()}
              </div>
            ))}
          <div aria-hidden className="pointer-events-none invisible absolute top-0 left-0 h-0 w-0 overflow-hidden">
            <span ref={probeRef} className="whitespace-pre">
              {PROBE}
            </span>
          </div>
        </div>
        {shown && reach > 0 && (
          <div
            ref={barRef}
            onScroll={scrollX}
            className="code-scroll h-2.5 shrink-0 overflow-x-auto overflow-y-hidden bg-background"
          >
            <div className="h-px" style={{ width: `calc(100% + ${reach}px)` }} />
          </div>
        )}
      </div>
      <OverviewRuler marks={overview} offsetOf={offsetOf} split={mode !== "file"} ready={shown} scrollRef={scrollRef} />
    </div>
  );
});

/** Runs of changed items (by index), for the overview ruler. */
function overviewMarks(items: Item[], fileMode: boolean): Mark[] {
  const marks: Mark[] = [];
  const last: Partial<Record<Lane, Mark>> = {};
  const add = (lane: Lane, i0: number, i1: number) => {
    const prev = last[lane];
    if (prev && prev.i1 >= i0) prev.i1 = Math.max(prev.i1, i1);
    else marks.push((last[lane] = { lane, i0, i1 }));
  };
  items.forEach((it, i) => {
    if (it.t === "row") {
      if (it.delAbove) add("del", i, i);
      if (it.delBelow) add("del", i + 1, i + 1);
      // In file mode only marked lines count (a brand-new file isn't "all changes").
      const lane: Lane | null = fileMode ? (it.marker ?? null) : it.r.k === 1 ? "add" : it.r.k === 2 ? "del" : null;
      if (lane) add(lane, i, i + 1);
    } else if (it.t === "pair") {
      if (it.l?.k === 2) add("del", i, i + 1);
      if (it.r?.k === 1) add("add", i, i + 1);
    }
  });
  return marks;
}

interface ItemProps {
  /** Only this half of each pair: the rows of one split column. */
  side?: "old" | "new";
  ctx: Ctx;
  onExpand: (id: number) => void;
}

function ItemView({ item, height, side, ctx, onExpand }: ItemProps & { item: Item; height: number }) {
  if (item.t === "gap") return <Gap count={item.count} height={height} blank={side === "new"} onExpand={() => onExpand(item.id)} />;
  return (
    <div style={{ height }}>
      {side && item.t === "pair" ? <Half row={side === "old" ? item.l : item.r} side={side} ctx={ctx} column /> : <Line item={item} ctx={ctx} />}
    </div>
  );
}

/** Index of the item at content offset y. */
function itemAt(prefix: Float64Array, y: number) {
  let lo = 0;
  let hi = prefix.length - 2;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (prefix[mid] <= y) lo = mid;
    else hi = mid - 1;
  }
  return lo;
}

/**
 * Where an item of a previous layout is now: the same diff row, the gap that now hides it, or
 * for a gap that opened its first row; -1 if it's gone.
 */
function sameItem(it: Item | undefined, items: Item[]) {
  if (!it) return -1;
  // Gaps hold unchanged lines only, named by their first original line number.
  const unchanged = (x: Item) => {
    const r = x.t === "row" ? x.r : x.t === "pair" ? x.l : null;
    return r?.k === 0 ? r.o : -1;
  };
  if (it.t === "gap") return items.findIndex((x) => (x.t === "gap" ? x.id === it.id : unchanged(x) === it.id));
  const row = it.t === "row" ? it.r : (it.r ?? it.l);
  const o = unchanged(it);
  return items.findIndex((x) => (x.t === "gap" ? o >= x.id && o < x.id + x.count : x.t === "row" ? x.r === row : x.l === row || x.r === row));
}

/** A block of rows, mounted as a unit by the viewport buffer above. */
const Chunk = memo(function Chunk({ items, prefix, start, height, ...rest }: ItemProps & { items: Item[]; prefix: Float64Array; start: number; height: number }) {
  const rows: React.ReactNode[] = [];
  for (let i = start; i < Math.min(items.length, start + CHUNK); i++) {
    rows.push(
      <div key={i} data-i={i}>
        <ItemView item={items[i]} height={prefix[i + 1] - prefix[i]} {...rest} />
      </div>,
    );
  }
  // Off-screen chunks skip layout and paint: with every row mounted, scrolling a 1500-line
  // diff took 27ms a frame (sticky gutters, all laid out each frame); this brings it to ~9ms.
  // The height is set, not an intrinsic size: WebKit kept a skipped chunk at the size it last
  // rendered at after its contain-intrinsic-size changed (a unified ⇄ split switch).
  return (
    <div data-chunk={start / CHUNK} style={{ contentVisibility: "auto", height }}>
      {rows}
    </div>
  );
});

type Ctx = {
  oldLines: string[];
  newLines: string[];
  oldTok: (i: number, text: string) => TokenLine | undefined;
  newTok: (i: number, text: string) => TokenLine | undefined;
  fg?: string;
  digits: number;
  mode: CodeMode;
  wrapAt: (text: string, side: Side) => Wrap | null;
};

const ROW_BG = ["bg-background", "bg-add-bg", "bg-del-bg"] as const;
const GUTTER_BG = [
  "bg-background",
  "bg-add-gutter shadow-[inset_2px_0_0_var(--added)]",
  "bg-del-gutter shadow-[inset_2px_0_0_var(--removed)]",
] as const;
const SIGN = [" ", "+", "−"] as const;
// Gutter widths as line-number digits plus fixed pixels, for the markup and the wrap math alike
// (`ch` is the width of "0", which is what `cw` measures). The file gutter: 16px lead like VS Code,
// 12px after, the 3px bar, a 12px gap; diff gutters: pl-1, per number 20px padding, the 20px sign.
const GUTTER = { unified: [2, 64], half: [1, 44], file: [1, 43] } as const;
type Gutter = keyof typeof GUTTER;
const gutterCss = (g: Gutter, digits: number) => `calc(${GUTTER[g][0] * digits}ch + ${GUTTER[g][1]}px)`;
const gutterPx = (g: Gutter, digits: number, cw: number) => GUTTER[g][0] * digits * cw + GUTTER[g][1];
const CODE_PADDING = { paddingLeft: 8, paddingRight: 24 };
const CODE_PAD = CODE_PADDING.paddingLeft + CODE_PADDING.paddingRight;
const PROBE = "0".repeat(64);

const Line = memo(function Line({ item, ctx }: { item: Exclude<Item, { t: "gap" }>; ctx: Ctx }) {
  if (item.t === "pair") {
    return (
      <div className="grid min-h-full grid-cols-2">
        <Half row={item.l} side="old" ctx={ctx} />
        <Half row={item.r} side="new" ctx={ctx} border />
      </div>
    );
  }
  const r = item.r;
  const isOld = r.k === 2;
  const text = isOld ? ctx.oldLines[r.o - 1] ?? "" : ctx.newLines[r.n - 1] ?? "";
  const tokens = isOld ? ctx.oldTok(r.o - 1, text) : ctx.newTok(r.n - 1, text);

  if (ctx.mode === "file") {
    return (
      <div className="flex min-h-full w-full">
        <span className="sticky left-0 z-10 flex shrink-0 bg-background select-none" style={{ width: gutterCss("file", ctx.digits) }}>
          <span className="flex-1 pr-3 pl-4 text-right text-subtle/80">{r.n}</span>
          <span
            className={cn(
              "relative w-[3px] shrink-0",
              item.marker === "add" && "bg-added",
              item.marker === "mod" && "bg-primary",
              item.delAbove &&
                "before:absolute before:-top-[3px] before:left-0 before:border-y-[3px] before:border-l-[5px] before:border-y-transparent before:border-l-removed",
              item.delBelow &&
                "after:absolute after:-bottom-[3px] after:left-0 after:border-y-[3px] after:border-l-[5px] after:border-y-transparent after:border-l-removed",
            )}
          />
          <span className="w-3 shrink-0" />
        </span>
        <Code text={text} tokens={tokens} wrap={ctx.wrapAt(text, "row")} />
      </div>
    );
  }

  return (
    <div className={cn("flex min-h-full w-full", ROW_BG[r.k])}>
      <span className={cn("sticky left-0 z-10 flex shrink-0 pl-1 select-none", GUTTER_BG[r.k])} style={{ width: gutterCss("unified", ctx.digits) }}>
        {/* Old numbers stay faint except on removed lines, where they're the only reference. */}
        <Num n={r.o} digits={ctx.digits} k={r.k} faint={r.k === 0} />
        <Num n={r.n} digits={ctx.digits} k={r.k} />
        <span className={cn("w-5 shrink-0 text-center", r.k === 1 ? "text-added" : r.k === 2 ? "text-removed" : "")}>{SIGN[r.k]}</span>
      </span>
      <Code text={text} tokens={tokens} emph={r.e} emphClass={r.k === 1 ? "bg-add-emph" : "bg-del-emph"} wrap={ctx.wrapAt(text, "row")} />
    </div>
  );
});

/** One side of a pair; `column`: alone in a split column that scrolls sideways under its gutter. */
function Half({ row, side, ctx, border, column }: { row: DiffRow | null; side: "old" | "new"; ctx: Ctx; border?: boolean; column?: boolean }) {
  const borderCls = cn(border && "border-l border-border-strong", column && "h-full");
  if (!row) {
    return <div className={cn(borderCls, "bg-[repeating-linear-gradient(135deg,transparent_0_5px,var(--hatch)_5px_6px)]")} />;
  }
  const n = side === "old" ? row.o : row.n;
  const text = side === "old" ? ctx.oldLines[n - 1] ?? "" : ctx.newLines[n - 1] ?? "";
  const tokens = side === "old" ? ctx.oldTok(n - 1, text) : ctx.newTok(n - 1, text);
  return (
    <div className={cn("flex min-w-0", ROW_BG[row.k], borderCls)}>
      <span className={cn("flex shrink-0 pl-1 select-none", column && "sticky left-0 z-10", GUTTER_BG[row.k])} style={{ width: gutterCss("half", ctx.digits) }}>
        <Num n={n} digits={ctx.digits} k={row.k} />
        <span className={cn("w-5 shrink-0 text-center", row.k === 1 ? "text-added" : row.k === 2 ? "text-removed" : "")}>{SIGN[row.k]}</span>
      </span>
      <Code text={text} tokens={tokens} emph={row.e} emphClass={row.k === 1 ? "bg-add-emph" : "bg-del-emph"} wrap={ctx.wrapAt(text, side)} />
    </div>
  );
}

function Num({ n, digits, k, faint }: { n: number; digits: number; k: 0 | 1 | 2; faint?: boolean }) {
  return (
    <span
      className={cn("shrink-0 pr-2 pl-3 text-right tabular-nums", faint ? "text-subtle/35" : k === 0 ? "text-subtle/80" : "text-foreground/60")}
      style={{ width: `calc(${digits}ch + 20px)` }}
    >
      {n || ""}
    </span>
  );
}

/**
 * Word-level emphasis only where it helps: never on indentation, and not at all when most
 * of the line changed (then the row color already says it, and fragments are just noise).
 */
function usefulEmphasis(text: string, emph?: [number, number][]) {
  if (!emph?.length) return undefined;
  const indent = text.length - text.trimStart().length;
  const ranges = emph.map(([a, b]) => [Math.max(a, indent), b] as [number, number]).filter(([a, b]) => b > a);
  const changed = ranges.reduce((n, [a, b]) => n + text.slice(a, b).replace(/\s/g, "").length, 0);
  const total = text.replace(/\s/g, "").length;
  return total && changed / total <= 0.6 ? ranges : undefined;
}

type CodeProps = { text: string; tokens?: TokenLine; emph?: [number, number][]; emphClass?: string; wrap: Wrap | null };

// New tokens (a fresh result, the old side arriving) or a new revision re-render every mounted
// row; comparing by value keeps that to the lines whose colors or text actually changed.
const Code = memo(function Code({ text, tokens, emph, emphClass, wrap }: CodeProps) {
  emph = usefulEmphasis(text, emph);
  const all = tokens ?? [[text, "", 0]];
  // Wrapped rows clip: hanging whitespace (see wrapLine) must not widen the view.
  const cls = cn("min-w-0 flex-1 select-text", wrap && "overflow-x-clip");
  if (!wrap?.at.length) return <span className={cn(cls, "whitespace-pre")} style={CODE_PADDING}>{renderTokens(all, emph, emphClass)}</span>;
  // One line per piece where wrapLine broke it (CSS could break elsewhere). Inline blocks: blocks
  // make copying add a newline per piece; and the container can't be `pre`, or they'd sit side by side.
  const cuts = [0, ...wrap.at, text.length];
  return (
    <span className={cls} style={CODE_PADDING}>
      {cuts.slice(1).map((b, p) => (
        <span key={p} className="inline-block w-full align-top whitespace-pre" style={p ? { paddingLeft: `${wrap.indent}ch` } : undefined}>
          {renderTokens(sliceTokens(all, cuts[p], b), sliceRanges(emph, cuts[p], b), emphClass)}
        </span>
      ))}
    </span>
  );
}, sameCode);

function sameCode(a: CodeProps, b: CodeProps) {
  return (
    a.text === b.text &&
    a.emphClass === b.emphClass &&
    sameWrap(a.wrap, b.wrap) &&
    sameTuples(a.emph, b.emph) &&
    sameTuples(a.tokens, b.tokens)
  );
}

/** Tokens covering text[a, b). */
function sliceTokens(tokens: TokenLine, a: number, b: number): TokenLine {
  const out: TokenLine = [];
  let pos = 0;
  for (const [str, color, fs] of tokens) {
    const end = pos + str.length;
    if (end > a && pos < b) out.push([str.slice(Math.max(0, a - pos), b - pos), color, fs]);
    if (end >= b) break;
    pos = end;
  }
  return out;
}

/** Ranges within [a, b), relative to a. */
function sliceRanges(ranges: [number, number][] | undefined, a: number, b: number) {
  return ranges?.map(([x, y]) => [Math.max(x, a) - a, Math.min(y, b) - a] as [number, number]).filter(([x, y]) => y > x);
}

function sameWrap(a: Wrap | null, b: Wrap | null) {
  if (a === b) return true;
  return !!a && !!b && a.indent === b.indent && a.at.length === b.at.length && a.at.every((x, i) => x === b.at[i]);
}

function sameTuples<T extends unknown[]>(a: T[] | undefined, b: T[] | undefined) {
  if (a === b) return true;
  if (!a || !b || a.length !== b.length) return false;
  return a.every((t, i) => t.every((v, j) => v === b[i][j]));
}

const FONT_STYLE = (fs: number): React.CSSProperties | undefined =>
  fs ? { fontStyle: fs & 1 ? "italic" : undefined, fontWeight: fs & 2 ? 600 : undefined, textDecoration: fs & 4 ? "underline" : undefined } : undefined;

/** Splits syntax tokens at emphasis boundaries so word-level changes keep their colors. */
function renderTokens(tokens: TokenLine, emph: [number, number][] | undefined, emphClass?: string) {
  const out: React.ReactNode[] = [];
  let pos = 0;
  let ri = 0;
  for (const [str, color, fs] of tokens) {
    const style = color || fs ? { color: color || undefined, ...FONT_STYLE(fs) } : undefined;
    if (!emph?.length) {
      out.push(
        <span key={out.length} style={style}>
          {str}
        </span>,
      );
      pos += str.length;
      continue;
    }
    for (let i = 0; i < str.length; ) {
      const abs = pos + i;
      while (ri < emph.length && emph[ri][1] <= abs) ri++;
      const range = emph[ri];
      const inside = !!range && abs >= range[0];
      const end = Math.min(str.length, range ? (inside ? range[1] : range[0]) - pos : str.length);
      out.push(
        <span key={out.length} style={style} className={inside ? cn(emphClass, "rounded-[2px]") : undefined}>
          {str.slice(i, end)}
        </span>,
      );
      i = end;
    }
    pos += str.length;
  }
  return out;
}

/** `blank`: the right column's stretch of a gap the left column labels. */
function Gap({ count, height, blank, onExpand }: { count: number; height: number; blank?: boolean; onExpand: () => void }) {
  return (
    <button
      onClick={onExpand}
      style={{ height }}
      tabIndex={blank ? -1 : undefined}
      aria-hidden={blank || undefined}
      className="sticky left-0 flex w-full items-center gap-2 border-y border-border bg-panel px-4 font-sans text-[11.5px] text-subtle transition-colors hover:bg-elevated hover:text-foreground"
    >
      {!blank && (
        <>
          <ChevronsUpDown className="size-3.5" />
          {count} unchanged lines
        </>
      )}
    </button>
  );
}
