import { invalidate } from "@/lib/github/githubCache";
import { createStore } from "@/lib/store";

// PR views elsewhere (merge, create) tell the list to reload.
export const pullsChanged = createStore(0);
export const notifyPullsChanged = () => {
  invalidate("pulls:");
  pullsChanged.set(pullsChanged.get() + 1);
};

// Issue views elsewhere (close, edit, comment) tell the list to reload.
export const issuesChanged = createStore(0);
export const notifyIssuesChanged = () => {
  invalidate("issues:");
  issuesChanged.set(issuesChanged.get() + 1);
};
