import type { Anchor } from "@/lib/review/notes";

/**
 * What a file of the stacked diff keeps for the life of the app: its diff unmounts while it's far
 * off screen, and the whole view on a tab switch, and this is what comes back with it.
 */
export interface FileMemo {
  /** Opened or closed by hand; none: closed once viewed. */
  shut?: boolean;
  /** Its large diff was asked for. */
  large?: boolean;
  /** Unchanged lines (new line numbers) opened from their fold. */
  revealed?: ReadonlySet<number>;
  /** Its diff's height as last drawn: the room it keeps while it isn't, so nothing around it moves. */
  height?: number;
  /** What its status said when it was read (`rev`: the revision then): it's read again only when that changes. */
  sig?: string;
  rev?: number;
  /** A review note being written on it: its lines and side, and what's typed so far. */
  draft?: { old: boolean; anchor: Anchor; body: string };
}

const memos = new Map<string, FileMemo>();
// Every commit or range opened adds its files' keys for good: past this the oldest go (not a note being written).
const MAX_MEMOS = 50_000;
const EVICT = 10_000;

/** `key`: the worktree and the file's selection key. */
export function fileMemo(key: string): FileMemo {
  let m = memos.get(key);
  if (!m) {
    if (memos.size >= MAX_MEMOS) {
      let left = EVICT;
      for (const [k, old] of memos) {
        if (left-- <= 0) break;
        if (!old.draft) memos.delete(k);
      }
    }
    memos.set(key, (m = {}));
  }
  return m;
}

/** Where each list was left in each worktree: the file at the top of the view, and how far into it. */
export const scrolls = new Map<string, { file: string; offset: number }>();
