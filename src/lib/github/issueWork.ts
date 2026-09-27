// Starting work on an issue (the issue view's Start in a worktree): the branch it proposes, the
// Run command's {issue}, and the closing line a PR from that branch gets.
import { sanitizedRefName } from "../git/refs.ts";

const SLUG = 40;

/** `12-short-title`, as GitHub's own "Create a branch" names one: lowercase, cut at a word. */
export function issueBranchName(number: number, title: string) {
  let slug = title
    .normalize("NFKD")
    .replace(/\p{M}/gu, "")
    .toLowerCase()
    .replace(/['’]/g, "")
    .replace(/[^\p{L}\p{N}]+/gu, "-")
    .replace(/^-+|-+$/g, "");
  if (slug.length > SLUG) {
    const cut = slug.slice(0, SLUG + 1);
    slug = cut.includes("-") ? cut.slice(0, cut.lastIndexOf("-")) : cut.slice(0, SLUG);
  }
  return sanitizedRefName(slug ? `${number}-${slug}` : `issue-${number}`);
}

/** The Run command with `{issue}` as the issue's number: `claude "Fix #{issue}"`. */
export const withIssue = (command: string, number: number) => command.replaceAll("{issue}", String(number));

const ISSUE_URL = /^https:\/\/github\.com\/([^/]+\/[^/]+)\/issues\/(\d+)$/;

/** `Closes #12` for an issue in the repository the PR goes to (owner/name), else `Closes owner/name#12`. */
export function closingLine(issueUrl: string, pullRepo: string) {
  const m = ISSUE_URL.exec(issueUrl);
  if (!m) return null;
  const [, repo, number] = m;
  return `Closes ${repo.toLowerCase() === pullRepo.toLowerCase() ? "" : repo}#${number}`;
}

/** `body` with `line` after it, unless it already closes that issue (any of GitHub's keywords). */
export function withClosing(body: string, line: string | null) {
  if (!line) return body;
  const ref = line.slice("Closes ".length).replace(/[.*+?^${}()|[\]\\/]/g, "\\$&");
  if (new RegExp(`\\b(close[sd]?|fix(e[sd])?|resolve[sd]?)\\s*:?\\s+${ref}\\b`, "i").test(body)) return body;
  return body.trim() ? `${body.trimEnd()}\n\n${line}` : line;
}
