import data from "../../../src-tauri/src/browser/devices.json" with { type: "json" };

// The devices a browser tab can show its page as (device mode), from devices.json.

/** The camera's island, notch or hole, in CSS px from the screen's top left, portrait. */
export interface Cutout {
  type: "island" | "notch" | "punch";
  x: number;
  y: number;
  w: number;
  h: number;
  r: number;
}

export interface Insets {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

export interface Device {
  name: string;
  /** The screen in CSS px, portrait. */
  w: number;
  h: number;
  /** 0: this Mac's own, as Responsive. */
  dpr: number;
  /** "": WebKit's own, as Responsive. */
  ua: string;
  /** The screen's corners, in CSS px. */
  radius: number;
  safe: Insets;
  cutout: Cutout | null;
  /** The body's edge around the screen, in CSS px. */
  bezel: number;
  platform: "ios" | "ipados" | "android" | "web";
}

export const DEVICES = data.devices as Device[];

export const RESPONSIVE = "Responsive";

/** A tab's device, saved with it: one of DEVICES by name, or Responsive at a size of its own. */
export interface DeviceChoice {
  name: string;
  rotated?: boolean;
  /** Responsive's size, in CSS px. */
  w?: number;
  h?: number;
}

/** Device mode's first device, until another is picked. */
export const DEFAULT_DEVICE: DeviceChoice = { name: "iPhone 16 Pro" };

const RESPONSIVE_SIZE = { w: 400, h: 800 };
/** Responsive's smallest and largest sides, in CSS px. */
export const RESPONSIVE_MIN = 200;
export const RESPONSIVE_MAX = 3000;

const NONE: Insets = { top: 0, right: 0, bottom: 0, left: 0 };

/** The device a choice names; Responsive is a bare screen of its size. Null for a name no longer known. */
export function deviceOf(choice: DeviceChoice): Device | null {
  if (choice.name !== RESPONSIVE) return DEVICES.find((d) => d.name === choice.name) ?? null;
  const side = (v: number | undefined, fallback: number) => Math.round(Math.min(RESPONSIVE_MAX, Math.max(RESPONSIVE_MIN, v ?? fallback)));
  return { name: RESPONSIVE, w: side(choice.w, RESPONSIVE_SIZE.w), h: side(choice.h, RESPONSIVE_SIZE.h), dpr: 0, ua: "", radius: 0, safe: NONE, cutout: null, bezel: 0, platform: "web" };
}

const side = (v: unknown) => v === undefined || (typeof v === "number" && Number.isFinite(v) && v > 0);

/** What a stored tab may hold as its device. */
export const isDeviceChoice = (v: unknown): v is DeviceChoice =>
  typeof v === "object" &&
  v !== null &&
  typeof (v as DeviceChoice).name === "string" &&
  ((v as DeviceChoice).rotated === undefined || typeof (v as DeviceChoice).rotated === "boolean") &&
  side((v as DeviceChoice).w) &&
  side((v as DeviceChoice).h);
