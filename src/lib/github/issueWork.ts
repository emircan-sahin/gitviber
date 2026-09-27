// Starting work on an issue (the issue view's Start in a worktree): the branch it proposes, the
// Run command's {issue}, and the closing line a PR from that branch gets.
import { sanitizedRefName } from "../git/refs.ts";

const SLUG = 40;
// Letters NFKD leaves whole: Turkish's dotless ı among them, which would otherwise stay while its
// neighbors lose their marks.
const PLAIN: Record<string, string> = { ı: "i", ł: "l", ø: "o", đ: "d", ð: "d", þ: "th", ß: "ss", æ: "ae", œ: "oe" };

/** `12-short-title`, as GitHub's own "Create a branch" names one: lowercase, cut at a word. */
export function issueBranchName(number: number, title: string) {
  let slug = title
    .normalize("NFKD")
    .replace(/\p{M}/gu, "")
    .toLowerCase()
    .replace(/[ıłøđðþßæœ]/g, (c) => PLAIN[c])
    .replace(/['’]/g, "")
    .replace(/[^\p{L}\p{N}]+/gu, "-")
    .replace(/^-+|-+$/g, "");
  // By code point: cutting UTF-16 units could leave half an emoji, which no ref name may hold.
  const chars = Array.from(slug);
  if (chars.length > SLUG) {
    const cut = chars.slice(0, SLUG + 1).join("");
    slug = cut.includes("-") ? cut.slice(0, cut.lastIndexOf("-")) : chars.slice(0, SLUG).join("");
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

const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\/]/g, "\\$&");

/**
 * `body` with the closing line for `issueUrl` after it, unless it already closes that issue: any of
 * GitHub's keywords, before `#12` (in the PR's own repository), `owner/name#12` or the issue's url.
 */
export function withClosing(body: string, issueUrl: string | null, pullRepo: string) {
  const line = issueUrl && closingLine(issueUrl, pullRepo);
  if (!line) return body;
  const [, repo, number] = ISSUE_URL.exec(issueUrl)!;
  const refs = [`${escape(repo)}#${number}`, escape(issueUrl)];
  if (repo.toLowerCase() === pullRepo.toLowerCase()) refs.push(`#${number}`);
  const closes = new RegExp(`\\b(close[sd]?|fix(e[sd])?|resolve[sd]?)\\s*:?\\s+(${refs.join("|")})(?![\\w/])`, "i");
  if (closes.test(body)) return body;
  return body.trim() ? `${body.trimEnd()}\n\n${line}` : line;
}
