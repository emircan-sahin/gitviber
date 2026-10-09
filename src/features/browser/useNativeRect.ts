import { type RefObject, useEffect, useState } from "react";
import { browserApi, type NativeScreen } from "@/lib/api";
import type { Fitted } from "@/lib/browser/fit";
import { pageHasFocus } from "@/lib/browser/store";
import { useSettings } from "@/lib/settings";
import { useTerminalsMaximized } from "@/lib/terminal/terminals";
import { coveredBy, watchOverlays } from "@/lib/ui/overlays";

/** Points to the half, as AppKit lays views out on a 2x screen. */
const half = (v: number) => Math.round(v * 2) / 2;

/** A device's screen as fit.ts lays it out, and the pixel ratio the page should see. */
export type PageScreen = Pick<Fitted, "viewport" | "radius" | "corners"> & { dpr: number | null };

/**
 * Keeps tab `id`'s native view over `area` while `live`, as `screen` in device mode. This page
 * can't draw over a native view, so while a menu, dialog or toast reaches into it, the view steps
 * aside and the picture it returns (the hook's value) stands in.
 */
export function useNativeRect(area: RefObject<HTMLElement | null>, id: string, live: boolean, screen: PageScreen | null): string | null {
  // The interface scale is the app page's zoom: its CSS pixels are that many points.
  const scale = useSettings().uiScale;
  const maximized = useTerminalsMaximized();
  const [cover, setCover] = useState<string | null>(null);
  const native: NativeScreen | null = screen && { width: screen.viewport.w, height: screen.viewport.h, radius: screen.radius * scale, corners: screen.corners, dpr: screen.dpr };
  // The effect runs again when the screen changes, not on each render's new object.
  const shape = native && [native.width, native.height, native.radius, native.dpr, ...Object.values(native.corners)].join(" ");

  useEffect(() => {
    const el = area.current;
    if (!live || maximized || !el) {
      void browserApi.hide(id, true).catch(() => {});
      return;
    }
    let frame = 0;
    // The rect last placed at, "" while hidden.
    let placed = "";
    let aside = false;
    // The page had the keys when it stepped aside for a toast or a menu: they go back to it after.
    let refocus = false;
    let gone = false;
    const hide = () => {
      placed = "";
      void browserApi.hide(id, true).catch(() => {});
    };
    const measure = () => {
      frame = 0;
      const r = el.getBoundingClientRect();
      if (r.width < 2 || r.height < 2) {
        if (placed) hide();
        return;
      }
      const over = coveredBy(r);
      if (over) {
        if (aside) return;
        aside = true;
        refocus = over === "overlay" && pageHasFocus(id);
        // A dialog (⌘P's field) gets the keys right away, not once the view is hidden. Typed
        // under a toast they land on the area, which keeps them from the app's one-key commands.
        if (over === "dialog") void browserApi.focus(id, false).catch(() => {});
        else if (refocus) el.focus();
        // The picture first: hidden, the view would have none to give.
        void browserApi
          .snapshot(id)
          .catch(() => null)
          .then((shot) => {
            if (gone || !aside) return;
            setCover(shot);
            hide();
          });
        return;
      }
      // Back from aside: placed again even where it was (the overlay may have left before the
      // view did), and the keys go back.
      const back = aside;
      aside = false;
      const rect = { x: half(r.left * scale), y: half(r.top * scale), w: half(r.width * scale), h: half(r.height * scale) };
      const key = `${rect.x} ${rect.y} ${rect.w} ${rect.h}`;
      if (key === placed && !back) return;
      placed = key;
      browserApi.place(id, rect, native).then(
        () => {
          if (gone) return;
          setCover(null);
          if (!back || !refocus) return;
          refocus = false;
          if (document.activeElement !== el) return;
          el.blur();
          void browserApi.focus(id, true).catch(() => {});
        },
        () => (placed = ""),
      );
    };
    const schedule = () => {
      frame ||= requestAnimationFrame(measure);
    };
    const resize = new ResizeObserver(schedule);
    resize.observe(el);
    window.addEventListener("resize", schedule);
    const unwatch = watchOverlays(schedule);
    schedule();
    return () => {
      gone = true;
      cancelAnimationFrame(frame);
      resize.disconnect();
      window.removeEventListener("resize", schedule);
      unwatch();
    };
  }, [area, id, live, scale, maximized, shape]);

  return live ? cover : null;
}
