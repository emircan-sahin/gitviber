/**
 * The size an SVG draws at on its own, given the natural size the browser reported for it.
 * Without an absolute width and height the browser invents one (Chrome: 150×150 for a square
 * icon), so the viewBox is the truer size then.
 */
export function svgSize(text: string, natural: [number, number]): [number, number] {
  const root = /<svg\b[^>]*>/i.exec(text)?.[0] ?? "";
  const attr = (name: string) => new RegExp(`\\s${name}\\s*=\\s*["']\\s*([^"']*?)\\s*["']`, "i").exec(root)?.[1];
  const sized = [attr("width"), attr("height")].every((v) => v && !v.endsWith("%"));
  const box = attr("viewBox")?.split(/[\s,]+/).map(Number);
  if (!sized && box?.length === 4 && box[2] > 0 && box[3] > 0) return [box[2], box[3]];
  return natural[0] > 0 && natural[1] > 0 ? natural : [300, 150];
}

/** The longest side zooming may draw: a webview rasterizing far past this stalls the whole app. */
const MAX_DRAWN = 16384;

/** How far an SVG of `natural` size, which fits its panel at `fit`, may zoom out and in. */
export function zoomLimits(natural: [number, number], fit: number): [number, number] {
  const min = Math.min(fit, 1) / 2;
  return [min, Math.max(min, Math.min(Math.max(fit, 1) * 16, MAX_DRAWN / Math.max(...natural)))];
}

/**
 * How a preview is zoomed. `scale` is display pixels per SVG pixel, null to fit the panel;
 * (u, v) is the point of the image, as fractions of its size, held at the panel's center.
 */
export interface Zoom {
  scale: number | null;
  u: number;
  v: number;
}

export const FIT: Zoom = { scale: null, u: 0.5, v: 0.5 };

/** Where along a `room`-long panel an image `size` long starts: centered if it fits, else never leaving a gap. */
export function place(room: number, size: number, u: number) {
  return size <= room ? (room - size) / 2 : Math.min(0, Math.max(room - size, room / 2 - u * size));
}

/** `u` limited to what place() can show, so panning past an edge doesn't build up. */
function clampCenter(room: number, size: number, u: number) {
  if (size <= room) return 0.5;
  const half = room / 2 / size;
  return Math.min(1 - half, Math.max(half, u));
}

/** Along one axis: the new center when zooming from `size` to `next`, keeping the point under `at` in place. */
export function zoomAxis(room: number, size: number, next: number, u: number, at: number) {
  const under = (at - place(room, size, u)) / size;
  return clampCenter(room, next, (room / 2 - at) / next + under);
}

/** Along one axis: the new center after dragging the image by `delta` pixels. */
export function panAxis(room: number, size: number, u: number, delta: number) {
  return clampCenter(room, size, u - delta / size);
}
