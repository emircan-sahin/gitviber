// Fixed panels' sizes (the sidebars, the terminal) in pixels, for usePanelSizes. Pure, so it runs
// under `node --test`.

/** A fixed panel as the user left it: its open size in pixels, and whether they closed it. */
export interface PanelSize {
  size?: number;
  collapsed?: boolean;
}

export type PanelSizes = Record<string, PanelSize>;

/** The pixels `share` percent of a group `total` pixels long comes to. */
export const toPixels = (share: number, total: number) => Math.round((share / 100) * total);

/** The size a panel opens at: as it was left, else `base`. */
export const openSize = (saved: PanelSize | undefined, base: number | string): number | string =>
  typeof saved?.size === "number" && saved.size > 0 ? saved.size : base;

/** The size a panel mounts at: closed when it was left closed. */
export const initialSize = (saved: PanelSize | undefined, base: number | string) => (saved?.collapsed === true ? 0 : openSize(saved, base));

/**
 * `sizes` with each of `ids` as `layout` (percentages of a group `total` pixels long) has it. A
 * collapsed panel keeps the size it reopens at; one missing from the layout (not shown) stays as it was.
 */
export function remembered(sizes: PanelSizes, layout: Record<string, number>, total: number, ids: string[]): PanelSizes {
  const next = { ...sizes };
  for (const id of ids) {
    const share = layout[id];
    if (share === undefined) continue;
    next[id] = share > 0 ? { size: toPixels(share, total) } : { ...sizes[id], collapsed: true };
  }
  return next;
}

/**
 * The width a panel header needs to show all of itself: its first child, a row of tabs that scrolls
 * when it doesn't fit, unscrolled; its last, the buttons pinned to the right; no room to spare.
 */
export function headerWidth(header: Element): number {
  const box = header.getBoundingClientRect();
  const tabs = header.firstElementChild;
  const buttons = header.lastElementChild;
  if (!tabs || !buttons) return box.width;
  const b = buttons.getBoundingClientRect();
  // The padding either side; on the right only while the buttons fit (in a collapsed panel they don't).
  return Math.ceil(tabs.getBoundingClientRect().left - box.left + tabs.scrollWidth + b.width + Math.max(0, box.right - b.right));
}
