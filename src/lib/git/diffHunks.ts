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
