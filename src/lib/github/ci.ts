// CI's verdict on commits, from GitHub's status rollup (github/pulls.rs ci_states): for the PR list and
// History. Asked in one request per list, kept a while, and asked again sooner while running.
import { useEffect, useSyncExternalStore } from "react";
import { type CiState, github, type Target } from "../api";
import { createStore } from "../store";
import { onGitHubReset, onGitHubWake } from "./githubCache";

const RUNNING = 30_000;
const SETTLED = 10 * 60_000;
// No GitHub account or no access: not asked again for a while.
const FAILED = 5 * 60_000;

const known = new Map<string, { state: CiState | null; at: number }>();
const failedAt = new Map<Target, number>();
// Bumped when answers land, to re-render what shows them.
const version = createStore(0);
// Another repo, or origin moved: the old one's answers and failures don't hold for it.
onGitHubReset(() => {
  known.clear();
  failedAt.clear();
});

const key = (target: Target, sha: string) => `${target ?? ""}\0${sha}`;
const stale = (target: Target, sha: string, now: number) => {
  const k = known.get(key(target, sha));
  return !k || now - k.at > (k.state === "pending" ? RUNNING : SETTLED);
};

let asking = new Set<string>();
async function ask(target: Target, shas: string[]) {
  const now = Date.now();
  if (now - (failedAt.get(target) ?? 0) < FAILED) return;
  const wanted = shas.filter((s) => stale(target, s, now) && !asking.has(key(target, s))).slice(0, 100);
  if (!wanted.length) return;
  wanted.forEach((s) => asking.add(key(target, s)));
  try {
    const found = await github.ciStates(target, wanted);
    for (const s of wanted) known.set(key(target, s), { state: found[s] ?? null, at: Date.now() });
    version.set(version.get() + 1);
  } catch {
    failedAt.set(target, Date.now());
  } finally {
    asking = new Set([...asking].filter((k) => !wanted.some((s) => k === key(target, s))));
  }
}

/**
 * CI's state for each of `shas` that has one, kept current while shown. `live` false: asked again
 * only on focus and the GitHub views' 5-minute recheck, for a badge that's always on screen.
 */
export function useCi(target: Target, shas: string[], live = true): Record<string, CiState> {
  // With nothing to show, no re-render on every answer app-wide.
  useSyncExternalStore(version.subscribe, () => (shas.length ? version.get() : -1));
  const list = shas.join(",");
  useEffect(() => {
    const all = list ? list.split(",") : [];
    if (!all.length) return;
    void ask(target, all);
    if (!live) return onGitHubWake(() => void ask(target, all));
    const timer = window.setInterval(() => void ask(target, all), RUNNING);
    return () => window.clearInterval(timer);
  }, [target, list, live]);
  const out: Record<string, CiState> = {};
  for (const s of shas) {
    const state = known.get(key(target, s))?.state;
    if (state) out[s] = state;
  }
  return out;
}
