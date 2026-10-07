// One commit's checks in detail (github/checks.rs commit_checks), for its header in the viewer;
// a History row's tooltip reads what that header already loaded.
import { useCallback, useEffect } from "react";
import { type CiState, type CommitChecks, github, repoOf } from "../api";
import { useCi } from "./ci";
import { checksSummary, checksVerdict } from "./checks";
import { cached, useGitHubData } from "./githubCache";

// ci.ts's cadence: asked again every half minute while a check runs, rarely once all are done.
const RUNNING = 30_000;
const SETTLED = 10 * 60_000;

const keyOf = (url: string) => `commit-checks:${url}`;

/**
 * The summary of a commit's checks (`url`: its GitHub page) if its header read them and they
 * still agree with the list's `state`; asks nothing.
 */
export function knownSummary(url: string, state: CiState) {
  const checks = cached<CommitChecks>(keyOf(url))?.checks;
  return checks && checksVerdict(checks) === state ? checksSummary(checks) : null;
}

/**
 * The checks on the commit at GitHub page `url`, kept current while shown. Read only once its
 * rollup (which History has usually loaded) says it has some: most commits have none, and no
 * access or no account fails there, quietly and backed off. `web`: origin's page.
 */
export function useCommitChecks(url: string | undefined, web: string | null) {
  const sha = url ? url.slice(url.lastIndexOf("/") + 1) : "";
  // ci.ts keeps origin's answers under null, as History asks for them.
  const target = !url || (web && url.startsWith(`${web}/commit/`)) ? null : repoOf(url);
  const rollup: CiState | undefined = useCi(target, sha ? [sha] : [], false)[sha];
  const key = url && rollup ? keyOf(url) : null;
  const settled = !!key && cached<CommitChecks>(key)?.checks.every((c) => c.state !== "pending");
  const read = useCallback(() => github.commitChecks(target, sha), [target, sha]);
  // Under the timer's period, so each tick reads again whatever its drift.
  const { data, error, refresh } = useGitHubData(key, read, settled ? SETTLED : RUNNING / 2);
  const running = data ? data.checks.some((c) => c.state === "pending") : rollup === "pending";
  useEffect(() => {
    if (!running) return;
    const timer = window.setInterval(() => document.visibilityState === "visible" && void refresh(), RUNNING);
    return () => window.clearInterval(timer);
  }, [running, refresh]);
  return { rollup, data, error };
}
