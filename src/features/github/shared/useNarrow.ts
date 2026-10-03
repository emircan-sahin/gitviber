import { useMemo, useState } from "react";
import type { IssueLabel, Narrow } from "@/lib/api";
import { putRecent, readJson } from "@/lib/storage";
import { type Choice, type ListKind, narrowKey, parseChoice, withChoice } from "./narrow";

const KEY = "gitviber.github.narrow";
/** Repositories kept, the most recently changed. */
const MAX_REPOS = 50;

const saved = (repoKey: string | undefined, kind: ListKind) => parseChoice(repoKey ? readJson<Record<string, unknown>>(KEY, {})[repoKey] : null, kind);

/**
 * What a PR or issue list is narrowed to: the chips' choice, kept per repository (`repoKey` is
 * its main worktree, as every worktree of it lists the same PRs), and the labels picked. `meUsable`
 * false (the account couldn't be read) lists everyone's: the choice stays for when it can.
 */
export function useNarrow(kind: ListKind, repoKey: string | undefined, meUsable: boolean) {
  const [state, setState] = useState(() => ({ repoKey, choice: saved(repoKey, kind) }));
  const choice = state.repoKey === repoKey ? state.choice : saved(repoKey, kind);
  const [labels, setLabels] = useState<IssueLabel[]>([]);
  const setChoice = (next: Choice) => {
    setState({ repoKey, choice: next });
    if (!repoKey) return;
    const all = readJson<Record<string, unknown>>(KEY, {});
    putRecent(KEY, repoKey, withChoice(all[repoKey], kind, next), MAX_REPOS);
  };
  const names = labels.map((l) => l.name);
  const key = narrowKey({ scope: meUsable ? choice.scope : null, draft: choice.draft, labels: names });
  // Stable while nothing changed, as the lists' loaders depend on it (`key` stands for the three).
  const narrow = useMemo<Narrow>(() => ({ scope: meUsable ? choice.scope : null, draft: choice.draft, labels: names }), [key]);
  return { choice, setChoice, labels, setLabels, narrow };
}
