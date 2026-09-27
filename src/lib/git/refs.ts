import type { Branch } from "../api/types.ts";
import { IS_MAC, IS_WINDOWS } from "../platform.ts";

/** refs/heads/main → main, refs/remotes/origin/main → origin/main. */
export const shortRef = (ref: string) => ref.replace(/^refs\/(heads|remotes|tags)\//, "");

// GitHub Desktop's sanitize-ref-name.ts, plus what it lets through that git refuses: control
// characters and space, DEL, ~ ^ : ? * [ \ | " < >, "@{", "..", a component starting with a dot,
// a trailing dot or "/", a component ending in ".lock"; then "//", a leading - + . or /, and HEAD.
const INVALID_REF = /[\x00-\x20\x7F~^:?*[\\|"<>]+|@{|\.\.+|(?<=^|\/)\.|\.$|\.lock(?=\/|$)|\/$/g;

/** A typed branch or tag name as git will take it: "fix login bug" → "fix-login-bug". */
export function sanitizedRefName(name: string) {
  const s = name.replace(INVALID_REF, "-").replace(/\/{2,}/g, "/").replace(/^[-+./]*/, "");
  return s === "HEAD" || s === "@" ? `${s}-` : s;
}

// Refs are files, often, and macOS and Windows file systems ignore case: there Master is master.
const FOLD_CASE = IS_MAC || IS_WINDOWS;

/** Whether two branch names are the same branch here. */
export const sameRef = (a: string, b: string, fold = FOLD_CASE) => (fold ? a.toLowerCase() === b.toLowerCase() : a === b);

/** Local branch names but `except`: what a new branch name must not be. */
export const localNames = (branches: Pick<Branch, "name" | "remote">[], except?: string) => branches.filter((b) => !b.remote && b.name !== except).map((b) => b.name);

/** A typed name as it will be used, and the line to show under it: what it became, or that `existing` has it. */
export function refNameCheck(typed: string, existing: string[], rename = false, fold = FOLD_CASE) {
  const name = sanitizedRefName(typed.trim());
  const clash = name ? existing.find((n) => sameRef(n, name, fold)) : undefined;
  const hint = clash ? `A branch named ${clash} already exists.` : name && name !== typed.trim() ? `Will be ${rename ? "renamed to" : "created as"} ${name}.` : null;
  return { name, taken: !!clash, hint };
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
