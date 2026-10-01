// Review notes: comments on lines of a diff, kept per worktree in app storage (never in the repo)
// and handed to the agent as a prompt. A note is anchored by its lines' content, not only their
// number, so it follows them as the agent edits around them and turns outdated once they change.
// Pure, so it runs under `node --test`.
import { basename } from "../path.ts";

export interface ReviewNote {
  id: string;
  path: string;
  /** First and last line, 1-based. A note on the files on disk follows its lines as they move. */
  start: number;
  end: number;
  /** The lines as they were when it was written: they place it in any version of the file, and the prompt quotes them. */
  code: string[];
  body: string;
  /** On the old side of a diff, removed or replaced lines. */
  old?: boolean;
  /** Written on a commit's version ("commit 1a2b3c4"); none: on the files on disk. */
  at?: string;
  /** Its lines on disk changed since: the agent rewrote them, or the file is gone. */
  outdated?: boolean;
  resolved?: boolean;
}

/** A note on the files on disk, which an agent's edits can make outdated; the rest are on versions that don't change. */
export const isLive = (n: ReviewNote) => !n.old && !n.at;

/** Where `code` is in `lines`: at `start` while it's still there, else the nearest place it moved to; null once it's gone. */
export function findLines(lines: string[], code: string[], start: number): number | null {
  if (!code.length) return null;
  const at = (s: number) => code.every((c, i) => lines[s - 1 + i] === c);
  if (at(start)) return start;
  let best: number | null = null;
  for (let s = 1; s + code.length - 1 <= lines.length; s++) if (at(s) && (best === null || Math.abs(s - start) < Math.abs(best - start))) best = s;
  return best;
}

/**
 * Live notes on `path` checked against its text on disk now (null: it's gone): each moves with its
 * lines, or turns outdated when they're no longer there (and back, if the agent puts them back).
 * The same array when nothing changed.
 */
export function checkNotes(notes: ReviewNote[], path: string, lines: string[] | null): ReviewNote[] {
  let changed = false;
  const next = notes.map((n) => {
    if (n.path !== path || !isLive(n) || n.resolved) return n;
    const start = lines && findLines(lines, n.code, n.start);
    const moved: ReviewNote = start === null ? { ...n, outdated: true } : { ...n, start, end: start + n.code.length - 1, outdated: false };
    if (moved.start === n.start && !!moved.outdated === !!n.outdated) return n;
    changed = true;
    return moved;
  });
  return changed ? next : notes;
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

/** The notes as a prompt for the agent: each one's place, its code quoted, then what was said about it. */
export function notesPrompt(notes: ReviewNote[]): string {
  return notes
    .map((n) => {
      const about = [n.old && "the old version", n.at && `in ${n.at}`, n.outdated && "outdated: these lines have changed since"].filter(Boolean).join("; ");
      const code = n.code.join("\n");
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
