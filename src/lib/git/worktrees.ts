import type { FileChange, Pull, Worktree, WorktreeState } from "../api";
import { folderName, isInside } from "../path.ts";

/** Where a worktree sits, as short as it can be said: inside the main one, or beside it. */
export function shortPath(path: string, main: string) {
  if (path === main) return ".";
  if (path.startsWith(`${main}/`)) return path.slice(main.length + 1);
  const parent = main.slice(0, main.lastIndexOf("/"));
  if (parent && path.startsWith(`${parent}/`)) return `../${path.slice(parent.length + 1)}`;
  return path;
}

/**
 * Files "stage all" / "commit all" may hand to git. Nested repositories are left out:
 * git would add each as a gitlink (a pointer to its commit), not its files.
 */
export function stageable(files: FileChange[]) {
  const paths = files.filter((f) => !f.nested).map((f) => f.path);
  return { paths, skipped: files.length - paths.length };
}

export const NESTED_EXPLAINED =
  "It's a separate git repository. git add would record only a pointer to its current commit (an embedded repo), not its files. Commit inside it instead. If it's a worktree that broke when the repo moved, git worktree repair reconnects it.";

/** The worktree `path` is in: the deepest, as agents' worktrees sit inside the main one. */
export function worktreeOf<W extends Pick<Worktree, "path">>(path: string, worktrees: W[]): W | undefined {
  return worktrees.filter((w) => path === w.path || isInside(path, w.path)).sort((a, b) => b.path.length - a.path.length)[0];
}

/** A worktree Clean up offers, and what says it's merged. */
export interface Cleanable {
  worktree: Worktree;
  /** "merged", or "#12 merged" when only its pull request says so (a squash git can't see yet). */
  why: string;
  /** That pull request's head: its branch goes too while it's still there. */
  mergedHead: string | null;
}

/**
 * The worktrees Clean up offers: merged (by git, or by its pull request), nothing uncommitted,
 * no terminal in it, not locked, not the main or the open one, and no other worktree inside it,
 * which removing it would take along. Those whose state isn't read yet aren't offered.
 */
export function cleanable(list: Worktree[], states: Record<string, WorktreeState>, pullOf: (branch: string | null) => Pull | undefined, terminalsIn: (path: string) => number): Cleanable[] {
  return list.flatMap((w) => {
    const s = states[w.path];
    const pull = pullOf(w.branch);
    const pulled = pull?.state === "merged";
    if (!s || w.main || w.current || w.locked || w.prunable || w.bare || s.uncommitted || !(s.merged || pulled)) return [];
    if (terminalsIn(w.path) || list.some((o) => isInside(o.path, w.path))) return [];
    return [{ worktree: w, why: s.merged ? "merged" : `#${pull!.number} merged`, mergedHead: pulled ? pull!.headSha : null }];
  });
}

/** The worktree colors: OKLCH hues spread around the wheel, drawn at each theme's --tint-l and --tint-c. */
export const HUES = { red: 25, orange: 60, yellow: 100, green: 150, teal: 190, blue: 245, violet: 295, pink: 345 } as const;
export type Hue = keyof typeof HUES;
/** A worktree's color as the user set it; "none" turns the default off. */
export type HueChoice = Hue | "none";
export const HUE_NAMES = Object.keys(HUES) as Hue[];

// Own keys only: "toString" is `in` every object.
export const isHueChoice = (v: unknown): v is HueChoice => v === "none" || (typeof v === "string" && Object.hasOwn(HUES, v));

/**
 * A worktree's color: the one picked for it, else one its folder's name always gives. The main
 * worktree has none unless picked: it's the baseline the others stand out from.
 */
export function worktreeHue(w: Pick<Worktree, "path" | "main">, picked: Record<string, HueChoice>): Hue | null {
  const p = picked[w.path];
  if (p) return p === "none" ? null : p;
  if (w.main) return null;
  let h = 0x811c9dc5;
  for (const c of folderName(w.path)) h = Math.imul(h ^ c.charCodeAt(0), 0x01000193);
  return HUE_NAMES[(h >>> 0) % HUE_NAMES.length];
}

/**
 * The colors of one repo's worktrees, by path. A picked one stands; each other linked one takes
 * its name's hue unless one before it in `list` already has that, then the least used from there
 * on round the wheel: names that hash alike still get apart, until there are more than eight.
 */
export function worktreeHues(list: Pick<Worktree, "path" | "main">[], picked: Record<string, HueChoice>): Map<string, Hue | null> {
  const used = new Map<Hue, number>(HUE_NAMES.map((h) => [h, 0]));
  const take = (h: Hue) => used.set(h, used.get(h)! + 1);
  for (const w of list) {
    const p = picked[w.path];
    if (p && p !== "none") take(p);
  }
  const out = new Map<string, Hue | null>();
  for (const w of list) {
    const named = worktreeHue(w, picked);
    if (picked[w.path] || !named) {
      out.set(w.path, named);
      continue;
    }
    const start = HUE_NAMES.indexOf(named);
    let best = named;
    for (let k = 1; k < HUE_NAMES.length; k++) {
      const h = HUE_NAMES[(start + k) % HUE_NAMES.length];
      if (used.get(h)! < used.get(best)!) best = h;
    }
    take(best);
    out.set(w.path, best);
  }
  return out;
}

/** The CSS color of `hue`, `alpha` of it over what's behind. */
export const hueColor = (hue: Hue, alpha = 1) => `oklch(var(--tint-l) var(--tint-c) ${HUES[hue]}${alpha < 1 ? ` / ${alpha}` : ""})`;
