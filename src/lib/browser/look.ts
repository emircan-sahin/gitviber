// How a tab shows its page outside device mode (BrowserView): its zoom, and light or dark.

/** Page zoom's steps, as Safari's. */
export const ZOOMS = [0.5, 0.75, 0.85, 1, 1.15, 1.25, 1.5, 1.75, 2, 2.5, 3];

export const isZoom = (z: unknown): z is number => typeof z === "number" && ZOOMS.includes(z);

/** The step past `zoom` the way `by` says (one in, one out), or 100% for 0; at the ends it stays. */
export function stepZoom(zoom: number, by: -1 | 0 | 1): number {
  if (by === 0) return 1;
  const next = by > 0 ? ZOOMS.find((z) => z > zoom + 1e-9) : [...ZOOMS].reverse().find((z) => z < zoom - 1e-9);
  return next ?? zoom;
}

export const zoomLabel = (zoom: number) => `${Math.round(zoom * 100)}%`;

/** A page shown light or dark, whatever the app is (`prefers-color-scheme` follows); none: as the app. */
export type Scheme = "light" | "dark";

export const isScheme = (s: unknown): s is Scheme => s === "light" || s === "dark";

/** The toggle's next: as the app, light, dark, round again. */
export const nextScheme = (s: Scheme | undefined): Scheme | undefined => (s === undefined ? "light" : s === "light" ? "dark" : undefined);
