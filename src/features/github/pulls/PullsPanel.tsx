import { Plus, RefreshCw } from "lucide-react";
import { useCallback, useState } from "react";
import { useListFilter } from "@/components/ListFilter";
import { Button } from "@/components/ui/button";
import { Tip } from "@/components/ui/tooltip";
import { type Branch, type Commit, errorMessage, fullName, type GitHubAccess, github, type IssueLabel, isNotConnected, type Narrow, type Pull, type RepoStatus } from "@/lib/api";
import { cached, useGitHubData } from "@/lib/github/githubCache";
import type { Selection } from "@/lib/repo/selection";
import { cn } from "@/lib/utils";
import { RepoPanes } from "@/components/RepoPanes";
import { ConnectGitHub } from "@/features/github/shared/ConnectGitHub";
import { type Filter, filterCounts, FilterTabs } from "@/features/github/shared/FilterTabs";
import { LabelFilter, SelectedLabels } from "@/features/github/issues/LabelPicker";
import { NarrowBar } from "@/features/github/shared/NarrowBar";
import { type Choice, NO_CHOICE, narrowKey } from "@/features/github/shared/narrow";
import { NewButton } from "@/features/github/shared/NewButton";
import { useNarrow } from "@/features/github/shared/useNarrow";
import { pullsChanged } from "@/features/github/shared/changed";
import { useGitHubAccount, useReloadAll } from "@/features/github/shared/useGitHubAccount";
import { SignedInAs } from "@/features/github/shared/ListNotes";
import { PullRows } from "./PullRows";
import { CreatePullDialog } from "./CreatePullDialog";

interface Props {
  status: RepoStatus | null;
  branches: Branch[];
  /** HEAD: its subject titles a new PR, as GitHub does for a single commit. */
  lastCommit: Commit | null;
  /** The repository's main worktree: what the chips' choice is kept under. */
  repoKey: string | undefined;
  activeKey: string | null;
  onOpen: (s: Selection, pin?: boolean) => void;
  refreshRepo: () => Promise<void>;
}

/** `pages` of a repository's PR list. While one more loads, the pages before stay on screen. */
function usePullList(target: string | null, name: string | null, filter: Filter, narrow: Narrow, pages: number) {
  // Unnarrowed lists keep their key: the worktrees' PR badges read it too.
  const tag = narrowKey(narrow);
  const key = (n: number) => `pulls:${name}:${filter}:${tag && `${tag}:`}${n}`;
  const list = useGitHubData(name && key(pages), useCallback(() => github.list(target, filter, pages, narrow), [target, filter, pages, narrow]));
  const shorter = list.data === undefined && pages > 1 ? cached<Pull[]>(key(pages - 1)) : undefined;
  return { ...list, data: list.data ?? shorter, shown: shorter ? pages - 1 : pages };
}

