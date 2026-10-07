import { useMemo, useState } from "react";
import { type IssueLabel, isNotConnected, type Narrow } from "@/lib/api";
import { putRecent, readJson } from "@/lib/storage";
import { type Choice, type ListKind, narrowKey, parseChoice, withChoice } from "./narrow";

const KEY = "gitviber.github.narrow";
/** Repositories kept, the most recently changed. */
const MAX_REPOS = 50;

const saved = (repoKey: string | undefined, kind: ListKind) => parseChoice(repoKey ? readJson<Record<string, unknown>>(KEY, {})[repoKey] : null, kind);

/**
 * What a PR or issue list is narrowed to: the chips' choice, kept per repository (`repoKey` is
 * its main worktree, as every worktree of it lists the same PRs), and the labels and author picked. The chips
 * that name the account wait on it, but not for it: the saved one lists at once. Once it can't be
 * read (`meReason` says why) everyone's are listed, and the choice stays for when it can.
 */
export function useNarrow(kind: ListKind, repoKey: string | undefined, account: unknown, accountError: unknown) {
  const meUsable = accountError === undefined || !!account;
  const meReason = meUsable ? null : isNotConnected(accountError) ? "Sign in to GitHub to filter by you" : "Your GitHub account couldn't be read";
  const [state, setState] = useState(() => ({ repoKey, choice: saved(repoKey, kind) }));
  const choice = state.repoKey === repoKey ? state.choice : saved(repoKey, kind);
  const [labels, setLabels] = useState<IssueLabel[]>([]);
  const [author, setAuthorState] = useState<string | null>(null);
  const saveChoice = (next: Choice) => {
    setState({ repoKey, choice: next });
    if (!repoKey) return;
    const all = readJson<Record<string, unknown>>(KEY, {});
    putRecent(KEY, repoKey, withChoice(all[repoKey], kind, next), MAX_REPOS);
  };
  // "Created by me" and an author both say who opened it: picking one lets the other go.
  const setChoice = (next: Choice) => {
    if (next.scope === "created") setAuthorState(null);
    saveChoice(next);
  };
  const setAuthor = (login: string | null) => {
    setAuthorState(login);
    if (login !== null && choice.scope === "created") saveChoice({ ...choice, scope: null });
  };
  const names = labels.map((l) => l.name);
  const key = narrowKey({ scope: meUsable ? choice.scope : null, draft: choice.draft, labels: names, author });
  // Stable while nothing changed, as the lists' loaders depend on it (`key` stands for the four).
  const narrow = useMemo<Narrow>(() => ({ scope: meUsable ? choice.scope : null, draft: choice.draft, labels: names, author }), [key]);
  return { choice, setChoice, labels, setLabels, author, setAuthor, narrow, meReason };
}
