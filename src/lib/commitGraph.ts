// Lanes for the history graph. Pure, so it runs under `node --test`.

/** One commit's row. Lanes are columns; each keeps its column until it ends, so lines run straight. */
export interface GraphRow {
  /** The commit's own lane. */
  col: number;
  /** Lanes that pass the commit by, top to bottom. */
  through: number[];
  /** Lanes that end at the commit: it's the parent they were waiting for. */
  into: number[];
  /** Lanes the commit's parents continue in, below it. */
  out: number[];
  /** Columns the row draws in. */
  width: number;
}

/**
 * Rows for commits listed children first, as `git log` lists them. A parent listed above its child
 * (git's date order under clock skew) gets no line: nothing below would ever close it.
 */
export function graphRows(commits: readonly { sha: string; parents: readonly string[] }[]): GraphRow[] {
  // The commit each lane waits for; null is a free column.
  const lanes: (string | null)[] = [];
  const seen = new Set<string>();
  return commits.map(({ sha, parents }) => {
    seen.add(sha);
    const into = lanes.flatMap((s, i) => (s === sha ? [i] : []));
    const free = () => (lanes.includes(null) ? lanes.indexOf(null) : lanes.length);
    const col = into[0] ?? free();
    for (const i of into) lanes[i] = null;
    const through = lanes.flatMap((s, i) => (s === null ? [] : [i]));
    const out: number[] = [];
    parents.forEach((p, k) => {
      if (seen.has(p)) return;
      // The first parent carries the commit's own lane on; another joins the lane already waiting for it.
      const j = k === 0 ? col : lanes.includes(p) ? lanes.indexOf(p) : free();
      lanes[j] = p;
      out.push(j);
    });
    while (lanes.length && lanes.at(-1) === null) lanes.pop();
    return { col, through, into, out, width: Math.max(col, ...through, ...into, ...out) + 1 };
  });
}
