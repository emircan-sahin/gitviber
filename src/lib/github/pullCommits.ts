// A PR's commits as its page lists them (by day, oldest first), and which of them Files changed
// is narrowed to.
import type { Commit } from "../api/types.ts";

/** Picked commits: from `anchor` (the last plain click) to `to`, either way round. */
export interface CommitRun {
  anchor: string;
  to: string;
}

/** The commits `run` covers (oldest first); null when a force-push took either end away. */
export function pickedCommits(commits: Commit[], run: CommitRun | null): Commit[] | null {
  const a = run ? commits.findIndex((c) => c.sha === run.anchor) : -1;
  const b = run ? commits.findIndex((c) => c.sha === run.to) : -1;
  return a < 0 || b < 0 ? null : commits.slice(Math.min(a, b), Math.max(a, b) + 1);
}

/** What a picked run of commits is called: its short id, or its first and last. */
export const pickLabel = (picked: Commit[]) =>
  picked.length === 1 ? picked[0].shortSha : `${picked[0].shortSha}–${picked[picked.length - 1].shortSha}`;

/** The day a commit landed on the branch (CommitTime's time), in local time. */
export const commitDay = (c: Commit) => new Date(c.committedAt * 1000).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });

/** Runs of commits that landed on the branch the same day, in the list's order. */
export function byDay(commits: Commit[]) {
  const days: { day: string; list: Commit[] }[] = [];
  for (const c of commits) {
    const day = commitDay(c);
    if (days[days.length - 1]?.day === day) days[days.length - 1].list.push(c);
    else days.push({ day, list: [c] });
  }
  return days;
}
