import { github } from "../api";
import { copyText } from "../app/clipboard";
import { failed } from "../app/toast";

/** A GitHub page (a PR, an issue, a commit) in the browser. */
export const openOnGitHub = (url: string) => github.openUrl(url).catch(failed("Could not open"));

export const copyLink = (url: string) => copyText(url, "Link copied");
