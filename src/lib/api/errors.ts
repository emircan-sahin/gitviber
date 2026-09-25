/** A network command the user stopped: not a failure. */
export const CANCELLED = "git:cancelled";
/** open_repo on a folder that isn't in a repository; the page offers to initialize one. */
export const NOT_A_REPO = "git:not-a-repo";

/** What a search a newer one stopped rejects with. */
export const SEARCH_CANCELLED = "search:cancelled";
/** What a lookup a newer one stopped rejects with. */
export const DEFINITIONS_CANCELLED = "definitions:cancelled";
/** suggest.rs CANCELLED. */
export const SUGGEST_CANCELLED = "cancelled";

/** Backend's marker for "no GitHub credentials found" (show setup, not an error). */
export const GITHUB_NOT_CONNECTED = "github:not-connected";

const MARKERS = new Map([
  [GITHUB_NOT_CONNECTED, "GitHub sign-in missing or expired. Sign in again (see the PRs tab)."],
  [CANCELLED, "Cancelled"],
  [NOT_A_REPO, "This folder is not inside a git repository."],
]);

export function errorMessage(e: unknown) {
  const raw = typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
  return MARKERS.get(raw) ?? raw;
}

export const isNotConnected = (e: unknown) => e === GITHUB_NOT_CONNECTED;
