// One commit's checks in detail (github/checks.rs commit_checks), for its header in the viewer.
import { useCallback, useEffect } from "react";
import { type CiState, type CommitChecks, github, repoOf } from "../api";
import { noteCi, useCi } from "./ci";
import { checksVerdict } from "./checks";
import { cached, useGitHubData } from "./githubCache";

// ci.ts's cadence: asked again every half minute while a check runs, rarely once all are done.
const RUNNING = 30_000;
const SETTLED = 10 * 60_000;

/** The verdict a read gives, if it saw every check: a partial or empty list leaves it to the rollup. */
const listed = (d: CommitChecks | undefined) => (d && !d.checksError && d.checks.length ? checksVerdict(d.checks) : undefined);

/**
 * The checks on commit `sha`, whose GitHub page is `url`, kept current while shown. Read only once
 * its rollup (which History has usually loaded) says it has some: most commits have none, and no
 * access or no account fails there, quietly and backed off. `web`: origin's page.
 */
export function useCommitChecks(sha: string, url: string, web: string | null) {
  // ci.ts keeps origin's answers under null, as History asks for them.
  const target = web && url.startsWith(`${web}/commit/`) ? null : repoOf(url);
  const rollup: CiState | undefined = useCi(target, [sha], false)[sha];
  const key = rollup ? `commit-checks:${url}` : null;
  const settled = !!key && (listed(cached<CommitChecks>(key)) ?? rollup) !== "pending";
  const read = useCallback(() => github.commitChecks(target, sha), [target, sha]);
  // Under the timer's period, so each tick reads again whatever its drift.
  const { data, error, refresh } = useGitHubData(key, read, settled ? SETTLED : RUNNING / 2);
  const fresh = listed(data);
  const state = fresh ?? rollup;
  // History's badge for it follows the newer answer at once.
  useEffect(() => {
    if (fresh) noteCi(target, sha, fresh);
  }, [target, sha, fresh]);
  useEffect(() => {
    if (state !== "pending") return;
    const timer = window.setInterval(() => document.visibilityState === "visible" && void refresh(), RUNNING);
    return () => window.clearInterval(timer);
  }, [state, refresh]);
  return { state, data, error };
}
