import type { Worktree } from "../api";
import { loadWorktreeColors, saveWorktreeColor } from "../repo/session";
import { createStore } from "../store";
import { type HueChoice, worktreeHue } from "./worktrees";

// The colors picked for worktrees, kept with the rest of a worktree's state (repo/session).
const picked = createStore<Record<string, HueChoice>>(loadWorktreeColors());

/** The colors picked, by worktree path, for worktreeHue. */
export const useWorktreeColors = picked.use;

export function setWorktreeColor(path: string, choice: HueChoice) {
  saveWorktreeColor(path, choice);
  picked.set(loadWorktreeColors());
}

/**
 * A worktree was renamed with its folder (moveRoot took a picked color along): the color its
 * old name gave it stays, rather than the new name's.
 */
export function keepColorOnRename(w: Pick<Worktree, "path" | "main">, to: string) {
  const before = picked.get();
  const hue = before[w.path] ? null : worktreeHue(w, before);
  if (hue) saveWorktreeColor(to, hue);
  picked.set(loadWorktreeColors());
}
