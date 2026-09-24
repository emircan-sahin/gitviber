// CI's verdict on commits, from GitHub's status rollup (github.rs ci_states): for the PR list and
// History. Asked in one request per list, kept a while, and asked again sooner while running.
import { useEffect, useState } from "react";
import { type CiState, github, type Target } from "./api";

const RUNNING = 30_000;
const SETTLED = 10 * 60_000;
// No GitHub account or no access: not asked again for a while.
const FAILED = 5 * 60_000;

const known = new Map<string, { state: CiState | null; at: number }>();
const failedAt = new Map<Target, number>();
const listeners = new Set<() => void>();

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
    listeners.forEach((l) => l());
  } catch {
    failedAt.set(target, Date.now());
  } finally {
    asking = new Set([...asking].filter((k) => !wanted.some((s) => k === key(target, s))));
  }
}

/** CI's state for each of `shas` that has one, kept current while shown. */
export function useCi(target: Target, shas: string[]): Record<string, CiState> {
  const [, bump] = useState(0);
  const list = shas.join(",");
  useEffect(() => {
    const l = () => bump((n) => n + 1);
    listeners.add(l);
    const all = list ? list.split(",") : [];
    void ask(target, all);
    const timer = window.setInterval(() => void ask(target, all), RUNNING);
    return () => {
      listeners.delete(l);
      window.clearInterval(timer);
    };
  }, [target, list]);
  const out: Record<string, CiState> = {};
  for (const s of shas) {
    const state = known.get(key(target, s))?.state;
    if (state) out[s] = state;
  }
  return out;
}
