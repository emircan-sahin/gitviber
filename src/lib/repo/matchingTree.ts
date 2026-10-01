import type { Entry } from "../api/types.ts";
import { basename, compareEntries, dirname } from "../path.ts";

/** The explorer's filter lists this many entries at most: the tree renders every row it has. */
export const MAX_MATCHES = 1000;

/**
 * The entries that match and the folders down to them, all open, in the explorer's order.
 * The root is always listed, empty with no match: the tree's own listing must not show through.
 */
export function matchingTree(listed: Entry[], matches: (path: string) => boolean) {
  const children: Record<string, Entry[]> = { "": [] };
  const add = (entry: Entry) => (children[dirname(entry.path)] ??= []).push(entry);
  const expanded = new Set([""]);
  let found = 0;
  for (const entry of listed) {
    if (!matches(entry.path)) continue;
    if (++found > MAX_MATCHES) break;
    add(entry);
    for (let dir = dirname(entry.path); dir && !expanded.has(dir); dir = dirname(dir)) {
      expanded.add(dir);
      add({ name: basename(dir), path: dir, isDir: true, ignored: false });
    }
  }
  for (const list of Object.values(children)) list.sort(compareEntries);
  return { children, expanded, found: Math.min(found, MAX_MATCHES), capped: found > MAX_MATCHES };
}
