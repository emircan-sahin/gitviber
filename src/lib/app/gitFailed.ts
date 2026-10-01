import { api, errorMessage, github } from "../api";
import { explainGitError, type GitFix, SIGNING_HELP } from "../git/gitErrors";
import { askForIdentity } from "./identity";
import { explainedError, failed, toast, type ToastAction } from "./toast";

/** The buttons for each way out that a caller can take, or a function of git's words that picks them. */
export type GitFixes = Partial<Record<GitFix, ToastAction[] | ((message: string) => ToastAction[] | undefined)>>;

/** The ways out that fit wherever the failure happens; `target` is what git's words named. */
const always = (target = ""): Partial<Record<GitFix, ToastAction[]>> => ({
  identity: [{ label: "Set name and email", run: askForIdentity }],
  signing: [{ label: "Signing guide", run: () => void github.openUrl(SIGNING_HELP).catch(failed("Could not open the link")) }],
  "index-lock": [
    {
      label: "Remove lock",
      run: () =>
        void api.removeIndexLock(target).then(() => toast("success", "Index lock removed", "Try again now."), failed("Could not remove the lock")),
    },
  ],
  secret: target ? [{ label: "Open on GitHub", run: () => void github.openUrl(target).catch(failed("Could not open the link")) }] : [],
});

/**
 * The error toast for a failed git action: git's own words, or for a failure gitErrors knows,
 * what it means and the way out, with git's words under Details.
 */
export function gitFailed(title: string, e: unknown, fixes: GitFixes = {}) {
  const message = errorMessage(e);
  const help = explainGitError(message);
  if (!help) return toast("error", title, message);
  const caller = help.fix && fixes[help.fix];
  const picked = typeof caller === "function" ? caller(message) : caller;
  const actions = help.fix ? [...(picked ?? []), ...(always(help.target)[help.fix] ?? [])] : [];
  explainedError(title, `${help.title}\n${help.explanation}`, message, actions);
}