export function PullsPanel({ status, branches, lastCommit, repoKey, activeKey, onOpen, refreshRepo }: Props) {
  const [filter, setFilter] = useState<Filter>("open");
  const [pages, setPages] = useState({ origin: 1, parent: 1 });
  const find = useListFilter("git", "Filter loaded pull requests");
  const match = (p: Pull) => find.matches(p.title, `#${p.number}`, p.author, p.headRef);
  // The repository the new PR goes to: origin, or a fork's parent.
  const [creating, setCreating] = useState<GitHubAccess | null>(null);
  const acct = useGitHubAccount();
  const { account, origin, upstream } = acct;
  const { choice, setChoice, labels, setLabels, narrow, meReason } = useNarrow("pulls", repoKey, account, acct.error);
  const firstPages = () => setPages({ origin: 1, parent: 1 });
  const pickChoice = (c: Choice) => {
    setChoice(c);
    firstPages();
  };
  const pickLabels = (l: IssueLabel[]) => {
    setLabels(l);
    firstPages();
  };
  // Origin's list loads alongside the account; a fork's parent is only known after it.
  const own = usePullList(null, "origin", filter, narrow, pages.origin);
  const up = usePullList(upstream, upstream, filter, narrow, pages.parent);
  const more = (pane: keyof typeof pages, list: typeof own) => ({
    shown: list.shown,
    loading: list.loading,
    onMore: () => setPages((p) => ({ ...p, [pane]: p[pane] + 1 })),
  });
  const failure = acct.error ?? own.error;
  const error = failure === undefined ? null : errorMessage(failure);
  const loading = acct.loading || own.loading || up.loading;

  // Counted apart from the list, which holds only the pages loaded.
  const ownCounts = useGitHubData(`pulls:counts:origin:${narrowKey(narrow)}`, useCallback(() => github.counts(null, narrow), [narrow]));
  const upCounts = useGitHubData(upstream ? `pulls:counts:${upstream}:${narrowKey(narrow)}` : null, useCallback(() => github.counts(upstream, narrow), [upstream, narrow]));
  const load = useReloadAll(pullsChanged, acct, own, up, ownCounts, upCounts);

  if (isNotConnected(failure)) return <ConnectGitHub onRetry={load} />;

  // This branch's open PR, in either repository. Another fork's same-named branch isn't it.
  const originName = origin ? fullName(origin.repo).toLowerCase() : null;
  const currentPull = status?.branch
    ? [...(own.data ?? []), ...(up.data ?? [])].find(
        (p) => p.headRef === status.branch && p.state === "open" && (!originName || p.headRepo?.toLowerCase() === originName),
      )
    : undefined;
  // A checked-out fork PR (pr/7) is someone else's: a new PR would publish it into origin.
  const newLabel = status?.follows ? `This branch is pull request ${status.follows}` : currentPull ? `#${currentPull.number} already open for this branch` : "New pull request";
  const canCreate = !!status?.branch && !status.follows && !currentPull;

  const clearNarrow = () => {
    setChoice(NO_CHOICE);
    pickLabels([]);
  };
  const ownRows = <PullRows pulls={own.data ?? null} match={find.needle ? match : null} error={error} filter={filter} narrow={narrow} onClearNarrow={clearNarrow} activeKey={activeKey} onOpen={onOpen} account={account} roomy={!upstream} target={null} {...more("origin", own)} />;

  return (
    <div className="flex h-full flex-col">
      {/* A container: narrower, the Label and New buttons drop their words, then the counts go (as the issues' header). */}
      <div className="@container flex h-8 shrink-0 items-center gap-1 border-b border-border px-2">
        {/* Closed is All less Open, so it goes uncounted. A fork's two lists count in their own pane headers,
            so none show before the account says whether it's one. */}
        <FilterTabs
          value={filter}
          onChange={(f) => {
            setFilter(f);
            firstPages();
          }}
          counts={!account || upstream || !ownCounts.data ? undefined : { ...filterCounts(ownCounts.data), closed: undefined }}
        />
        <div className="ml-auto flex items-center gap-0.5">
          <LabelFilter
            upstream={upstream}
            selected={labels}
            onChange={pickLabels}
            counted={!upstream}
            noun="Pull requests"
          />
          <Tip label="Refresh">
            <Button variant="ghost" size="icon-sm" onClick={load} disabled={loading}>
              <RefreshCw className={cn(loading && "animate-spin")} />
            </Button>
          </Tip>
          {/* A fork has one per pane: a PR goes either to the fork or to the original. */}
          {!upstream && (
            <Tip label={newLabel}>
              <span>
                <Button variant="secondary" size="sm" disabled={!origin || !canCreate} onClick={() => setCreating(origin)}>
                  <Plus /> <span className="@max-[380px]:hidden">New</span>
                </Button>
              </span>
            </Tip>
          )}
        </div>
      </div>
      <NarrowBar
        kind="pulls"
        choice={choice}
        onChange={pickChoice}
        meReason={meReason}
      />
      <SelectedLabels labels={labels} onChange={pickLabels} />
      {find.bar}
      {upstream && account?.parent ? (
        <div className="min-h-0 flex-1">
          <RepoPanes
            id="pulls"
            panes={[
              {
                id: "origin",
                title: "Your fork",
                detail: origin ? fullName(origin.repo) : "",
                badge: ownCounts.data ? filterCounts(ownCounts.data)[filter] : undefined,
                actions: <NewButton label={newLabel} disabled={!origin || !canCreate} onClick={() => setCreating(origin)} />,
                children: ownRows,
              },
              {
                id: "parent",
                title: "Original",
                detail: upstream,
                badge: upCounts.data ? filterCounts(upCounts.data)[filter] : undefined,
                actions: <NewButton label={newLabel} disabled={!canCreate} onClick={() => setCreating(account.parent)} />,
                children: (
                  <PullRows
                    pulls={up.data ?? null}
                    match={find.needle ? match : null}
                    error={up.error === undefined ? null : errorMessage(up.error)}
                    filter={filter}
                    narrow={narrow}
                    onClearNarrow={clearNarrow}
                    activeKey={activeKey}
                    onOpen={onOpen}
                    account={account}
                    roomy={false}
                    target={upstream}
                    {...more("parent", up)}
                  />
                ),
              },
            ]}
          />
        </div>
      ) : (
        <div className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto py-1">{ownRows}</div>
      )}
      {account && <SignedInAs account={account} />}
      {creating && account?.origin && status?.branch && (
        <CreatePullDialog
          target={creating}
          origin={account.origin}
          status={status}
          branches={branches}
          // A merge (say, of the original's changes) says nothing about this branch's work.
          defaultTitle={lastCommit && lastCommit.parents.length === 1 ? lastCommit.subject : status.branch}
          onClose={() => setCreating(null)}
          onPushChanged={refreshRepo}
          onCreated={async (p) => {
            setCreating(null);
            // The dialog's notifyPullsChanged reloads the list.
            await refreshRepo();
            onOpen({ kind: "pull", pull: p }, true);
          }}
        />
      )}
    </div>
  );
}
