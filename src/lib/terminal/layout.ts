// A terminal tab's split panes as a tree. Pure, so it runs under `node --test`.

/** A pane's id, or a row (side by side) or column (stacked) of them; `sizes` are percents, one per child. */
export type Layout = number | Split;
export interface Split {
  dir: "row" | "col";
  children: Layout[];
  sizes: number[];
}

export type Direction = "left" | "right" | "up" | "down";

/** The panes in reading order: what ⌥⌘←/→ step through, and the order a tab lists them in. */
export const leaves = (l: Layout): number[] => (typeof l === "number" ? [l] : l.children.flatMap(leaves));

/** Panes side by side at equal widths: a tab saved before splits went both ways. */
export const row = (ids: number[]): Layout => (ids.length === 1 ? ids[0] : { dir: "row", children: ids, sizes: ids.map(() => 100 / ids.length) });

/** `pane` right after `at` along `dir`, halving its space. A split already going that way takes it as one more child. */
export function splitPane(l: Layout, at: number, pane: number, dir: Split["dir"]): Layout {
  if (l === at) return { dir, children: [at, pane], sizes: [50, 50] };
  if (typeof l === "number") return l;
  const i = l.children.indexOf(at);
  if (i >= 0 && l.dir === dir) {
    const half = l.sizes[i] / 2;
    return { dir, children: [...l.children.slice(0, i + 1), pane, ...l.children.slice(i + 1)], sizes: [...l.sizes.slice(0, i), half, half, ...l.sizes.slice(i + 1)] };
  }
  return { ...l, children: l.children.map((c) => splitPane(c, at, pane, dir)) };
}

/** Without `pane`; a split left with one child gives way to it. Null once nothing is left. */
export function removePane(l: Layout, pane: number): Layout | null {
  if (typeof l === "number") return l === pane ? null : l;
  const kept = l.children.map((c, i) => [removePane(c, pane), l.sizes[i]] as const).filter((x): x is readonly [Layout, number] => x[0] !== null);
  if (!kept.length) return null;
  if (kept.length === 1) return kept[0][0];
  // The closed pane's space goes to the rest, in proportion.
  const total = kept.reduce((n, [, s]) => n + s, 0);
  // A child split going the same way (one left after its own close) joins this one.
  const children: Layout[] = [];
  const sizes: number[] = [];
  for (const [c, s] of kept) {
    const share = (s / total) * 100;
    if (typeof c !== "number" && c.dir === l.dir) {
      children.push(...c.children);
      sizes.push(...c.sizes.map((x) => (x / 100) * share));
    } else {
      children.push(c);
      sizes.push(share);
    }
  }
  return { dir: l.dir, children, sizes };
}

/**
 * The split at `path` (child indexes from the top) with new sizes, as its divider was dragged. Only
 * the nodes on the way are copied: two dividers dragged at once report one after the other.
 */
export function resize(l: Layout, path: number[], sizes: number[]): Layout {
  if (typeof l === "number") return l;
  if (!path.length) return sizes.length === l.children.length ? { ...l, sizes } : l;
  const [at, ...rest] = path;
  return { ...l, children: l.children.map((c, i) => (i === at ? resize(c, rest, sizes) : c)) };
}

/**
 * The split at `path` with its children at equal sizes: a double-click on its divider. `deep`: every
 * split under it too, Equalize Terminal Panes on a whole tab.
 */
export function equalize(l: Layout, path: number[] = [], deep = false): Layout {
  if (typeof l === "number") return l;
  if (path.length) {
    const [at, ...rest] = path;
    return { ...l, children: l.children.map((c, i) => (i === at ? equalize(c, rest, deep) : c)) };
  }
  return { ...l, children: deep ? l.children.map((c) => equalize(c, [], true)) : l.children, sizes: l.children.map(() => 100 / l.children.length) };
}

/** A pane's box on screen (a DOMRect will do). */
export type Rect = { x: number; y: number; width: number; height: number };

/** Half a pixel: fractional layout's rounding, and panes meeting only at a divider's corner. */
const EDGE = 0.5;

/**
 * The pane next to `from` on that side, by where they're drawn rather than the tree: the nearest one
 * sharing an edge with it, then the one sharing most of it, then the first in reading order.
 */
export function neighbor(rects: Map<number, Rect>, from: number, dir: Direction): number | undefined {
  const r = rects.get(from);
  if (!r) return undefined;
  const across = dir === "left" || dir === "right";
  const sign = dir === "left" || dir === "up" ? -1 : 1;
  const mid = (b: Rect) => (across ? b.x + b.width / 2 : b.y + b.height / 2);
  let best: { id: number; gap: number; overlap: number } | undefined;
  for (const [id, c] of rects) {
    const gap = { left: r.x - (c.x + c.width), right: c.x - (r.x + r.width), up: r.y - (c.y + c.height), down: c.y - (r.y + r.height) }[dir];
    const overlap = across ? Math.min(r.y + r.height, c.y + c.height) - Math.max(r.y, c.y) : Math.min(r.x + r.width, c.x + c.width) - Math.max(r.x, c.x);
    // Its middle past this one's: however narrow the panes, never one on the other side.
    if (id === from || (mid(c) - mid(r)) * sign <= EDGE || gap < -EDGE || overlap <= EDGE) continue;
    if (!best || gap < best.gap - EDGE || (gap <= best.gap + EDGE && overlap > best.overlap + EDGE)) best = { id, gap, overlap };
  }
  return best?.id;
}

/** Deeper than any grid a screen fits: a save nested past it was made by hand. */
const MAX_DEPTH = 16;

/**
 * A saved layout, whose leaves index the saved panes, if it holds each of the `count` once and in
 * order (panes are saved in reading order), its sizes rescaled to 100; else (none saved, or a
 * hand-edited one) a row of them all.
 */
export function savedLayout(raw: unknown, count: number): Layout {
  let next = 0;
  const read = (x: unknown, depth: number): Layout | null => {
    if (typeof x === "number") return x === next ? next++ : null;
    if (depth >= MAX_DEPTH || !x || typeof x !== "object") return null;
    const { dir, children, sizes } = x as Partial<Split>;
    if ((dir !== "row" && dir !== "col") || !Array.isArray(children) || !Array.isArray(sizes) || children.length < 2 || sizes.length !== children.length) return null;
    if (!sizes.every((n) => Number.isFinite(n) && n > 0)) return null;
    const kids: Layout[] = [];
    for (const c of children) {
      const l = read(c, depth + 1);
      if (l === null) return null;
      kids.push(l);
    }
    const total = sizes.reduce((a, b) => a + b, 0);
    return { dir, children: kids, sizes: sizes.map((n) => (n * 100) / total) };
  };
  const l = read(raw, 0);
  return l !== null && next === count ? l : row([...Array(count).keys()]);
}

/** The same tree with each pane's id mapped, as a save turns ids into indexes and a restore back. */
export const mapPanes = (l: Layout, fn: (id: number) => number): Layout => (typeof l === "number" ? fn(l) : { ...l, children: l.children.map((c) => mapPanes(c, fn)) });
