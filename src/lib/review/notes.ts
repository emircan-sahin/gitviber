// Review notes: comments on lines of a diff, kept per worktree in app storage (never in the repo)
// and handed to the agent as a prompt. A note is anchored by its lines' content and the lines
// around them, not only their number, so it follows them as the agent edits around them, tells
// them from a same-looking line elsewhere (`}`, `return null;`), and turns outdated once they change.
// Pure, so it runs under `node --test`.
import { hash } from "../hash.ts";
import { basename } from "../path.ts";

/** Where a note is: its lines, and what was around them when it was written. */
export interface Anchor {
  /** First and last line, 1-based. A note on the files on disk follows its lines as they move. */
  start: number;
  end: number;
  /** The lines as they were: all of them, or the first KEEP of a longer run (its last KEEP in `tail`). */
  code: string[];
  tail?: string[];
  /** A longer run's lines all hashed: a rewrite between `code` and `tail` changes it. None: a short run, or a note from before this was kept. */
  hash?: string;
  /** Up to CONTEXT lines above and below as they were (fewer at the file's start and end); none: a note from before these were kept. */
  before?: string[];
  after?: string[];
}

export interface ReviewNote extends Anchor {
  id: string;
  path: string;
  body: string;
  /** On the old side of a diff, removed or replaced lines. */
  old?: boolean;
  /** Written on a commit's version ("commit 1a2b3c4"); none: on the files on disk. */
  at?: string;
  /** Its lines on disk changed since: the agent rewrote them, or the file is gone. */
  outdated?: boolean;
  resolved?: boolean;
}

/** A note as stored by this or an older build; anything else there is dropped. */
export function isNote(v: unknown): v is ReviewNote {
  const lines = (x: unknown) => Array.isArray(x) && x.every((l) => typeof l === "string");
  if (!v || typeof v !== "object" || Array.isArray(v)) return false;
  const n = v as Record<string, unknown>;
  return (
    typeof n.id === "string" &&
    typeof n.path === "string" &&
    typeof n.body === "string" &&
    Number.isInteger(n.start) &&
    Number.isInteger(n.end) &&
    (n.end as number) >= (n.start as number) &&
    lines(n.code) &&
    [n.tail, n.before, n.after].every((x) => x === undefined || lines(x)) &&
    (n.hash === undefined || typeof n.hash === "string")
  );
}

const CONTEXT = 2;
// A note on a whole file would keep all of it, in storage every workspace save shares.
const KEEP = 20;

/** A note's anchor on lines `start`–`end` of `lines`. */
export function anchorAt(lines: string[], start: number, end: number): Anchor {
  const all = lines.slice(start - 1, end);
  const long = all.length > 2 * KEEP;
  return {
    start,
    end,
    code: long ? all.slice(0, KEEP) : all,
    ...(long && { tail: all.slice(-KEEP), hash: hash(all.join("\n")) }),
    before: lines.slice(Math.max(0, start - 1 - CONTEXT), start - 1),
    after: lines.slice(end, end + CONTEXT),
  };
}

/** A note on the files on disk, which an agent's edits can make outdated; the rest are on versions that don't change. */
export const isLive = (n: ReviewNote) => !n.old && !n.at;

/** Where a note should be in a newer version of its file: near `expect`, between lines `lo` and `hi` (neither included). */
export interface Near {
  expect: number;
  lo: number;
  hi: number;
}

/**
 * Where `a`'s lines are in `lines`, or null once they're gone; nearest to where it's expected if
 * there are several. Its own lines must be there as they were, with the lines around them as
 * they were too. Without that, only where it's expected: in the stretch `near` says the edits
 * left it in (a line edited next to it doesn't lose it), else at its own line number. A
 * same-looking line elsewhere never takes it.
 */
export function findNote(lines: string[], a: Anchor, near?: Near): number | null {
  if (!a.code.length) return null;
  const size = a.end - a.start + 1;
  const tail = a.tail ?? [];
  const expect = near?.expect ?? a.start;
  const same = (at: number, want: string[]) => at >= 1 && want.every((w, i) => lines[at - 1 + i] === w);
  const own = (s: number) =>
    s >= 1 && s + size - 1 <= lines.length && same(s, a.code) && same(s + size - tail.length, tail) && (!a.hash || hash(lines.slice(s - 1, s - 1 + size).join("\n")) === a.hash);
  const around = (s: number) => (!a.before || same(s - a.before.length, a.before)) && (!a.after || same(s + size, a.after));
  const closer = (best: number | null, s: number) => (best === null || Math.abs(s - expect) < Math.abs(best - expect) ? s : best);
  let full: number | null = null;
  let inside: number | null = null;
  for (let s = 1; s + size - 1 <= lines.length; s++) {
    if (!own(s)) continue;
    if (around(s)) full = closer(full, s);
    if (near && s > near.lo && s + size - 1 < near.hi) inside = closer(inside, s);
  }
  return full ?? (near ? inside : own(expect) ? expect : null);
}

/**
 * Where lines `start`–`end` of `prev` went in `next`, by the nearest lines around them that each
 * version has once (as patience diff anchors a diff): the stretch between those, and the shift above.
 */
