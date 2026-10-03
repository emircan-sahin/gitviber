// JSON Canvas (jsoncanvas.org, Obsidian's .canvas files): parsed defensively, and the geometry
// to draw it. No JSX or app imports, so node tests run it.

import { COLORS, type Rgb } from "./colors.ts";

export type Side = "top" | "right" | "bottom" | "left";

export interface CanvasNode {
  id: string;
  type: "text" | "file" | "link" | "group";
  x: number;
  y: number;
  width: number;
  height: number;
  color?: string;
  text?: string;
  file?: string;
  /** A file node's #heading or #^block. */
  subpath?: string;
  url?: string;
  label?: string;
}

export interface CanvasEdge {
  id: string;
  fromNode: string;
  toNode: string;
  fromSide?: Side;
  toSide?: Side;
  fromEnd: "none" | "arrow";
  toEnd: "none" | "arrow";
  color?: string;
  label?: string;
}

export interface Canvas {
  nodes: CanvasNode[];
  edges: CanvasEdge[];
}

const SIDES: Side[] = ["top", "right", "bottom", "left"];
const TYPES = ["text", "file", "link", "group"];
const str = (v: unknown) => (typeof v === "string" ? v : undefined);
const num = (v: unknown) => (typeof v === "number" && Number.isFinite(v) ? v : undefined);
const side = (v: unknown) => (SIDES.includes(v as Side) ? (v as Side) : undefined);

/** The canvas in `text`, leaving out what the spec doesn't allow; throws when it isn't JSON. */
export function parseCanvas(text: string): Canvas {
  const data: unknown = text.trim() ? JSON.parse(text) : {};
  const obj = (v: unknown) => (v && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, unknown>) : null);
  const list = (v: unknown) => (Array.isArray(v) ? v.map(obj).filter((o) => !!o) : []);
  const seen = new Set<string>();
  const nodes = list(obj(data)?.nodes).flatMap((n): CanvasNode[] => {
    const [id, type, x, y, width, height] = [str(n.id), str(n.type), num(n.x), num(n.y), num(n.width), num(n.height)];
    // Ids are unique (a second one with the same id is left out), sizes positive.
    if (!id || seen.has(id) || !type || !TYPES.includes(type) || x === undefined || y === undefined || !width || width < 0 || !height || height < 0) return [];
    seen.add(id);
    return [{ id, type: type as CanvasNode["type"], x, y, width, height, color: str(n.color), text: str(n.text), file: str(n.file), subpath: str(n.subpath), url: str(n.url), label: str(n.label) }];
  });
  const ids = new Set(nodes.map((n) => n.id));
  const edges = list(obj(data)?.edges).flatMap((e): CanvasEdge[] => {
    const [id, fromNode, toNode] = [str(e.id), str(e.fromNode), str(e.toNode)];
    if (!id || !fromNode || !toNode || !ids.has(fromNode) || !ids.has(toNode)) return [];
    return [
      {
        id,
        fromNode,
        toNode,
        fromSide: side(e.fromSide),
        toSide: side(e.toSide),
        fromEnd: e.fromEnd === "arrow" ? "arrow" : "none",
        toEnd: e.toEnd === "none" ? "none" : "arrow",
        color: str(e.color),
        label: str(e.label),
      },
    ];
  });
  // Groups first: later nodes draw over earlier ones, and a group sits behind what's in it.
  return { nodes: [...nodes.filter((n) => n.type === "group"), ...nodes.filter((n) => n.type !== "group")], edges };
}

