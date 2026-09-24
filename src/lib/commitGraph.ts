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
 * `head`, when listed, is held the first lane and branch 0 even below newer branches' tips.
 */
export function graphRows(commits: readonly { sha: string; parents: readonly string[] }[], head?: string): GraphRow[] {
  // The commit each lane waits for; null is a free column. A held lane draws nothing until
  // something leads into it.
  const lanes: ({ sha: string; id: number; held?: boolean } | null)[] = head ? [{ sha: head, id: 0, held: true }] : [];
  const seen = new Set<string>();
  let next = head ? 1 : 0;
  return commits.map(({ sha, parents }) => {
    seen.add(sha);
    const waiting = lanes.flatMap((l, col) => (l?.sha === sha ? [{ col, id: l.id, held: l.held }] : []));
    const free = () => (lanes.includes(null) ? lanes.indexOf(null) : lanes.length);
    const col = waiting[0]?.col ?? free();
    const id = waiting[0]?.id ?? next++;
    for (const l of waiting) lanes[l.col] = null;
    const into = waiting.flatMap((l) => (l.held ? [] : [{ col: l.col, id: l.id }]));
    const through = lanes.flatMap((l, c) => (l && !l.held ? [{ col: c, id: l.id }] : []));
    const out: Lane[] = [];
    parents.forEach((p, k) => {
      if (seen.has(p)) return;
      // The first parent carries the commit's own lane on; another joins the lane already waiting for it.
      const at = lanes.findIndex((l) => l?.sha === p);
      const lane = k === 0 ? { col, id } : at >= 0 ? { col: at, id: lanes[at]!.id } : { col: free(), id: next++ };
      lanes[lane.col] = { sha: p, id: lane.id };
      out.push(lane);
    });
    while (lanes.length && lanes.at(-1) === null) lanes.pop();
    return { col, id, through, into, out, width: Math.max(col, ...[...through, ...into, ...out].map((l) => l.col)) + 1 };
  });
}
