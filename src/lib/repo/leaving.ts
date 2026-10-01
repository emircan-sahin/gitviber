/**
 * Rows an action is taking out of their list (staged, unstaged, discarded), by key. `follow`: the open
 * row was one, with no other row of its list to move to yet; `seen`: the status shown when the action
 * settled, null while it runs.
 */
export interface Leaving<S> {
  keys: Set<string>;
  follow: boolean;
  seen: S | null;
}

type Keys = { has: (key: string) => boolean };

/**
 * The row now in the place of `from`, which left a list whose keys were `before`, in order: the next
 * one still there (`after`), else the one before it. None when the list has nothing left.
 */
export function rowInPlace(before: string[], after: Keys, from: string): string | undefined {
  const at = before.indexOf(from);
  if (at < 0) return undefined;
  return before.slice(at + 1).find((k) => after.has(k)) ?? before.slice(0, at).reverse().find((k) => after.has(k));
}

/**
 * Drops the records whose rows have all left (`present` has none of them), or that settled before
 * the status `now` came without them leaving (the file was rewritten meanwhile). True when rows that
 * left took the `open` one with them, and it still has to be moved on.
 */
export function settleLeaving<S>(records: Set<Leaving<S>>, present: Keys, now: S, open: string | null): boolean {
  let follow = false;
  for (const l of records) {
    const gone = ![...l.keys].some((k) => present.has(k));
    if (gone && l.follow && open !== null && l.keys.has(open)) follow = true;
    if (gone || (l.seen !== null && l.seen !== now)) records.delete(l);
  }
  return follow;
}