export function shifted(prev: string[], next: string[], start: number, end: number): Near {
  // Each line's index, or -1 once it's seen twice.
  const once = (ls: string[]) => {
    const m = new Map<string, number>();
    ls.forEach((l, i) => m.set(l, m.has(l) ? -1 : i));
    return m;
  };
  const [a, b] = [once(prev), once(next)];
  // The 0-based index in `next` of `prev[i]` when both have it once (blank lines tell nothing apart).
  const anchor = (i: number) => (prev[i].trim() && (a.get(prev[i]) ?? -1) >= 0 ? (b.get(prev[i]) ?? -1) : -1);
  const near: Near = { expect: start, lo: 0, hi: next.length + 1 };
  for (let i = Math.min(start - 1, prev.length) - 1; i >= 0; i--) {
    const j = anchor(i);
    if (j < 0) continue;
    near.lo = j + 1;
    near.expect = start + j - i;
    break;
  }
  for (let k = end; k < prev.length; k++) {
    const m = anchor(k);
    if (m < 0) continue;
    near.hi = m + 1;
    // Nothing to go by above: it ends where it did, counted from below.
    if (!near.lo) near.expect = start + m - k;
    break;
  }
  return near;
}

/**
 * Live notes on `path` checked against its text on disk now (null: it's gone; `prev`: as it was
 * last checked): each moves with its lines, or turns outdated when they're no longer there (and
 * back, if the agent puts them back). The same array when nothing changed.
 */
export function checkNotes(notes: ReviewNote[], path: string, lines: string[] | null, prev?: string[]): ReviewNote[] {
  let changed = false;
  const next = notes.map((n) => {
    if (n.path !== path || !isLive(n) || n.resolved) return n;
    const start = lines && findNote(lines, n, prev && shifted(prev, lines, n.start, n.end));
    const moved: ReviewNote = start === null ? { ...n, outdated: true } : { ...n, start, end: start + n.end - n.start, outdated: false };
    if (moved.start === n.start && !!moved.outdated === !!n.outdated) return n;
    changed = true;
    return moved;
  });
  return changed ? next : notes;
}

/** A note where a file on show has its lines: on the old side or the new, from `start` to `end`. */
export interface Placed {
  note: ReviewNote;
  old: boolean;
  start: number;
  end: number;
}

/** The open notes on a file on show (each side's path and lines; null: that side doesn't exist), where its versions have their lines. */
export function placeNotes(notes: ReviewNote[], file: { path: string; oldPath: string; oldLines: string[] | null; newLines: string[] | null }): Placed[] {
  return notes.flatMap((note) => {
    const old = !!note.old;
    const lines = old ? file.oldLines : file.newLines;
    if (note.resolved || !lines || note.path !== (old ? file.oldPath : file.path)) return [];
    const start = findNote(lines, note);
    return start === null ? [] : [{ note, old, start, end: start + note.end - note.start }];
  });
}

/** A fence the code can't close early: one backtick longer than any run of them in it. */
function fence(code: string) {
  const longest = Math.max(0, ...(code.match(/`+/g) ?? []).map((r) => r.length));
  return "`".repeat(Math.max(3, longest + 1));
}

/** "line 12" or "lines 12–14". */
export const noteLines = (n: Pick<ReviewNote, "start" | "end">) => (n.end > n.start ? `lines ${n.start}–${n.end}` : `line ${n.start}`);

/** Where a note is, as an agent reads a location: `path:12` or `path:12-14`. */
export const noteLocation = (n: ReviewNote) => `${n.path}:${n.start}${n.end > n.start ? `-${n.end}` : ""}`;

/** The note's code as the prompt quotes it: a long run's start and end, with how much is left out between. */
function quoted(n: ReviewNote) {
  if (!n.tail) return n.code.join("\n");
  const left = n.end - n.start + 1 - n.code.length - n.tail.length;
  return [...n.code, `⋯ ${left} more lines ⋯`, ...n.tail].join("\n");
}

/** The notes as a prompt for the agent: each one's place, its code quoted, then what was said about it. */
export function notesPrompt(notes: ReviewNote[]): string {
  return notes
    .map((n) => {
      const about = [n.old && "the old version", n.at && `in ${n.at}`, n.outdated && "outdated: these lines have changed since"].filter(Boolean).join("; ");
      const code = quoted(n);
      const f = fence(code);
      const ext = basename(n.path).match(/\.([\w+-]+)$/)?.[1] ?? "";
      return `\`${noteLocation(n)}\`${about ? ` (${about})` : ""}\n${f}${ext}\n${code}\n${f}\n${n.body.trim()}`;
    })
    .join("\n\n");
}

/**
 * Text to paste into a terminal: no control characters (an ESC could end the bracketed paste early
 * and run what follows), and no line break at the end, which a program would take as Enter.
 */
export function forTerminal(text: string): string {
  return text
    .replace(/\r\n?/g, "\n")
    .replace(/[\x00-\x08\x0b-\x1f\x7f]/g, "")
    .replace(/\n+$/, "");
}
