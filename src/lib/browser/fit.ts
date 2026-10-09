import type { Device, Insets } from "./devices.ts";

// Where a device's body, screen and page go in the area a browser tab has: shrunk to fit,
// never enlarged, centered, and turned on its side when rotated. As in a phone's browser, the
// page gets the screen less its status bar (and its home indicator), which this page draws.

export interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Corners {
  topLeft: boolean;
  topRight: boolean;
  bottomRight: boolean;
  bottomLeft: boolean;
}

export interface Fitted {
  /** On-screen CSS px per device CSS px. */
  scale: number;
  /** Big enough to show; smaller, a device would be a smudge. */
  fits: boolean;
  /** The screen, in device CSS px, as turned. */
  size: { w: number; h: number };
  /** The page's viewport, in device CSS px: the screen less its bars. */
  viewport: { w: number; h: number };
  /** In the area's CSS px. */
  frame: Box;
  screen: Box;
  page: Box;
  /** The screen's corner radius, in the area's CSS px. */
  radius: number;
  /** The screen's rounded corners the page reaches: the page's view rounds those itself. */
  corners: Corners;
  /** The status bar (top, or left when turned) and home indicator, in the area's CSS px. */
  bars: { top: number; left: number; bottom: number };
  /** In the area's CSS px, from the screen's top left. */
  cutout: (Box & { r: number }) | null;
  /** In device CSS px, as turned. */
  safe: Insets;
}

/** Below this a device is too small to read, and the tab says so instead. */
const SMALLEST = 0.15;

/** Turned a quarter to the left, as a phone held in landscape: its top (the camera) goes left. */
function turn(device: Device): { w: number; h: number; cutout: Device["cutout"]; safe: Insets } {
  const { w, h, cutout, safe } = device;
  return {
    w: h,
    h: w,
    cutout: cutout && { ...cutout, x: cutout.y, y: w - cutout.x - cutout.w, w: cutout.h, h: cutout.w },
    safe: { top: safe.right, right: safe.bottom, bottom: safe.left, left: safe.top },
  };
}

export function fit(device: Device, rotated: boolean, area: { w: number; h: number }): Fitted {
  const { w, h, cutout, safe } = rotated ? turn(device) : device;
  const b = device.bezel;
  const scale = Math.max(0, Math.min(1, area.w / (w + 2 * b), area.h / (h + 2 * b)));
  const frame = { w: (w + 2 * b) * scale, h: (h + 2 * b) * scale, x: 0, y: 0 };
  frame.x = (area.w - frame.w) / 2;
  frame.y = (area.h - frame.h) / 2;
  const screen = { x: frame.x + b * scale, y: frame.y + b * scale, w: w * scale, h: h * scale };
  // Upright, the status bar and the home indicator; turned, the camera's side. A phone's browser
  // in landscape hides its status bar.
  const bars = rotated ? { top: 0, left: safe.left, bottom: 0 } : { top: safe.top, left: 0, bottom: safe.bottom };
  const viewport = { w: w - bars.left, h: h - bars.top - bars.bottom };
  return {
    scale,
    fits: scale >= SMALLEST,
    size: { w, h },
    viewport,
    frame,
    screen,
    page: { x: screen.x + bars.left * scale, y: screen.y + bars.top * scale, w: viewport.w * scale, h: viewport.h * scale },
    radius: device.radius * scale,
    corners: { topLeft: !bars.top && !bars.left, topRight: !bars.top, bottomRight: !bars.bottom, bottomLeft: !bars.bottom && !bars.left },
    bars: { top: bars.top * scale, left: bars.left * scale, bottom: bars.bottom * scale },
    cutout: cutout && { x: cutout.x * scale, y: cutout.y * scale, w: cutout.w * scale, h: cutout.h * scale, r: cutout.r * scale },
    safe,
  };
}
