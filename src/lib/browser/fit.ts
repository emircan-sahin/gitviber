import type { Device, Insets } from "./devices.ts";

// Where a device's body and screen go in the area a browser tab has: shrunk to fit, never
// enlarged, centered, and turned on its side when rotated.

export interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Fitted {
  /** On-screen CSS px per device CSS px. */
  scale: number;
  /** The page's viewport, in device CSS px. */
  viewport: { w: number; h: number };
  /** In the area's CSS px. */
  frame: Box;
  screen: Box;
  /** The screen's corners and cutout, in the area's CSS px; the cutout from the screen's top left. */
  radius: number;
  cutout: (Box & { r: number }) | null;
  /** In device CSS px, as turned. */
  safe: Insets;
}

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
  const scale = Math.max(0.05, Math.min(1, area.w / (w + 2 * b), area.h / (h + 2 * b)));
  const frame = { w: (w + 2 * b) * scale, h: (h + 2 * b) * scale, x: 0, y: 0 };
  frame.x = (area.w - frame.w) / 2;
  frame.y = (area.h - frame.h) / 2;
  const screen = { x: frame.x + b * scale, y: frame.y + b * scale, w: w * scale, h: h * scale };
  return {
    scale,
    viewport: { w, h },
    frame,
    screen,
    radius: device.radius * scale,
    cutout: cutout && { x: cutout.x * scale, y: cutout.y * scale, w: cutout.w * scale, h: cutout.h * scale, r: cutout.r * scale },
    safe,
  };
}
