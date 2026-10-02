// The size fitPane gives a pane. Pure, so it runs under `node --test`.
import type { ITerminalDimensions } from "@xterm/addon-fit";

/** History lines past which a column change waits for the resize to settle (VS Code's threshold). */
export const REWRAP_LINES = 200;

/**
 * The size to give a pane now, null to leave it: `box` (its content box) is empty while hidden or not laid out.
 * Past REWRAP_LINES of `history`, new columns wait (`colsLater`) for a drag to settle, as in VS Code; rows follow at once.
 */
export function planFit(box: { width: number; height: number }, proposed: ITerminalDimensions | undefined, current: ITerminalDimensions, history: number): { size: ITerminalDimensions; colsLater: boolean } | null {
  if (!box.width || !box.height || !proposed || isNaN(proposed.cols) || isNaN(proposed.rows)) return null;
  if (history > REWRAP_LINES && proposed.cols !== current.cols) return { size: { cols: current.cols, rows: proposed.rows }, colsLater: true };
  return { size: proposed, colsLater: false };
}
