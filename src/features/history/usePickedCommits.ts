import { useEffect, useRef, useState } from "react";
import type { Commit } from "@/lib/api";
import { primaryKey } from "@/lib/platform";
import { rangeOf, toggled } from "@/lib/ui/pick";

const sha = (c: Commit) => c.sha;
const same = (s: string) => s;

/**
 * Commits picked together in History: ⌘-click (Ctrl off macOS) picks one or puts it back, ⇧-click
 * and ⇧↑↓ the range from the last one clicked, along commits on the same side of HEAD; a plain
 * click or move lets them go. Picks a rewrite replaced leave with their commits. `open`: the
 * open commit, which a first ⌘-click picks along.
 */
export function usePickedCommits(commits: Commit[], open: string | null) {
  const [picked, setPicked] = useState<string[]>([]);
  // Where a range starts.
  const from = useRef<string | null>(null);
  // An undo can bring rewritten commits back: their old picks don't come with them.
  useEffect(() => {
    const listed = new Set(commits.map(sha));
    setPicked((p) => (p.every((s) => listed.has(s)) ? p : p.filter((s) => listed.has(s))));
  }, [commits]);
  const chosen = new Set(picked);
  // As listed, newest first.
  const selection = commits.filter((c) => chosen.has(c.sha));
  /** Lets the picks go, and where a range would start; false when there were none. */
  const clear = () => {
    from.current = null;
    if (!selection.length) return false;
    setPicked([]);
    return true;
  };

  const range = (to: Commit) => {
    const start = commits.find((c) => c.sha === (from.current ?? open)) ?? to;
    from.current = start.sha;
    setPicked(rangeOf(commits, start, to, sha).filter((c) => c.notInHead === to.notInHead).map(sha));
  };

  /** True when the click picked; a plain one is the list's to open the commit with. */
  const click = (c: Commit, e: { metaKey: boolean; ctrlKey: boolean; shiftKey: boolean }) => {
    if (primaryKey(e)) {
      setPicked(toggled(selection.length ? selection.map(sha) : open ? [open] : [], c.sha, same));
      from.current = c.sha;
    } else if (e.shiftKey) range(c);
    else {
      clear();
      from.current = c.sha;
      return false;
    }
    return true;
  };

  /** useListNav's moves, by row key: a commit's is `commit:<sha>`. */
  const onMove = (fromKey: string, toKey: string, shift: boolean) => {
    const of = (key: string) => commits.find((c) => `commit:${c.sha}` === key);
    const to = of(toKey);
    if (!shift) {
      clear();
      if (to) from.current = to.sha;
    } else if (to) {
      // With nothing picked, the range starts on the row the keys left.
      if (!selection.length) from.current = of(fromKey)?.sha ?? null;
      range(to);
    }
  };

  return { selection, many: selection.length > 1, pickedSet: new Set(selection.map(sha)), click, onMove, onEscape: clear, clear };
}
