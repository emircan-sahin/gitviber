import type { Branch } from "../api/types.ts";

/** refs/heads/main → main, refs/remotes/origin/main → origin/main. */
export const shortRef = (ref: string) => ref.replace(/^refs\/(heads|remotes|tags)\//, "");

// GitHub Desktop's sanitize-ref-name.ts: control characters and space, DEL, ~ ^ : ? * [ \ | " < >,
// "@{", "..", a leading or trailing dot, a trailing ".lock" or "/".
const INVALID_REF = /[\x00-\x20\x7F~^:?*[\\|"<>]+|@{|\.\.+|^\.|\.$|\.lock$|\/$/g;

/** A typed branch or tag name as git will take it: "fix login bug" → "fix-login-bug". */
export const sanitizedRefName = (name: string) => name.replace(INVALID_REF, "-").replace(/^[-+]*/, "");

/** Local branch names but `except`: what a new branch name must not be. */
export const localNames = (branches: Pick<Branch, "name" | "remote">[], except?: string) => branches.filter((b) => !b.remote && b.name !== except).map((b) => b.name);

/** A typed name as it will be used, and the line to show under it: what it became, or that it's taken. */
export function refNameCheck(typed: string, existing: string[], as = "created as", kind = "branch") {
  const name = sanitizedRefName(typed.trim());
  const taken = existing.includes(name);
  const hint = !name ? null : taken ? `A ${kind} named ${name} already exists.` : name !== typed.trim() ? `Will be ${as} ${name}` : null;
  return { name, taken, hint };
}

/**
 * What a branch review compares with at first: origin's default branch as last fetched (where a
 * pull request would go), else a local branch of that name; null when there's neither.
 */
export function reviewBase(branches: Pick<Branch, "name" | "remote" | "remoteDefault">[]): string | null {
  const name = branches.find((b) => b.remoteDefault && b.name.startsWith("origin/"))?.name.slice("origin/".length) ?? "main";
  if (branches.some((b) => b.remote && b.name === `origin/${name}`)) return `refs/remotes/origin/${name}`;
  if (branches.some((b) => !b.remote && b.name === name)) return `refs/heads/${name}`;
  return null;
}