export interface Box {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** The box around every node; a 0×0 box for none. */
export function bounds(nodes: Box[]): Box {
  if (!nodes.length) return { x: 0, y: 0, width: 0, height: 0 };
  // A loop, not Math.min(...nodes): spreading 100k+ arguments overflows the stack.
  let [x, y, right, bottom] = [Infinity, Infinity, -Infinity, -Infinity];
  for (const n of nodes) {
    x = Math.min(x, n.x);
    y = Math.min(y, n.y);
    right = Math.max(right, n.x + n.width);
    bottom = Math.max(bottom, n.y + n.height);
  }
  return { x, y, width: right - x, height: bottom - y };
}

const median = (xs: number[]) => {
  const sorted = [...xs].sort((a, b) => a - b);
  return sorted[Math.floor((sorted.length - 1) / 2)];
};

/**
 * The box a canvas is fitted to: around every card, unless a few lie far off from the rest (a
 * card dragged to x = 10,000,000), which would shrink the rest to dots. Those are left out: the
 * cards more than four times as far from the middle as three in four of them are, past a card's
 * size. They stay a pan away.
 */
export function fitBox(nodes: Box[]): Box {
  const all = bounds(nodes);
  if (nodes.length < 3) return all;
  const centers = nodes.map((n) => [n.x + n.width / 2, n.y + n.height / 2]);
  const [mx, my] = [median(centers.map((c) => c[0])), median(centers.map((c) => c[1]))];
  const dist = centers.map(([x, y]) => Math.hypot(x - mx, y - my));
  const sorted = [...dist].sort((a, b) => a - b);
  const reach = 4 * sorted[Math.floor((sorted.length - 1) * 0.75)] + Math.max(...nodes.slice(0, 1000).map((n) => Math.hypot(n.width, n.height)));
  const near = nodes.filter((_, i) => dist[i] <= reach);
  return near.length ? bounds(near) : all;
}

/** The middle of a node's side, where an edge meets it. */
export function anchor(n: Box, s: Side): [number, number] {
  if (s === "top") return [n.x + n.width / 2, n.y];
  if (s === "bottom") return [n.x + n.width / 2, n.y + n.height];
  if (s === "left") return [n.x, n.y + n.height / 2];
  return [n.x + n.width, n.y + n.height / 2];
}

const NORMAL: Record<Side, [number, number]> = { top: [0, -1], right: [1, 0], bottom: [0, 1], left: [-1, 0] };

/** The side of `from` that faces `to` best, for an edge that names none. */
export function facing(from: Box, to: Box): Side {
  const dx = to.x + to.width / 2 - (from.x + from.width / 2);
  const dy = to.y + to.height / 2 - (from.y + from.height / 2);
  return Math.abs(dx) > Math.abs(dy) ? (dx > 0 ? "right" : "left") : dy > 0 ? "bottom" : "top";
}

/**
 * An edge as a curve leaving each node straight out of its side, as Obsidian draws them: the
 * SVG path, its midpoint (for the label) and the direction it arrives in at each end (for arrows).
 */
export function edgePath(from: Box, to: Box, fromSide = facing(from, to), toSide = facing(to, from)) {
  const [x1, y1] = anchor(from, fromSide);
  const [x2, y2] = anchor(to, toSide);
  const reach = Math.max(40, Math.min(240, Math.hypot(x2 - x1, y2 - y1) / 2));
  const [c1x, c1y] = [x1 + NORMAL[fromSide][0] * reach, y1 + NORMAL[fromSide][1] * reach];
  const [c2x, c2y] = [x2 + NORMAL[toSide][0] * reach, y2 + NORMAL[toSide][1] * reach];
  // The cubic's point at t = ½.
  const mid: [number, number] = [(x1 + 3 * c1x + 3 * c2x + x2) / 8, (y1 + 3 * c1y + 3 * c2y + y2) / 8];
  return {
    d: `M ${x1} ${y1} C ${c1x} ${c1y}, ${c2x} ${c2y}, ${x2} ${y2}`,
    mid,
    start: { at: [x1, y1] as [number, number], angle: Math.atan2(-NORMAL[fromSide][1], -NORMAL[fromSide][0]) },
    end: { at: [x2, y2] as [number, number], angle: Math.atan2(-NORMAL[toSide][1], -NORMAL[toSide][0]) },
  };
}

/** Obsidian's six canvas colors, "1" to "6"; any other value is a CSS color as written. */
const PRESETS: Record<string, Rgb> = { "1": COLORS.red, "2": COLORS.orange, "3": COLORS.yellow, "4": COLORS.green, "5": COLORS.cyan, "6": COLORS.purple };

/** A node's or edge's color as CSS, with `alpha`; null for none (the theme's). */
export function canvasColor(color: string | undefined, dark: boolean, alpha = 1): string | null {
  if (!color) return null;
  // A color named like a built-in ("constructor") is no preset.
  const preset = Object.hasOwn(PRESETS, color) ? PRESETS[color] : undefined;
  if (preset) return `rgb(${preset[dark ? 0 : 1]} / ${alpha})`;
  // Only a hex color: the value goes into a style attribute.
  return /^#[0-9a-f]{3,8}$/i.test(color) ? (alpha === 1 ? color : `color-mix(in srgb, ${color} ${Math.round(alpha * 100)}%, transparent)`) : null;
}
