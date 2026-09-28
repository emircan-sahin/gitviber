import { isRecord, putRecent, readJson, stringList } from "../storage.ts";

// Per pull request (by url), the commits already looked at; the rest were pushed since and show as
// new. Kept for the most recently opened PRs.
const KEY = "gitviber.seenCommits";
const MAX = 50;

/** The commits of the PR at `url` already seen; null for a PR never opened. */
export function seenCommits(url: string): Set<string> | null {
  const v = readJson(KEY, {}, isRecord)[url];
  return Array.isArray(v) ? new Set(stringList(v)) : null;
}

/** Stores what's `seen` of the PR's commits now (`shas`): those a force-push replaced are left behind. */
export function saveSeenCommits(url: string, shas: string[], seen: Set<string>) {
  putRecent(KEY, url, shas.filter((s) => seen.has(s)), MAX);
}
