import { createRef, useState } from "react";
import type { Layout, LayoutChangedMeta, Orientation, PanelImperativeHandle } from "react-resizable-panels";
import { isRecord, readJson, writeJson } from "@/lib/storage";
import { initialSize, openSize, type PanelSizes, remembered, toPixels } from "./panelSizes";

/**
 * A group whose fixed panels (`bases`: their sizes until the user sets one) keep their pixels, as an
 * IDE's sidebars do: kept as shares of the group, they shrank with the window, and the window opens
 * at its default size each launch. Only what the user did is saved; a size a too-small window
 * squeezed comes back with the room.
 */
export function usePanelSizes(key: string, orientation: Orientation, bases: Record<string, number | string>) {
  const [api] = useState(() => {
    const ids = Object.keys(bases);
    const initial = readJson(key, {}, isRecord) as PanelSizes;
    let saved = initial;
    const refs = Object.fromEntries(ids.map((id) => [id, createRef<PanelImperativeHandle>()]));
    const element = createRef<HTMLDivElement>();
    const panel = (id: string) => refs[id]?.current ?? null;
    // Set while a call through here resizes: the library reports those as not the user's.
    let asked = false;
    const ask = (resize: () => void) => {
      asked = true;
      try {
        resize();
      } finally {
        asked = false;
      }
    };
    return {
      group: {
        orientation,
        elementRef: element,
        onLayoutChanged(layout: Layout, meta: LayoutChangedMeta) {
          const el = element.current;
          if (!el) return;
          // As the library measures it: its panels, without the dividers.
          let total = 0;
          for (const c of el.children) if (c instanceof HTMLElement && c.hasAttribute("data-panel")) total += orientation === "horizontal" ? c.offsetWidth : c.offsetHeight;
          if (meta.isUserInteraction || asked) {
            saved = remembered(saved, layout, total, ids);
            writeJson(key, saved);
            return;
          }
          // The window resized, or the group (re)mounted: back to the sizes left, as far as they fit.
          for (const id of ids) {
            const share = layout[id];
            const size = saved[id]?.size;
            if (share === undefined || size === undefined || saved[id]?.collapsed) continue;
            // After the library's update is through; a size that still doesn't fit changes nothing.
            if (Math.abs(toPixels(share, total) - size) > 1) queueMicrotask(() => panel(id)?.resize(size));
          }
        },
      },
      /** A fixed panel's props: its ref, the size it mounts at, and its pixels kept on a window resize. */
      panel: (id: string) => ({ panelRef: refs[id], defaultSize: initialSize(initial[id], bases[id]), groupResizeBehavior: "preserve-pixel-size" as const }),
      isCollapsed: (id: string) => panel(id)?.isCollapsed() ?? false,
      /** Opens it at the size it was left at; the library's own expand knows only this run's. */
      expand(id: string) {
        const p = panel(id);
        if (p?.isCollapsed()) ask(() => p.resize(openSize(saved[id], bases[id])));
      },
      collapse: (id: string) => ask(() => panel(id)?.collapse()),
      resize: (id: string, size: number | string) => ask(() => panel(id)?.resize(size)),
    };
  });
  return api;
}
