import type { Branch } from "../api/types.ts";

/** refs/heads/main → main, refs/remotes/origin/main → origin/main. */
export const shortRef = (ref: string) => ref.replace(/^refs\/(heads|remotes|tags)\//, "");

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
