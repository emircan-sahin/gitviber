/** Where a side of the code view is on GitHub: `path` in commit `sha` of the repo at `web`. */
export interface GitHubSide {
  web: string;
  path: string;
  /** null: the file on disk, linked to the newest pushed commit whose lines match (worked out on use). */
  sha: string | null;
}

/** A page on GitHub for `path` at commit `sha`, so it keeps showing that code: a file, its lines as `#L10-L24`, or a folder. */
export function permalinkUrl(web: string, sha: string, path: string, { tree = false, lines = null }: { tree?: boolean; lines?: [number, number] | null } = {}) {
  const at = path ? `/${path.split("/").map(encodeURIComponent).join("/")}` : "";
  const anchor = lines ? (lines[0] === lines[1] ? `#L${lines[0]}` : `#L${lines[0]}-L${lines[1]}`) : "";
  return `${web}/${tree ? "tree" : "blob"}/${sha}${at}${anchor}`;
}

/** The repo's page from one of its commit pages (`…/commit/<sha>`). */
export const repoOfCommitUrl = (url: string) => url.slice(0, url.lastIndexOf("/commit/"));
