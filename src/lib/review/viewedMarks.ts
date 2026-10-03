// Marks on files of commits and ranges are never cleared by a change, so they're kept to the latest.
const MAX_FIXED = 2000;
const fixedKey = (k: string) => k.startsWith("commit:") || k.startsWith("pr-file:");

/** `marks` with all but the latest MAX_FIXED marks of commits' and ranges' files dropped (Map order is the order they were set). */
export function pruned(marks: Map<string, string>, max = MAX_FIXED) {
  const fixed = [...marks.keys()].filter(fixedKey);
  for (const k of fixed.slice(0, Math.max(0, fixed.length - max))) marks.delete(k);
  return marks;
}
