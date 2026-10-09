import { useEffect } from "react";
import { browserApi } from "@/lib/api";
import { nextScheme, type Scheme, stepZoom } from "@/lib/browser/look";
import type { Selection } from "@/lib/repo/selection";

type BrowserSelection = Extract<Selection, { kind: "browser" }>;

/** A tab's zoom and light or dark (look.ts), kept with the tab and given to its view each time it's made. */
export function usePageLook(tabKey: string, sel: BrowserSelection, onUpdate: (key: string, sel: Selection) => void, made: boolean) {
  const { id, zoom = 1, scheme } = sel;
  useEffect(() => {
    if (made) void browserApi.zoom(id, zoom).catch(() => {});
  }, [made, id, zoom]);
  useEffect(() => {
    if (made) void browserApi.appearance(id, scheme ? scheme === "dark" : null).catch(() => {});
  }, [made, id, scheme]);
  const save = (change: { zoom?: number; scheme?: Scheme }) => {
    const { zoom: _, scheme: __, ...rest } = sel;
    const next = { zoom: sel.zoom, scheme, ...change };
    onUpdate(tabKey, { ...rest, ...(next.zoom !== undefined && next.zoom !== 1 && { zoom: next.zoom }), ...(next.scheme && { scheme: next.scheme }) });
  };
  return {
    zoom,
    scheme,
    /** A step in (1) or out (-1), or back to 100% (0). */
    zoomBy: (by: -1 | 0 | 1) => save({ zoom: stepZoom(zoom, by) }),
    cycleScheme: () => save({ scheme: nextScheme(scheme) }),
  };
}
