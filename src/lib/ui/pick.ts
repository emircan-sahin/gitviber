/** Picking rows with ⌘/⇧-click and ⇧-arrows, for lists that let several be picked (Changes, History). */

/** `picked` with `item` added, or taken out when it's in. */
export function toggled<T>(picked: T[], item: T, key: (t: T) => string): T[] {
  const k = key(item);
  return picked.some((p) => key(p) === k) ? picked.filter((p) => key(p) !== k) : [...picked, item];
}

/** The rows of `list` from `from` to `to`, either way round; only `to` when either isn't listed. */
export function rangeOf<T>(list: T[], from: T, to: T, key: (t: T) => string): T[] {
  const [i, j] = [list.findIndex((t) => key(t) === key(from)), list.findIndex((t) => key(t) === key(to))];
  return i < 0 || j < 0 ? [to] : list.slice(Math.min(i, j), Math.max(i, j) + 1);
}
