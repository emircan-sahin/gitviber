import { errorMessage, github } from "../api";
import { explainGitError, type GitFix, SIGNING_HELP } from "../git/gitErrors";
import { askForIdentity } from "./identity";
import { explainedError, failed, toast, type ToastAction } from "./toast";

/** The buttons for each way out that a caller can take. */
export type GitFixes = Partial<Record<GitFix, ToastAction[]>>;

const ALWAYS: GitFixes = {
  identity: [{ label: "Set name and email", run: askForIdentity }],
  signing: [{ label: "Signing guide", run: () => void github.openUrl(SIGNING_HELP).catch(failed("Could not open the link")) }],
};

/**
 * The error toast for a failed git action: git's own words, or for a failure gitErrors knows,
 * what it means and the way out, with git's words under Details.
 */
export function gitFailed(title: string, e: unknown, fixes: GitFixes = {}) {
  const message = errorMessage(e);
  const help = explainGitError(message);
  if (!help) return toast("error", title, message);
  explainedError(title, `${help.title}\n${help.explanation}`, message, (help.fix && (fixes[help.fix] ?? ALWAYS[help.fix])) || []);
}
