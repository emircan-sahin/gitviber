import { api, errorMessage, github } from "../api";
import { copyText } from "../app/clipboard";
import { failed, toast } from "../app/toast";
import { type GitHubSide, permalinkUrl } from "./permalink";

/** A GitHub page (a PR, an issue, a commit) in the browser. */
export const openOnGitHub = (url: string) => github.openUrl(url).catch(failed("Could not open"));

export const copyLink = (url: string) => copyText(url, "Link copied");

/** Copies or opens `side`'s permalink, to its `lines` if any; says why when GitHub doesn't have them yet. */
export async function gitHubLink(side: GitHubSide, lines: [number, number] | null, open: boolean) {
  let url = side.sha && permalinkUrl(side.web, side.sha, side.path, { lines });
  if (!url) {
    try {
      const p = await api.githubPermalink(side.path, lines);
      url = permalinkUrl(side.web, p.sha, side.path, { tree: p.tree, lines: p.lines });
    } catch (e) {
      return toast("info", "Not on GitHub yet", errorMessage(e));
    }
  }
  return open ? openOnGitHub(url) : copyLink(url);
}
