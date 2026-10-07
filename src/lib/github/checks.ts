import type { CiCheck, CiState, CommitChecks } from "../api/github.ts";
import { shortDuration } from "../format.ts";

export const FAILING = new Set(["failure", "cancelled", "timed_out", "action_required", "startup_failure"]);

const WAITS: Record<string, string> = {
  queued: "Queued",
  in_progress: "In progress",
  // A deployment environment's protection rule: a reviewer, or a wait timer.
  waiting: "Waiting for approval",
  requested: "Requested",
  pending: "Pending",
};

/** Failing first, then running, then the rest, each in GitHub's order: what needs a look leads. */
export function sortChecks(checks: CiCheck[]) {
  const rank = (c: CiCheck) => (FAILING.has(c.state) ? 0 : c.state === "pending" ? 1 : 2);
  return [...checks].sort((a, b) => rank(a) - rank(b));
}

/** The commit's verdict as these checks give it; undefined with none. */
export function checksVerdict(checks: CiCheck[]): CiState | undefined {
  if (!checks.length) return undefined;
  if (checks.some((c) => FAILING.has(c.state))) return "failure";
  return checks.some((c) => c.state === "pending") ? "pending" : "success";
}

/** "3/5 passed · 1 failing · 1 running": what isn't a pass, counted. */
export function checksSummary(checks: CiCheck[]) {
  const count = (test: (c: CiCheck) => boolean) => checks.filter(test).length;
  const parts = [`${count((c) => c.state === "success")}/${checks.length} passed`];
  const rest: [number, string][] = [
    [count((c) => FAILING.has(c.state)), "failing"],
    [count((c) => c.state === "pending"), "running"],
    [count((c) => c.state === "skipped"), "skipped"],
    // Neutral and stale, and whatever else GitHub says (a run done without a conclusion: "").
    [count((c) => !["success", "pending", "skipped"].includes(c.state) && !FAILING.has(c.state)), "neutral"],
  ];
  for (const [n, word] of rest) if (n) parts.push(`${n} ${word}`);
  return parts.join(" · ");
}

/**
 * A commit's badge text: the count from a read that saw every check, else `label` (the rollup's
 * "Checks failed"); a partial read can't say "2/2 passed".
 */
export function commitSummary(ci: CommitChecks | undefined, label: string) {
  if (ci?.checksError) return `${label} · some couldn't be read`;
  return ci?.checks.length ? checksSummary(ci.checks) : label;
}

/**
 * What shows beside a check's name: for a running one what it waits on and for how long
 * ("Queued · 3m"), for a done one how it ended and how long it took.
 */
export function checkNote(c: CiCheck, now = Date.now()) {
  const span = (from: string, to: number) => shortDuration((to - Date.parse(from)) / 1000);
  if (c.state === "pending") return [WAITS[c.status] ?? "Pending", c.startedAt && span(c.startedAt, now), c.description].filter(Boolean).join(" · ");
  return [c.description, c.startedAt && c.completedAt && span(c.startedAt, Date.parse(c.completedAt))].filter(Boolean).join(" · ");
}
