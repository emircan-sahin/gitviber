// The size fitPane gives a pane. Pure, so it runs under `node --test`.

export interface GridSize {
  cols: number;
  rows: number;
}

/** History lines past which a column change waits for the resize to settle (VS Code's threshold). */
export const REWRAP_LINES = 200;

/**
 * The size to give a pane now, and whether its new columns follow later; null leaves it as it is.
 * `box`: the pane's content box, 0 high while hidden or not laid out yet (the panel's padding
 * aside), where a fit gives one row. A shell that shrinks to it and back before its pty hears
 * (PTY_RESIZE_WAIT) never repaints: xterm dropped the lines under the cursor on the way.
 * `history`: the lines a column change would rewrap; past REWRAP_LINES the columns wait 100 ms for
 * a drag to settle, as in VS Code, the rows following at once.
 */
export function planFit(box: { width: number; height: number }, proposed: GridSize | undefined, current: GridSize, history: number | null): { size: GridSize; colsLater: boolean } | null {
  if (!box.width || !box.height || !proposed || isNaN(proposed.cols) || isNaN(proposed.rows)) return null;
  if (history !== null && history > REWRAP_LINES && proposed.cols !== current.cols) return { size: { cols: current.cols, rows: proposed.rows }, colsLater: true };
  return { size: { cols: proposed.cols, rows: proposed.rows }, colsLater: false };
}
