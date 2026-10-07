import { createRef, useState } from "react";
import type { GroupImperativeHandle, Layout, LayoutChangedMeta, Orientation, PanelImperativeHandle } from "react-resizable-panels";
import { isRecord, readJson, writeJson } from "@/lib/storage";
import { initialSize, type Limits, openSize, type PanelSizes, remembered, settle, toPixels } from "./panelSizes";

export interface FixedPanel {
  /** Its size until the user sets one: pixels, or a percentage as a string. */
  base: number | string;
  min: number;
  /** A percentage, as a string. */
  max?: string;
  collapsible?: boolean;
}

/** Pixels from a size as the library takes it: a number is pixels, a string a percentage. */
const pixels = (size: number | string, total: number) => (typeof size === "number" ? size : (parseFloat(size) / 100) * total);

/**
 * A group whose fixed panels keep their pixels, as an IDE's sidebars do; its one other panel (at
 * least `flexMin` long) takes the difference. Only what the user resized is saved, and a size a
 * too-small window squeezed comes back with the room. `legacy`: keys of the layout older builds
 * kept as shares of the window (shrinking with it), read once.
 */
export function usePanelSizes(key: string, orientation: Orientation, fixed: Record<string, FixedPanel>, { flexMin, legacy = [] }: { flexMin: number; legacy?: string[] }) {
  const [api] = useState(() => {
    const ids = Object.keys(fixed);
    const initial = readSizes(key, legacy, ids, orientation);
    let saved = initial;
    const refs = Object.fromEntries(ids.map((id) => [id, createRef<PanelImperativeHandle>()]));
    const element = createRef<HTMLDivElement>();
    const groupRef = createRef<GroupImperativeHandle>();
    const panel = (id: string) => refs[id]?.current ?? null;
    const size = (id: string) => openSize(saved[id], fixed[id].base);
    // The layout last reported, to tell which panels a drag moved.
    let last: Layout = {};
    // The panel a call through here resizes (the library reports those as not the user's), or
    // true while a restore applies.
    let asking: string | true | null = null;
    let restoring = false;
    // The group's length at the last layout: a window resize changes it, a drag doesn't.
    let seen = 0;

    const measure = () => {
      let total = 0;
      // As the library measures it: its panels, without the dividers.
      for (const c of element.current?.children ?? []) if (c instanceof HTMLElement && c.hasAttribute("data-panel")) total += orientation === "horizontal" ? c.offsetWidth : c.offsetHeight;
      return total;
    };
    const save = (layout: Layout, total: number, which: string[]) => {
      if (!which.length) return;
      saved = remembered(saved, layout, total, which);
      writeJson(key, saved);
    };
    // Back to the sizes the user left, as far as the room allows: one layout, applied once.
    const restore = () => {
      restoring = false;
      const layout = groupRef.current?.getLayout() ?? {};
      const total = (seen = measure());
      const flex = Object.keys(layout).find((id) => !(id in fixed));
      if (!total || !flex) return;
      const shown = ids.filter((id) => id in layout);
      const px = (id: string) => toPixels(layout[id], total);
      const current = Object.fromEntries(shown.map((id) => [id, px(id)]));
      const want = Object.fromEntries(shown.map((id) => [id, saved[id]?.collapsed === true ? 0 : pixels(size(id), total)]));
      const limits = Object.fromEntries(shown.map((id): [string, Limits] => [id, { min: fixed[id].min, max: pixels(fixed[id].max ?? total, total), collapsible: fixed[id].collapsible }]));
      const next = settle(current, want, limits, px(flex) - flexMin);
      if (shown.every((id) => Math.round(next[id]) === current[id])) return;
      const target: Layout = { ...layout };
      for (const id of shown) target[id] = (next[id] / total) * 100;
      target[flex] = 100 - shown.reduce((sum, id) => sum + target[id], 0);
      asking = true;
      try {
        groupRef.current?.setLayout(target);
      } finally {
        asking = null;
      }
    };
    // After the library's update is through.
    const scheduleRestore = () => {
      if (!restoring) queueMicrotask(restore);
      restoring = true;
    };
    const ask = (id: string, resize: (p: PanelImperativeHandle) => void) => {
      const p = panel(id);
      if (!p) return;
      asking = id;
      try {
        resize(p);
      } finally {
        asking = null;
      }
    };

    return {
      group: {
        orientation,
        elementRef: element,
        groupRef,
        onLayoutChanged(layout: Layout, meta: LayoutChangedMeta) {
          const before = last;
          last = layout;
          const total = (seen = measure());
          if (!total || asking === true) return;
          if (asking) return save(layout, total, [asking]);
          const moved = (id: string) => id in layout && (!(id in before) || toPixels(layout[id], total) !== toPixels(before[id], total));
          if (meta.isUserInteraction) return save(layout, total, ids.filter(moved));
          // The window resized, or the group (re)mounted.
          scheduleRestore();
        },
      },
      /**
       * The other panel's props. Its resize tells of a window resize that left the layout as it
       * was (both sidebars closed), which reports nothing.
       */
      flex: {
        minSize: flexMin,
        onResize: () => {
          if (measure() !== seen) scheduleRestore();
        },
      },
      /** A fixed panel's props: its ref, limits, the size it mounts at, and its pixels kept on a window resize. */
      panel: (id: string) => {
        const { base, min, max, collapsible } = fixed[id];
        return {
          panelRef: refs[id],
          defaultSize: initialSize(initial[id], base),
          minSize: min,
          maxSize: max,
          collapsible,
          collapsedSize: collapsible ? 0 : undefined,
          groupResizeBehavior: "preserve-pixel-size" as const,
        };
      },
      isCollapsed: (id: string) => panel(id)?.isCollapsed() ?? false,
      /** Opens it at the size it was left at; the library's own expand knows only this run's. */
      expand: (id: string) => ask(id, (p) => p.isCollapsed() && p.resize(size(id))),
      collapse: (id: string) => ask(id, (p) => p.collapse()),
      resize: (id: string, to: number | string) => ask(id, (p) => p.resize(to)),
    };
  });
  return api;
}

/** The sizes saved under `key`; else, once, an older layout under `legacy`, as shares of the window. */
function readSizes(key: string, legacy: string[], ids: string[], orientation: Orientation): PanelSizes {
  let sizes = readJson(key, null, isRecord) as PanelSizes | null;
  for (const k of legacy) {
    const layout = readJson(k, null, isRecord);
    if (!sizes && layout && ids.every((id) => typeof layout[id] === "number")) {
      sizes = remembered({}, layout as Layout, orientation === "horizontal" ? innerWidth : innerHeight, ids);
      writeJson(key, sizes);
    }
    // Gone even when unreadable: it's read once.
    try {
      localStorage.removeItem(k);
    } catch {
      // Storage off: nothing to remove.
    }
  }
  return sizes ?? {};
}
