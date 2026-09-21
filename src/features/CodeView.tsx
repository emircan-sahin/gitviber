import { useVirtualizer } from "@tanstack/react-virtual";
import { ChevronsUpDown } from "lucide-react";
import { forwardRef, memo, useCallback, useEffect, useImperativeHandle, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { DiffPair, DiffRow } from "@/lib/api";
import { languageFor, type TokenLine, tokenLookup, useHighlight } from "@/lib/highlight";
import { CODE_FONTS, useSettings } from "@/lib/settings";
import { cn } from "@/lib/utils";
import { type Lane, type Mark, OverviewRuler } from "./OverviewRuler";

export type CodeMode = "unified" | "split" | "file";

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
  const lang = languageFor(path);
  const oldHl = useHighlight(mode === "file" ? null : pair.original.text, lang, s.codeTheme);
  const newHl = useHighlight(pair.modified.text, lang, s.codeTheme);
  const oldTok = useMemo(() => tokenLookup(oldHl), [oldHl]);
  const newTok = useMemo(() => tokenLookup(newHl), [newHl]);

  const items = useMemo(() => {
    if (mode === "file") return buildFile(pair.rows, pair.original.exists && pair.modified.exists);
    const unified = buildUnified(pair.rows, collapse, expanded);
    return mode === "split" ? toSplit(unified) : unified;
  }, [pair, mode, collapse, expanded]);

  const lh = Math.round(s.codeFontSize * s.lineHeight);
  const gapH = lh + 12;
  const measured = wrap || mode === "split";
  const digits = String(Math.max(oldLines.length, newLines.length, 1)).length;
  const maxLen = useMemo(() => {
    // Tabs render 4 columns wide (tab-size), so count them that way.
    const width = (l: string) => l.length + 3 * (l.split("\t").length - 1);
    let m = 0;
    for (const l of newLines) m = Math.max(m, width(l));
    if (mode !== "file") for (const l of oldLines) m = Math.max(m, width(l));
    return m;
  }, [oldLines, newLines, mode]);

  // Small/medium files stay fully in the DOM so scrolling is purely native (no rows
  // appearing late on fast flicks); only huge files fall back to virtualization.
  const virtual = items.length > FULL_LIMIT;
  const virtualizer = useVirtualizer({
    count: virtual ? items.length : 0,
    getScrollElement: () => scrollRef.current,
    estimateSize: (i) => (items[i]?.t === "gap" ? gapH : lh),
    overscan: 120,
  });

  // Row heights depend on font metrics; re-measure when they change.
  useEffect(() => {
    // Also on content change: sizes are cached by index, which now holds different items.
    if (virtual) virtualizer.measure();
  }, [lh, measured, mode, virtual, virtualizer, items]);

  // Show the file once, colored, instead of plain text that recolors a moment later.
  const [grace, setGrace] = useState(true);
  useEffect(() => {
    const t = setTimeout(() => setGrace(false), 250);
    return () => clearTimeout(t);
  }, []);
  const waiting = grace && lang !== "text" && !newHl;

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

  const scrollToItem = useCallback(
    (i: number) => {
      const target = Math.max(0, i - CONTEXT);
      const sc = scrollRef.current;
      if (virtual) virtualizer.scrollToIndex(target, { align: "start" });
      else if (!measured) sc!.scrollTop = items.slice(0, target).reduce((h, it) => h + (it.t === "gap" ? gapH : lh), 0);
      else if (sc) {
        const el = sc.querySelector<HTMLElement>(`[data-i="${target}"]`);
        if (el) sc.scrollTop += el.getBoundingClientRect().top - sc.getBoundingClientRect().top;
        else {
          // Not mounted yet: land on its placeholder; the buffer fills it on arrival.
          const c = Math.floor(target / CHUNK);
          const ph = sc.querySelector<HTMLElement>(`[data-chunk="${c}"]`);
          if (ph) sc.scrollTop = ph.offsetTop + items.slice(c * CHUNK, target).reduce((h, it) => h + (it.t === "gap" ? gapH : lh), 0);
        }
      }
    },
    [virtual, virtualizer, measured, items, gapH, lh],
  );

  // First open: restore the remembered position, else jump near the first change.
  const positioned = useRef(false);
  useLayoutEffect(() => {
    if (positioned.current || waiting || !items.length) return;
    positioned.current = true;
    const saved = scrollMemory.get(scrollKey);
    if (saved != null) scrollRef.current!.scrollTop = saved;
    else if (changeStarts[0] > CONTEXT) scrollToItem(changeStarts[0]);
  }, [waiting, items.length, changeStarts, scrollKey, scrollToItem]);

  useImperativeHandle(
    ref,
    () => {
      // The row under the top of the viewport, whatever the layout mode.
      const current = () => {
        const r = scrollRef.current!.getBoundingClientRect();
        // scrollToItem puts a change CONTEXT rows below the top: sample exactly that row.
        const hit = document.elementFromPoint(r.left + r.width / 2, r.top + lh * CONTEXT + 1);
        const row = hit?.closest<HTMLElement>("[data-i]");
        if (row) return Number(row.dataset.i);
        const chunk = hit?.closest<HTMLElement>("[data-chunk]");
        return chunk ? Number(chunk.dataset.chunk) * CHUNK : 0;
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
    [changeStarts, scrollToItem, lh],
  );

  const ctx = useMemo(
    () => ({ oldLines, newLines, oldTok, newTok, fg: newHl?.data.fg ?? oldHl?.data.fg, digits, mode, wrap }),
    [oldLines, newLines, oldTok, newTok, newHl?.data.fg, oldHl?.data.fg, digits, mode, wrap],
  );
  const expand = useCallback((id: number) => setExpanded((e) => new Set(e).add(id)), []);
  const chunkCount = Math.ceil(items.length / CHUNK);

  // Chunks mount lazily. Everything within two viewports of the scroll position is
  // rendered ahead of time (so rows never appear while you look); the rest fills in
  // one chunk at a time, only while you're not scrolling.
  const [origin] = useState(() => {
    const saved = scrollMemory.get(scrollKey);
    if (saved != null) return Math.floor(saved / lh / CHUNK);
    return Math.floor(Math.max(0, (changeStarts[0] ?? 0) - CONTEXT) / CHUNK);
  });
  const around = (c: number) => new Set([c - 1, c, c + 1].filter((x) => x >= 0 && x < chunkCount));
  const filledRef = useRef<Set<number>>(around(origin));
  // New items (unified ⇄ split, a gap expanded) re-render every mounted row: start again from
  // the chunks on screen, measured 0.5–3s of frozen UI on a 1500-line file otherwise.
  const filledFor = useRef(items);
  if (filledFor.current !== items) {
    filledFor.current = items;
    filledRef.current = around(Math.floor((scrollRef.current?.scrollTop ?? 0) / lh / CHUNK));
  }
  const [, setFillTick] = useState(0);
  const lastScroll = useRef(0);
  const anchor = useRef<{ el: Element; top: number } | null>(null);

  /** Chunk indices overlapping [top − 2 viewports, bottom + 2 viewports], nearest first. */
  const nearChunks = useCallback(() => {
    const sc = scrollRef.current;
    if (!sc) return [];
    const mid = sc.scrollTop + sc.clientHeight / 2;
    const reach = sc.clientHeight * 2.5;
    const out: [number, number][] = [];
    sc.querySelectorAll<HTMLElement>("[data-chunk]").forEach((el) => {
      const top = el.offsetTop;
      const bottom = top + el.offsetHeight;
      const dist = mid < top ? top - mid : mid > bottom ? mid - bottom : 0;
      if (dist <= reach) out.push([Number(el.dataset.chunk), dist]);
    });
    return out.sort((x, y) => x[1] - y[1]).map((x) => x[0]);
  }, []);

  const fill = useCallback(
    (chunks: number[], max = Infinity) => {
      const fresh = chunks.filter((c) => c >= 0 && c < chunkCount && !filledRef.current.has(c)).slice(0, max);
      if (!fresh.length) return;
      // WebKit has no scroll anchoring: remember the row on screen so a chunk growing
      // above it (wrapped rows taller than estimated) can't push it away.
      const sc = scrollRef.current;
      if (sc && measured) {
        const r = sc.getBoundingClientRect();
        const el = document.elementFromPoint(r.left + r.width / 2, r.top + 4)?.closest("[data-i],[data-chunk]");
        // Content coordinates: the user may scroll before this render commits, and that
        // scroll must not be "corrected" away.
        if (el) anchor.current = { el, top: el.getBoundingClientRect().top - r.top + sc.scrollTop };
      }
      fresh.forEach((c) => filledRef.current.add(c));
      setFillTick((t) => t + 1);
    },
    [chunkCount, measured],
  );

  // After every render: restore the anchor, then top up the buffer around the viewport.
  useLayoutEffect(() => {
    if (virtual || waiting) return;
    const a = anchor.current;
    anchor.current = null;
    if (a?.el.isConnected) {
      const sc = scrollRef.current!;
      const delta = a.el.getBoundingClientRect().top - sc.getBoundingClientRect().top + sc.scrollTop - a.top;
      if (delta) sc.scrollTop += delta;
    }
    fill(nearChunks());
  });

  // Background fill while idle, one chunk per tick so a task never gets long.
  useEffect(() => {
    if (virtual || waiting) return;
    const id = setInterval(() => {
      let unfilled = false;
      for (let c = 0; c < chunkCount && !unfilled; c++) unfilled = !filledRef.current.has(c);
      if (!unfilled) return clearInterval(id);
      if (performance.now() - lastScroll.current < 250) return;
      const sc = scrollRef.current;
      const mid = sc ? Math.floor((sc.scrollTop / Math.max(1, sc.scrollHeight)) * chunkCount) : 0;
      let best = -1;
      for (let c = 0; c < chunkCount; c++) if (!filledRef.current.has(c) && (best < 0 || Math.abs(c - mid) < Math.abs(best - mid))) best = c;
      if (best >= 0) fill([best]);
    }, 40);
    return () => clearInterval(id);
  }, [virtual, waiting, chunkCount, fill]);

  const onScroll = (e: React.UIEvent<HTMLDivElement>) => {
    scrollMemory.set(scrollKey, e.currentTarget.scrollTop);
    lastScroll.current = performance.now();
    // Keep the buffer ahead of the scroll; one chunk per event keeps frames short.
    if (!virtual) fill(nearChunks(), 1);
  };

  const gutterW = mode === "file" ? `calc(${digits}ch + 28px)` : `calc(${digits * 2}ch + 60px)`;

  const overview = useMemo(() => overviewMarks(items, mode === "file"), [items, mode]);
  const prefix = useMemo(() => {
    const p = new Float64Array(items.length + 1);
    items.forEach((it, i) => (p[i + 1] = p[i] + (it.t === "gap" ? gapH : lh)));
    return p;
  }, [items, lh, gapH]);

  // Real top offset of item i in the scroll content (i === items.length gives the end),
  // so ruler marks line up with the code and with the viewport box.
  const offsetOf = useCallback(
    (i: number) => {
      if (!measured) return prefix[i];
      if (virtual) {
        const m = virtualizer.measurementsCache;
        return i < m.length ? m[i].start : (m[m.length - 1]?.end ?? 0);
      }
      const sc = scrollRef.current;
      const el = rowIndex(sc).get(i);
      if (el) return el.offsetTop;
      if (i >= items.length) return (sc?.firstElementChild as HTMLElement | null)?.offsetHeight ?? prefix[i];
      return prefix[i]; // chunk not filled yet; the ruler redraws once it is
    },
    [measured, virtual, virtualizer, prefix, items.length],
  );

  return (
    <div className="flex h-full">
      <div
        ref={scrollRef}
        onScroll={onScroll}
        className="code-scroll relative h-full min-w-0 flex-1 overflow-auto bg-background"
        style={{
          fontFamily: CODE_FONTS[s.codeFont],
          fontSize: s.codeFontSize,
          lineHeight: `${lh}px`,
          fontVariantLigatures: s.ligatures ? "normal" : "none",
          tabSize: 4,
          color: ctx.fg,
        }}
      >
        {!waiting && (
          <div className="relative" style={{ width: measured ? "100%" : `max(100%, calc(${gutterW} + ${maxLen}ch + 48px))` }}>
            {virtual ? (
              <div className="relative" style={{ height: virtualizer.getTotalSize() }}>
                {virtualizer.getVirtualItems().map((v) => (
                  <div
                    key={v.key}
                    data-i={v.index}
                    data-index={v.index}
                    ref={measured ? virtualizer.measureElement : undefined}
                    className="absolute top-0 left-0 w-full"
                    style={{ transform: `translateY(${v.start}px)` }}
                  >
                    <ItemView item={items[v.index]} ctx={ctx} lh={lh} gapH={gapH} measured={measured} onExpand={expand} />
                  </div>
                ))}
              </div>
            ) : (
              Array.from({ length: chunkCount }, (_, c) =>
                filledRef.current.has(c) ? (
                  <Chunk key={c} items={items} start={c * CHUNK} height={chunkHeight(items, c * CHUNK, lh, gapH)} ctx={ctx} lh={lh} gapH={gapH} measured={measured} onExpand={expand} />
                ) : (
                  <div key={c} data-chunk={c} style={{ height: chunkHeight(items, c * CHUNK, lh, gapH) }} />
                ),
              )
            )}
          </div>
        )}
      </div>
      <OverviewRuler marks={overview} offsetOf={offsetOf} split={mode !== "file"} ready={!waiting} scrollRef={scrollRef} />
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
  ctx: Ctx;
  lh: number;
  gapH: number;
  measured: boolean;
  onExpand: (id: number) => void;
}

function ItemView({ item, ctx, lh, gapH, measured, onExpand }: ItemProps & { item: Item }) {
  if (item.t === "gap") return <Gap count={item.count} height={gapH} onExpand={() => onExpand(item.id)} />;
  return (
    <div style={{ height: measured ? undefined : lh }}>
      <Line item={item} ctx={ctx} />
    </div>
  );
}

// data-i → row element, rebuilt at most once per frame (the ruler asks for many rows at once).
let rowCache: { at: number; root: Element | null; map: Map<number, HTMLElement> } = { at: -1, root: null, map: new Map() };
function rowIndex(root: HTMLElement | null) {
  const now = performance.now();
  if (rowCache.root !== root || now - rowCache.at > 16) {
    const map = new Map<number, HTMLElement>();
    root?.querySelectorAll<HTMLElement>("[data-i]").forEach((el) => map.set(Number(el.dataset.i), el));
    rowCache = { at: now, root, map };
  }
  return rowCache.map;
}

function chunkHeight(items: Item[], start: number, lh: number, gapH: number) {
  let height = 0;
  for (let i = start; i < Math.min(items.length, start + CHUNK); i++) height += items[i].t === "gap" ? gapH : lh;
  return height;
}

/** A block of rows, mounted as a unit by the viewport buffer above. */
const Chunk = memo(function Chunk({ items, start, height, ...rest }: ItemProps & { items: Item[]; start: number; height: number }) {
  const rows: React.ReactNode[] = [];
  for (let i = start; i < Math.min(items.length, start + CHUNK); i++) {
    rows.push(
      <div key={i} data-i={i}>
        <ItemView item={items[i]} {...rest} />
      </div>,
    );
  }
  // Off-screen chunks skip layout and paint: with every row mounted, scrolling a 1500-line
  // diff took 27ms a frame (sticky gutters, all laid out each frame); this brings it to ~9ms.
  // The intrinsic size stands in until the chunk has been rendered once, then `auto` keeps its real size.
  return (
    <div data-chunk={start / CHUNK} style={{ contentVisibility: "auto", containIntrinsicSize: `auto ${height}px` }}>
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
  wrap: boolean;
};

const ROW_BG = ["bg-background", "bg-add-bg", "bg-del-bg"] as const;
const GUTTER_BG = [
  "bg-background",
  "bg-add-gutter shadow-[inset_2px_0_0_var(--added)]",
  "bg-del-gutter shadow-[inset_2px_0_0_var(--removed)]",
] as const;
const SIGN = [" ", "+", "−"] as const;

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
        <span className="sticky left-0 z-10 flex shrink-0 bg-background select-none" style={{ width: `calc(${ctx.digits}ch + 28px)` }}>
          <span className="flex-1 pr-3 text-right text-subtle/80">{r.n}</span>
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
        <Code text={text} tokens={tokens} wrap={ctx.wrap} />
      </div>
    );
  }

  return (
    <div className={cn("flex min-h-full w-full", ROW_BG[r.k])}>
      <span className={cn("sticky left-0 z-10 flex shrink-0 select-none", GUTTER_BG[r.k])}>
        {/* Old numbers stay faint except on removed lines, where they're the only reference. */}
        <Num n={r.o} digits={ctx.digits} k={r.k} faint={r.k === 0} />
        <Num n={r.n} digits={ctx.digits} k={r.k} />
        <span className={cn("w-5 shrink-0 text-center", r.k === 1 ? "text-added" : r.k === 2 ? "text-removed" : "")}>{SIGN[r.k]}</span>
      </span>
      <Code text={text} tokens={tokens} emph={r.e} emphClass={r.k === 1 ? "bg-add-emph" : "bg-del-emph"} wrap={ctx.wrap} />
    </div>
  );
});

function Half({ row, side, ctx, border }: { row: DiffRow | null; side: "old" | "new"; ctx: Ctx; border?: boolean }) {
  const borderCls = border && "border-l border-border-strong";
  if (!row) {
    return <div className={cn(borderCls, "bg-[repeating-linear-gradient(135deg,transparent_0_5px,var(--hatch)_5px_6px)]")} />;
  }
  const n = side === "old" ? row.o : row.n;
  const text = side === "old" ? ctx.oldLines[n - 1] ?? "" : ctx.newLines[n - 1] ?? "";
  const tokens = side === "old" ? ctx.oldTok(n - 1, text) : ctx.newTok(n - 1, text);
  return (
    <div className={cn("flex min-w-0", ROW_BG[row.k], borderCls)}>
      <span className={cn("flex shrink-0 select-none", GUTTER_BG[row.k])}>
        <Num n={n} digits={ctx.digits} k={row.k} />
        <span className={cn("w-5 shrink-0 text-center", row.k === 1 ? "text-added" : row.k === 2 ? "text-removed" : "")}>{SIGN[row.k]}</span>
      </span>
      <Code text={text} tokens={tokens} emph={row.e} emphClass={row.k === 1 ? "bg-add-emph" : "bg-del-emph"} wrap />
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

function Code({ text, tokens, emph, emphClass, wrap }: { text: string; tokens?: TokenLine; emph?: [number, number][]; emphClass?: string; wrap: boolean }) {
  emph = usefulEmphasis(text, emph);
  return (
    <span className={cn("min-w-0 flex-1 pr-6 pl-2 select-text", wrap ? "whitespace-pre-wrap [overflow-wrap:anywhere]" : "whitespace-pre")}>
      {renderTokens(tokens ?? [[text, "", 0]], emph, emphClass)}
    </span>
  );
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

function Gap({ count, height, onExpand }: { count: number; height: number; onExpand: () => void }) {
  return (
    <button
      onClick={onExpand}
      style={{ height }}
      className="sticky left-0 flex w-full items-center gap-2 border-y border-border bg-panel px-4 font-sans text-[11.5px] text-subtle transition-colors hover:bg-elevated hover:text-foreground"
    >
      <ChevronsUpDown className="size-3.5" />
      {count} unchanged lines
    </button>
  );
}
