import { useCallback, useEffect } from "react";
import { fullName, github } from "@/lib/api";
import { useGitHubData } from "@/lib/github/githubCache";

/**
 * The signed-in account, one cache entry for every GitHub view, and the repositories it reaches:
 * origin and, for a fork, the original (`parent`, owner/name as `upstream`). It only changes with
 * a new sign-in: rechecked every 10 minutes and on every manual refresh or retry (a 304 when
 * nothing changed, so free). `enabled` false: not asked for until it's true.
 */
export function useGitHubAccount(enabled = true) {
  const acct = useGitHubData(enabled ? "account" : null, github.account, 600_000);
  const account = acct.data ?? null;
  const parent = account?.parent ?? null;
  return { ...acct, account, origin: account?.origin ?? null, parent, upstream: parent ? fullName(parent.repo) : null };
}

/**
 * Reloads every one of `loads` at once: returned for ⟳ and retry, and run whenever `changed`
 * says a view elsewhere wrote. `loads` is the same length on every render.
 */
export function useReloadAll(changed: { subscribe: (listener: () => void) => () => void }, ...loads: { refresh: (force?: boolean) => Promise<void> }[]) {
  const refreshes = loads.map((l) => l.refresh);
  const load = useCallback(() => {
    for (const refresh of refreshes) refresh(true);
  }, refreshes);
  useEffect(() => changed.subscribe(load), [load]);
  return load;
}
