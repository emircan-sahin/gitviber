import { useSyncExternalStore } from "react";

export type Platform = "mac" | "linux" | "other";

const detect = (): Platform => {
  const ua = navigator.userAgent;
  if (/Android|iPhone|iPod/.test(ua)) return "other";
  if (/Macintosh|Mac OS X/.test(ua)) return "mac";
  if (/Linux|X11/.test(ua)) return "linux";
  return "other";
};

const noop = () => () => {};

/** The visitor's OS for picking a download. The prerendered page (and first paint) says macOS. */
export const usePlatform = () => useSyncExternalStore(noop, detect, () => "mac" as Platform);
