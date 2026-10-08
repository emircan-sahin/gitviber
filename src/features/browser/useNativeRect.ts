import { type RefObject, useEffect, useState } from "react";
import { browserApi } from "@/lib/api";
import { useSettings } from "@/lib/settings";
import { useTerminalsMaximized } from "@/lib/terminal/terminals";
import { covered, watchOverlays } from "./overlays";

/** Points to the half, as AppKit lays views out on a 2x screen. */
const half = (v: number) => Math.round(v * 2) / 2;

/**
 * Keeps tab `id`'s native view over `area` while `live`: placed as the area moves or resizes,
 * hidden while it's too small or the maximized terminal covers it. While this page draws over it
 * (a menu, a dialog, a toast) the view steps aside, and the picture it returns stands in.
 */
export function useNativeRect(area: RefObject<HTMLElement | null>, id: string, live: boolean): string | null {
  // The interface scale is the app page's zoom: its CSS pixels are that many points.
  const scale = useSettings().uiScale;
  const maximized = useTerminalsMaximized();
  const [cover, setCover] = useState<string | null>(null);

  useEffect(() => {
    const el = area.current;
    if (!live || maximized || !el) {
      void browserApi.hide(id).catch(() => {});
      return;
    }
    let frame = 0;
    // The rect last placed at, "" while hidden.
    let placed = "";
    let aside = false;
    let gone = false;
    const hide = () => {
      placed = "";
      void browserApi.hide(id).catch(() => {});
    };
    const measure = () => {
      frame = 0;
      const r = el.getBoundingClientRect();
      if (r.width < 2 || r.height < 2) {
        if (placed) hide();
        return;
      }
      if (covered(r)) {
        if (aside) return;
        aside = true;
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
      aside = false;
      const rect = { x: half(r.left * scale), y: half(r.top * scale), w: half(r.width * scale), h: half(r.height * scale) };
      const key = `${rect.x} ${rect.y} ${rect.w} ${rect.h}`;
      if (key === placed) return;
      placed = key;
      browserApi.place(id, rect).then(
        () => !gone && setCover(null),
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
  }, [area, id, live, scale, maximized]);

  return live ? cover : null;
}
