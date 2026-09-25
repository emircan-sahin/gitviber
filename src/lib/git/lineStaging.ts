// Which lines staging, unstaging or discarding part of a diff takes, as VS Code's "Stage Selected
// Ranges": a change the chosen lines touch goes with the chosen ones of its side and all of the
// other side's (the lines they replace). Pure, so it runs under `node --test`.
import type { DiffRow } from "../api";

/** A run of changed lines: old lines [start, end) removed, new lines [start, end) added, 1-based; an empty range sits before `start`. */
export interface Change {
  original: [number, number];
  modified: [number, number];
}

export type Side = keyof Change;

/** The lines an action takes: removed ones by their old line, added ones by their new line. */
export interface Picked {
  removed: number[];
  added: number[];
}

export function changes(rows: DiffRow[]): Change[] {
  const out: Change[] = [];
  // The next old and new line.
  let o = 1;
  let n = 1;
  for (let i = 0; i < rows.length; ) {
    if (rows[i].k === 0) {
      o = rows[i].o + 1;
      n = rows[i++].n + 1;
      continue;
    }
    const [o0, n0] = [o, n];
    for (; i < rows.length && rows[i].k !== 0; i++) {
      if (rows[i].k === 2) o = rows[i].o + 1;
      else n = rows[i].n + 1;
    }
    out.push({ original: [o0, o], modified: [n0, n] });
  }
  return out;
}

const lines = ([a, b]: [number, number]) => Array.from({ length: Math.max(0, b - a) }, (_, i) => a + i);
const other = (side: Side): Side => (side === "modified" ? "original" : "modified");

function picked(side: Side, mine: number[], theirs: number[]): Picked {
  return side === "modified" ? { removed: theirs, added: mine } : { removed: mine, added: theirs };
}

/** What choosing lines [from, to] of `side` takes. A change with none of that side's lines counts when the line above it is chosen. */
export function pick(list: Change[], side: Side, from: number, to: number): Picked {
  const out: Picked = { removed: [], added: [] };
  for (const c of list) {
    const [a, b] = c[side];
    const mine = a < b ? lines([Math.max(a, from), Math.min(b, to + 1)]) : [];
    const touched = a < b ? mine.length > 0 : Math.max(a - 1, 1) >= from && Math.max(a - 1, 1) <= to;
    if (!touched) continue;
    const p = picked(side, mine, lines(c[other(side)]));
    out.removed.push(...p.removed);
    out.added.push(...p.added);
  }
  return out;
}

/** The change at `line` of `side`: one with lines there, else one that sits right above or below it. */
export function changeAt(list: Change[], side: Side, line: number): Change | null {
  const has = list.find((c) => c[side][0] <= line && line < c[side][1]);
  if (has) return has;
  return list.find(({ [side]: [a, b] }) => a === b && (line === a - 1 || line === a)) ?? null;
}

/** All of a change. */
export const whole = (c: Change): Picked => ({ removed: lines(c.original), added: lines(c.modified) });

export const isEmpty = (p: Picked) => !p.removed.length && !p.added.length;
