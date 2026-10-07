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

/** A fixed panel's bounds in pixels; a collapsible one may also be 0. */
export interface Limits {
  min: number;
  max: number;
  collapsible?: boolean;
}

/**
 * The fixed panels' sizes moved toward `want` (0: closed), in order, with what the flexible panel
 * has above its min (`room`) and what the others give up; never by taking from another fixed
 * panel, so two squeezed sidebars can't win room back from each other forever. A closed panel
 * opens only when it can reach its min.
 */
export function settle(current: Record<string, number>, want: Record<string, number>, limits: Record<string, Limits>, room: number): Record<string, number> {
  const ids = Object.keys(want);
  const target = (id: string) => (want[id] <= 0 && limits[id].collapsible ? 0 : Math.min(Math.max(want[id], limits[id].min), limits[id].max));
  const next = { ...current };
  let spare = Math.max(0, room);
  // Shrinks first: what they give up is room for the rest.
  for (const id of ids) {
    if (target(id) < current[id]) {
      spare += current[id] - target(id);
      next[id] = target(id);
    }
  }
  for (const id of ids) {
    if (target(id) <= current[id]) continue;
    const size = current[id] + Math.min(target(id) - current[id], spare);
    if (size < limits[id].min) continue;
    spare -= size - current[id];
    next[id] = size;
  }
  return next;
}
