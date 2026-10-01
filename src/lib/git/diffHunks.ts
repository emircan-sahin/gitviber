// git's diff rows grouped the way a diff editor draws them: one hunk per run of changed lines,
// with the word-level changes worth showing. Pure, so it runs under `node --test`.
import type { DiffRow } from "../api";

/** A line and a 0-based UTF-16 column. */
export type Pos = [line: number, col: number];

interface Hunk {
  /** Old and new lines [start, end), 1-based; an empty range sits before `start`. */
  original: [number, number];
  modified: [number, number];
  /** Word-level changes as [old start, old end, new start, new end]; a side with none is empty. */
  inner: [Pos, Pos, Pos, Pos][];
}

const MOSTLY = 0.6;

/**
 * Word-level emphasis only where it helps: never on indentation, and not at all when most of the
 * line changed (the row color already says it, and fragments are just noise).
 */
export function usefulEmphasis(text: string, emph?: [number, number][]) {
  if (!emph?.length) return [];
  const indent = text.length - text.trimStart().length;
  const ranges = emph.map(([a, b]) => [Math.max(a, indent), b] as [number, number]).filter(([a, b]) => b > a);
  const solid = (s: string) => s.replace(/\s/g, "").length;
  const changed = ranges.reduce((n, [a, b]) => n + solid(text.slice(a, b)), 0);
  const total = solid(text);
  return total && changed / total <= MOSTLY ? ranges : [];
}

/** Hunks of `rows`, with emphasis paired line by line: the k-th removed line with the k-th added. */
export function hunks(rows: DiffRow[], oldLines: string[], newLines: string[]): Hunk[] {
  const out: Hunk[] = [];
  let o = 0;
  let n = 0;
  for (let i = 0; i < rows.length; ) {
    if (rows[i].k === 0) {
      ({ o, n } = rows[i++]);
      continue;
    }
    const dels: DiffRow[] = [];
    const adds: DiffRow[] = [];
    for (; i < rows.length && rows[i].k !== 0; i++) (rows[i].k === 2 ? dels : adds).push(rows[i]);
    const inner: Hunk["inner"] = [];
    for (let p = 0; p < Math.min(dels.length, adds.length); p++) {
      const d = dels[p];
      const a = adds[p];
      const dr = usefulEmphasis(oldLines[d.o - 1] ?? "", d.e);
      const ar = usefulEmphasis(newLines[a.n - 1] ?? "", a.e);
      // A side without a range of its own gets an empty one where its previous one ended.
      let od = 0;
      let ad = 0;
      for (let q = 0; q < Math.max(dr.length, ar.length); q++) {
        const [o0, o1] = dr[q] ?? [od, od];
        const [a0, a1] = ar[q] ?? [ad, ad];
        inner.push([[d.o, o0], [d.o, o1], [a.n, a0], [a.n, a1]]);
        [od, ad] = [o1, a1];
      }
    }
    out.push({
      original: dels.length ? [dels[0].o, dels[dels.length - 1].o + 1] : [o + 1, o + 1],
      modified: adds.length ? [adds[0].n, adds[adds.length - 1].n + 1] : [n + 1, n + 1],
      inner,
    });
    if (dels.length) o = dels[dels.length - 1].o;
    if (adds.length) n = adds[adds.length - 1].n;
  }
  return out;
}

/** A run of unchanged lines a stacked diff folds away: `count` of them, the first at old `o` and new `n`. */
export interface Gap {
  gap: number;
  o: number;
  n: number;
}

/**
 * `rows` as a unified diff shows them on its own: the changes with `context` lines around each,
 * the rest folded into gaps, except runs shorter than `minimum` and the new lines `revealed`.
 */
export function shownRows(rows: DiffRow[], context: number, minimum = 1, revealed: ReadonlySet<number> = new Set()): (DiffRow | Gap)[] {
  const keep = rows.map((r) => r.k !== 0 || revealed.has(r.n));
  rows.forEach((r, i) => {
    if (r.k === 0) return;
    for (let j = Math.max(0, i - context); j <= Math.min(rows.length - 1, i + context); j++) keep[j] = true;
  });
  const out: (DiffRow | Gap)[] = [];
  for (let i = 0; i < rows.length; ) {
    let end = i;
    while (end < rows.length && !keep[end]) end++;
    if (end > i && end - i >= minimum) out.push({ gap: end - i, o: rows[i].o, n: rows[i].n });
    else out.push(...rows.slice(i, end));
    if (end < rows.length) out.push(rows[end]);
    i = end + 1;
  }
  return out;
}

/** A piece of a highlighted line: its text, its token's color and font style, and whether it's a word change. */
export type Piece = [text: string, color: string, fontStyle: number, emphasis: boolean];

/** A line's tokens (none: plain `text`) cut where the word-level `ranges` start and end, so those pieces can be marked. */
export function emphasized(text: string, tokens: [string, string, number][] | undefined, ranges: [number, number][]): Piece[] {
  const runs = tokens ?? [[text, "", 0]];
  const out: Piece[] = [];
  let at = 0;
  for (const [t, color, fs] of runs) {
    const end = at + t.length;
    // Every range edge inside this token cuts it.
    const cuts = [at, ...ranges.flat().filter((c) => c > at && c < end), end].sort((a, b) => a - b);
    for (let c = 0; c < cuts.length - 1; c++) {
      const [a, b] = [cuts[c], cuts[c + 1]];
      if (b > a) out.push([t.slice(a - at, b - at), color, fs, ranges.some(([x, y]) => a >= x && b <= y)]);
    }
    at = end;
  }
  return out;
}
