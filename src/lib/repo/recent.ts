import { readJson, stringList, writeJson } from "../storage";

// The projects list: every repo opened, in the user's order.
const RECENT_KEY = "gitviber.recent";

export function recentRepos(): string[] {
  // Saved lists have held nulls (an undefined path serializes as null), and one null crashed
  // the project switcher's sortable list.
  return stringList(readJson<unknown>(RECENT_KEY, []));
}

/** Adds a repo to the projects list. The user owns the order: opening never moves it. */
export function rememberRepo(path: string) {
  const list = recentRepos();
  if (!list.includes(path)) writeJson(RECENT_KEY, [...list, path]);
}

export function setRepoOrder(list: string[]) {
  writeJson(RECENT_KEY, list);
}

export function forgetRepo(path: string) {
  writeJson(RECENT_KEY, recentRepos().filter((p) => p !== path));
}

/** The repo to reopen on launch (the list order no longer tells). */
const LAST_KEY = "gitviber.last";
export function lastRepo(): string | null {
  try {
    return localStorage.getItem(LAST_KEY);
  } catch {
    return null;
  }
}
export function setLastRepo(path: string) {
  try {
    localStorage.setItem(LAST_KEY, path);
  } catch {
    // Not critical.
  }
}
