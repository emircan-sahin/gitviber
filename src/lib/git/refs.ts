import type { Branch } from "../api/types.ts";
import { plural } from "../format.ts";
import { folderName } from "../path.ts";
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

type BranchRow = Pick<Branch, "name" | "remote" | "current" | "worktree">;

/**
 * What a new worktree for the typed name checks out. A local branch as it is, unless a worktree
 * has it already (git keeps a branch in one at a time). A branch only on a remote, as a local one
 * tracking it: origin's where several remotes have it, or the one typed as `upstream/feat`. Else a
 * new branch, from the base picked. `base`: what to give add_worktree, undefined for the picked one;
 * `track`: whether the branch tracks it.
 */
export function worktreeBranch(typed: string, branches: BranchRow[], fold = FOLD_CASE) {
  const check = refNameCheck(typed, localNames(branches), false, fold);
  // A remote's name ends at the first "/", as everywhere in the app: "my/fork" remotes aren't told apart.
  const remotes = branches.filter((b) => b.remote);
  // "upstream/feat" is feat from upstream: a local branch by that name would be ambiguous.
  const localOf = (n: string) => branches.find((b) => !b.remote && sameRef(b.name, n, fold));
  const named = localOf(check.name) ? undefined : remotes.find((b) => b.name === check.name);
  const name = named ? named.name.slice(named.name.indexOf("/") + 1) : check.name;
  const local = localOf(name);
  if (local) {
    const held = local.current ? "here" : local.worktree && `in ${folderName(local.worktree)}`;
    if (held) return { name: local.name, base: undefined, track: false, hint: `${local.name} is checked out ${held}; a branch can be in one worktree at a time.`, taken: true };
    return { name: local.name, base: null, track: false, hint: `Checks out the existing branch ${local.name}.`, taken: false };
  }
  const namesakes = remotes.filter((b) => b.name.slice(b.name.indexOf("/") + 1) === name);
  const remote = named ?? namesakes.find((b) => b.name.startsWith("origin/")) ?? namesakes[0];
  if (remote) return { name, base: `refs/remotes/${remote.name}`, track: true, hint: `Checks out ${remote.name} as a new tracking branch ${name}.`, taken: false };
  const prefix = remotes.map((b) => b.name.slice(0, b.name.indexOf("/") + 1)).find((p) => name.startsWith(p));
  if (prefix) return { name, base: undefined, track: false, hint: `${prefix} is a remote's; a branch named ${name} would be ambiguous.`, taken: true };
  return { name, base: undefined, track: false, hint: check.hint, taken: false };
}

/**
 * Where a new worktree starts: the branch checked out here, as GitHub Desktop and VS Code have it.
 * Detached, the default branch, as git/branch.rs's default_branch finds it: local if there is one.
 */
export function worktreeBase(branches: Pick<Branch, "name" | "remote" | "current" | "remoteDefault">[]) {
  const current = branches.find((b) => b.current && !b.remote);
  if (current) return `refs/heads/${current.name}`;
  const remote = branches.find((b) => b.remoteDefault && b.name.startsWith("origin/"));
  const name = remote ? remote.name.slice("origin/".length) : "main";
  if (branches.some((b) => !b.remote && b.name === name)) return `refs/heads/${name}`;
  return remote ? `refs/remotes/${remote.name}` : "HEAD";
}

/**
 * How a local branch stands with its upstream, for the branch picker: `text` to show ("↑2 ↓1",
 * "local only", "upstream gone"), `label` for a screen reader and the tooltip. Null when even, or remote.
 */
export function branchTracking(b: Pick<Branch, "remote" | "upstream" | "ahead" | "behind" | "upstreamGone">): { text: string; label: string } | null {
  if (b.remote) return null;
  if (b.upstreamGone) return { text: "upstream gone", label: ["Its upstream", b.upstream, "is gone from the remote"].filter(Boolean).join(" ") };
  if (!b.upstream) return { text: "local only", label: "Not published: it has no upstream" };
  const parts = [b.ahead && [`↑${b.ahead}`, `${plural(b.ahead, "commit")} ahead`], b.behind && [`↓${b.behind}`, `${plural(b.behind, "commit")} behind`]].filter((p): p is string[] => !!p);
  if (!parts.length) return null;
  return { text: parts.map((p) => p[0]).join(" "), label: `Compared with ${b.upstream}: ${parts.map((p) => p[1]).join(", ")}` };
}
