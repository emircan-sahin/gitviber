import type { Branch } from "../api/types.ts";
import type { ComparePoint } from "../repo/selection.ts";
import { reviewBase, shortRef } from "./refs.ts";

/** HEAD itself: what a detached checkout compares as. */
export const HEAD_POINT: ComparePoint = { ref: "HEAD", label: "HEAD" };

export const branchPoint = (b: Pick<Branch, "name" | "remote">): ComparePoint => ({ ref: `refs/${b.remote ? "remotes" : "heads"}/${b.name}`, label: b.name });
export const tagPoint = (name: string): ComparePoint => ({ ref: `refs/tags/${name}`, label: name });
export const commitPoint = (sha: string): ComparePoint => ({ ref: sha, label: sha.slice(0, 7) });

export type PointKind = "head" | "branch" | "remote" | "tag" | "commit";
export const pointKind = (ref: string): PointKind =>
  ref === "HEAD" ? "head" : ref.startsWith("refs/heads/") ? "branch" : ref.startsWith("refs/remotes/") ? "remote" : ref.startsWith("refs/tags/") ? "tag" : "commit";

/** Where the Compare screen starts: origin's default branch (where a PR would go) against the branch checked out. */
export function defaultPoints(branches: Pick<Branch, "name" | "remote" | "remoteDefault">[], current: string | null) {
  const base = reviewBase(branches);
  return { base: base ? { ref: base, label: shortRef(base) } : HEAD_POINT, head: current ? branchPoint({ name: current, remote: false }) : HEAD_POINT };
}

/**
 * What `git merge` is given for a point: a branch by its name, as the branch picker does; a tag by
 * its full ref, which a branch of the same name couldn't be taken for; a commit by its id.
 */
export const mergeName = (p: ComparePoint) => (pointKind(p.ref) === "tag" ? p.ref : shortRef(p.ref));

/**
 * GitHub's page for a comparison, as `base...head` (a PR's view) or `base..head`. Only what GitHub
 * knows by name: a branch, a tag or a commit, and a remote branch only of `origin`.
 */
export function githubCompareUrl(web: string | null, base: ComparePoint, head: ComparePoint, mergeBase: boolean): string | null {
  const name = (p: ComparePoint) => {
    const kind = pointKind(p.ref);
    if (kind === "head") return null;
    if (kind === "remote") return p.ref.startsWith("refs/remotes/origin/") ? p.ref.slice("refs/remotes/origin/".length) : null;
    return kind === "commit" ? p.ref : shortRef(p.ref);
  };
  const [a, b] = [name(base), name(head)];
  if (!web || !a || !b) return null;
  const path = (n: string) => n.split("/").map(encodeURIComponent).join("/");
  return `${web}/compare/${path(a)}${mergeBase ? "..." : ".."}${path(b)}`;
}
