// Lanes for the history graph. Pure, so it runs under `node --test`.

/** A lane's line in one row: its column, and which branch it is (0 is the list's own). */
export interface Lane {
  col: number;
  id: number;
}

/** One commit's row. Lanes are columns; each keeps its column until it ends, so lines run straight. */
export interface GraphRow {
  /** The commit's own lane. */
  col: number;
  /** The branch the commit is on, carried on by its first parent. */
  id: number;
  /** Lanes that pass the commit by, top to bottom. */
  through: Lane[];
  /** Lanes that end at the commit: it's the parent they were waiting for. */
  into: Lane[];
  /** Lanes the commit's parents continue in, below it. */
  out: Lane[];
  /** Columns the row draws in. */
  width: number;
}

/**
 * Rows for commits listed children first, as `git log` lists them. A parent listed above its child
 * (git's date order under clock skew) gets no line: nothing below would ever close it.
 */
export function graphRows(commits: readonly { sha: string; parents: readonly string[] }[]): GraphRow[] {
  // The commit each lane waits for; null is a free column.
  const lanes: ({ sha: string; id: number } | null)[] = [];
  const seen = new Set<string>();
  let next = 0;
  return commits.map(({ sha, parents }) => {
    seen.add(sha);
    const into = lanes.flatMap((l, col) => (l?.sha === sha ? [{ col, id: l.id }] : []));
    const free = () => (lanes.includes(null) ? lanes.indexOf(null) : lanes.length);
    const col = into[0]?.col ?? free();
    const id = into[0]?.id ?? next++;
    for (const l of into) lanes[l.col] = null;
    const through = lanes.flatMap((l, c) => (l ? [{ col: c, id: l.id }] : []));
    const out: Lane[] = [];
    parents.forEach((p, k) => {
      if (seen.has(p)) return;
      // The first parent carries the commit's own lane on; another joins the lane already waiting for it.
      const waiting = lanes.findIndex((l) => l?.sha === p);
      const lane = k === 0 ? { col, id } : waiting >= 0 ? { col: waiting, id: lanes[waiting]!.id } : { col: free(), id: next++ };
      lanes[lane.col] = { sha: p, id: lane.id };
      out.push(lane);
    });
    while (lanes.length && lanes.at(-1) === null) lanes.pop();
    return { col, id, through, into, out, width: Math.max(col, ...[...through, ...into, ...out].map((l) => l.col)) + 1 };
  });
}
