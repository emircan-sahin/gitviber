import type { Pull } from "../api/github.ts";

const RANK: Record<Pull["state"], number> = { open: 3, merged: 2, closed: 1 };

/**
 * The PR a branch's badge shows, out of every cached list: one whose head is that branch of
 * `headRepo` (another fork's same-named branch isn't it). Open beats merged beats closed, then
 * the most recently updated, as cmux's sidebar picks. The same PR may sit in several lists.
 */
export function pullForBranch(pulls: Pull[], branch: string, headRepo: string): Pull | undefined {
  const repo = headRepo.toLowerCase();
  let best: Pull | undefined;
  for (const p of pulls) {
    if (p.headRef !== branch || p.headRepo?.toLowerCase() !== repo) continue;
    if (!best || RANK[p.state] > RANK[best.state] || (RANK[p.state] === RANK[best.state] && p.updatedAt > best.updatedAt)) best = p;
  }
  return best;
}
