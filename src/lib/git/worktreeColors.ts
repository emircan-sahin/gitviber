import { loadWorktreeColors, saveWorktreeColor } from "../repo/session";
import { createStore } from "../store";
import type { Hue, HueChoice } from "./worktrees";

// The colors picked for worktrees, kept with the rest of a worktree's state (repo/session).
const picked = createStore<Record<string, HueChoice>>(loadWorktreeColors());

/** The colors picked, by worktree path, for worktreeHues. */
export const useWorktreeColors = picked.use;
export const pickedColors = picked.get;

export function setWorktreeColor(path: string, choice: HueChoice) {
  saveWorktreeColor(path, choice);
  picked.set(loadWorktreeColors());
}

/**
 * A worktree was renamed with its folder (moveRoot took a picked color along): `hue`, the color
 * it showed without a pick, stays rather than the new name's.
 */
export function keepColorOnRename(from: string, to: string, hue: Hue | null) {
  if (!picked.get()[from] && hue) saveWorktreeColor(to, hue);
  picked.set(loadWorktreeColors());
}
