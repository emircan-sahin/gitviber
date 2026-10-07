import { ListFilter, MessageSquare, Plus, RefreshCw } from "lucide-react";
import { RelativeTime } from "@/components/RelativeTime";
import { useCallback, useState } from "react";
import { useListFilter } from "@/components/ListFilter";
import { Button } from "@/components/ui/button";
import { ContextMenuItem, ContextMenuSeparator } from "@/components/ui/context-menu";
import { RowFilter } from "@/components/RowFilter";
import { Tip } from "@/components/ui/tooltip";
import { errorMessage, fullName, type Issue, type IssueLabel, isNotConnected, issues, type Narrow, type Target } from "@/lib/api";
import { useGitHubData } from "@/lib/github/githubCache";
import { type Selection, selectionKey } from "@/lib/repo/selection";
import { useListNav } from "@/lib/ui/useListNav";
import { cn } from "@/lib/utils";
import { NewButton } from "@/features/github/shared/NewButton";
import { LinkMenu } from "@/features/github/shared/LinkMenu";
import { type Filter, filterCounts, FilterTabs } from "@/features/github/shared/FilterTabs";
import { ConnectGitHub } from "@/features/github/shared/ConnectGitHub";
import { RepoPanes } from "@/components/RepoPanes";
import { IssueStateIcon } from "@/features/github/shared/StateBadges";
import { issuesChanged } from "@/features/github/shared/changed";
import { useGitHubAccount, useReloadAll } from "@/features/github/shared/useGitHubAccount";
import { ClearNarrow, EmptyNote, ListError, SignedInAs } from "@/features/github/shared/ListNotes";
import { NarrowBar } from "@/features/github/shared/NarrowBar";
import { emptyText, isNarrowed, narrowKey, NO_CHOICE } from "@/features/github/shared/narrow";
import { useNarrow } from "@/features/github/shared/useNarrow";
import { LabelChip } from "./IssueBadges";
import { LabelFilter, SelectedLabels } from "./LabelPicker";
import { CreateIssueDialog } from "./CreateIssueDialog";

/** `repoKey`: the repository's main worktree, what the chips' choice is kept under. */
export function IssuesPanel({ repoKey, activeKey, onOpen }: { repoKey: string | undefined; activeKey: string | null; onOpen: (s: Selection, pin?: boolean) => void }) {
  const [filter, setFilter] = useState<Filter>("open");
  const find = useListFilter("git", "Filter loaded issues");
  const match = (i: Issue) => find.matches(i.title, `#${i.number}`, i.author, ...i.labels.map((l) => l.name));
  // The repository the new issue goes to: origin (null), or a fork's parent.
  const [creating, setCreating] = useState<{ target: Target } | null>(null);
  const acct = useGitHubAccount();
  const { account, origin, parent, upstream } = acct;
  const { choice, setChoice, labels, setLabels, author, setAuthor, narrow, meReason } = useNarrow("issues", repoKey, account, acct.error);
  const addLabel = (label: IssueLabel) => setLabels((l) => (l.some((m) => m.name === label.name) ? l : [...l, label]));
  // Forks start with issues off: nothing is listed where they are.
  const tag = narrowKey(narrow);
  const own = useGitHubData(`issues:origin:${filter}:${tag}`, useCallback(() => issues.list(null, filter, narrow), [filter, narrow]));
  const up = useGitHubData(
    upstream && parent?.issues ? `issues:${upstream}:${filter}:${tag}` : null,
    useCallback(() => issues.list(upstream, filter, narrow), [upstream, filter, narrow]),
  );
  // Counted apart from the list, which holds only the 50 most recent.
  const ownCounts = useGitHubData(`issues:counts:origin:${tag}`, useCallback(() => issues.counts(null, narrow), [narrow]));
  const upCounts = useGitHubData(
    upstream && parent?.issues ? `issues:counts:${upstream}:${tag}` : null,
    useCallback(() => issues.counts(upstream, narrow), [upstream, narrow]),
  );
  const failure = acct.error ?? own.error;
  const error = failure === undefined ? null : errorMessage(failure);
  const loading = acct.loading || own.loading || up.loading;

  const load = useReloadAll(issuesChanged, acct, own, up, ownCounts, upCounts);

  if (isNotConnected(failure)) return <ConnectGitHub subject="issues" />;

  const clearNarrow = () => {
    setChoice(NO_CHOICE);
    setAuthor(null);
    setLabels([]);
  };
  const rowProps = { match: find.needle ? match : null, filter, narrow, onLabel: addLabel, onAuthor: setAuthor, onClearNarrow: clearNarrow, activeKey, onOpen };
  const ownRows = (roomy: boolean) => <IssueRows items={own.data ?? null} error={error} roomy={roomy} {...rowProps} />;

  return (
    <div className="flex h-full flex-col">
      {/* A container: narrower, the Label and New buttons drop their words, then the counts go (measured inside the padding: all of it needs ~385px, the counts ~260px). */}
      <div className="@container flex h-8 shrink-0 items-center gap-1 border-b border-border px-2">
        {/* Closed is All less Open, so it goes uncounted. A fork's two lists count in their own pane headers,
            so none show before the account says whether it's one. */}
        <FilterTabs value={filter} onChange={setFilter} counts={!account || parent || !origin?.issues || !ownCounts.data ? undefined : { ...filterCounts(ownCounts.data), closed: undefined }} />
        <div className="ml-auto flex items-center gap-0.5">
          <LabelFilter upstream={parent?.issues ? upstream : null} selected={labels} onChange={setLabels} counted={!parent} />
          <Tip label="Refresh">
            <Button variant="ghost" size="icon-sm" onClick={load} disabled={loading}>
              <RefreshCw className={cn(loading && "animate-spin")} />
            </Button>
          </Tip>
          {!parent && (
            <Tip label="New issue">
              <Button variant="secondary" size="sm" disabled={!origin} onClick={() => setCreating({ target: null })}>
                <Plus /> <span className="@max-[380px]:hidden">New</span>
              </Button>
            </Tip>
          )}
        </div>
      </div>
      <NarrowBar kind="issues" choice={choice} onChange={setChoice} author={author} onAuthor={setAuthor} meReason={meReason} />
      <SelectedLabels labels={labels} onChange={setLabels} />
      {find.bar}
      {parent && upstream ? (
        <div className="min-h-0 flex-1">
          <RepoPanes
            id="issues"
            panes={[
              {
                id: "origin",
                title: "Your fork",
                detail: origin ? fullName(origin.repo) : "",
                badge: origin?.issues && ownCounts.data ? filterCounts(ownCounts.data)[filter] : undefined,
                actions: origin?.issues && <NewButton label="New issue" onClick={() => setCreating({ target: null })} />,
                children: origin?.issues ? ownRows(false) : <IssuesOff repo={origin ? fullName(origin.repo) : "your fork"} />,
              },
              {
                id: "parent",
                title: "Original",
                detail: upstream,
                badge: parent.issues && upCounts.data ? filterCounts(upCounts.data)[filter] : undefined,
                actions: parent.issues && <NewButton label="New issue" onClick={() => setCreating({ target: upstream })} />,
                children: parent.issues ? (
                  <IssueRows items={up.data ?? null} error={up.error === undefined ? null : errorMessage(up.error)} roomy={false} {...rowProps} />
                ) : (
                  <IssuesOff repo={upstream} />
                ),
              },
            ]}
          />
        </div>
      ) : (
        <div className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto py-1">{ownRows(true)}</div>
      )}
      {account && <SignedInAs account={account} />}
      {creating && (
        <CreateIssueDialog
          target={creating.target}
          repo={creating.target ?? fullName(origin!.repo)}
          onClose={() => setCreating(null)}
          onCreated={(i) => {
            setCreating(null);
            onOpen({ kind: "issue", issue: i }, true);
          }}
        />
      )}
    </div>
  );
}

/** Forks start with issues off; the pane stays, so both repositories are always in view. */
function IssuesOff({ repo }: { repo: string }) {
  return <div className="px-4 py-3 text-center text-[12px] text-subtle">Issues are turned off in {repo}.</div>;
}

function IssueRows({
  items: loaded,
  match,
  error,
  filter,
  narrow,
  onLabel,
  onAuthor,
  onClearNarrow,
  activeKey,
  onOpen,
  roomy,
}: {
  items: Issue[] | null;
  /** The list filter's test, while it has text. */
  match: ((i: Issue) => boolean) | null;
  error: string | null;
  filter: Filter;
  narrow: Narrow;
  onLabel: (label: IssueLabel) => void;
  onAuthor: (login: string) => void;
  onClearNarrow: () => void;
  activeKey: string | null;
  onOpen: (s: Selection, pin?: boolean) => void;
  /** The whole panel, not a pane: the empty note sits lower. */
  roomy: boolean;
}) {
  const nav = useListNav({ activeKey });
  const items = match ? (loaded?.filter(match) ?? null) : loaded;
  return (
    <>
      <ListError error={error} loaded={!!loaded} />
      {match && !!loaded?.length && items?.length === 0 && <EmptyNote roomy={roomy}>None of the {loaded?.length} loaded issues match.</EmptyNote>}
      {!(match && loaded?.length) && items?.length === 0 && (
        <EmptyNote roomy={roomy}>
          {emptyText("issues", filter, narrow)}
          {isNarrowed(narrow) && <ClearNarrow onClick={onClearNarrow} />}
        </EmptyNote>
      )}
      <div role="listbox" aria-label="Issues" {...nav}>
      {items?.map((i) => {
        const sel: Selection = { kind: "issue", issue: i };
        const key = selectionKey(sel);
        const active = activeKey === key;
        // The keyboard's way to what the author and label chips do on a click.
        const extra = (
          <>
            <ContextMenuItem disabled={narrow.author === i.author} onSelect={() => onAuthor(i.author)}>
              <ListFilter /> Filter by author {i.author}
            </ContextMenuItem>
            {i.labels.map((l) => (
              <ContextMenuItem key={l.name} disabled={narrow.labels.includes(l.name)} onSelect={() => onLabel(l)}>
                <ListFilter /> Filter by label {l.name}
              </ContextMenuItem>
            ))}
            <ContextMenuSeparator />
          </>
        );
        return (
          <LinkMenu key={i.number} url={i.url} extra={extra}>
          <div
            role="option"
            aria-selected={active}
            tabIndex={-1}
            data-row={key}
            onClick={() => onOpen(sel)}
            onDoubleClick={() => onOpen(sel, true)}
            className={cn("relative flex cursor-pointer gap-2.5 py-1.5 pr-2 pl-3 outline-none focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset", active ? "bg-primary/15" : "hover:bg-hover focus:bg-hover")}
          >
            {active && <span className="absolute inset-y-0 left-0 w-0.5 bg-primary" />}
            <IssueStateIcon issue={i} className="mt-0.5" />
            <div className="min-w-0 flex-1">
              <div className="truncate text-[12px] leading-4 text-foreground/90" title={i.title}>
                {i.title}
              </div>
              {i.labels.length > 0 && (
                <div className="mt-1 flex flex-wrap gap-1">
                  {i.labels.map((l) => (
                    <LabelChip key={l.name} label={l} onClick={() => onLabel(l)} />
                  ))}
                </div>
              )}
              <div className="mt-0.5 flex items-center gap-1.5 text-[10.5px] text-subtle">
                <span className="font-mono">#{i.number}</span>
                <span>·</span>
                <RowFilter title={`Filter by author ${i.author}`} onFilter={() => onAuthor(i.author)}>
                  {i.author}
                </RowFilter>
                {i.comments > 0 && (
                  <span className="flex shrink-0 items-center gap-0.5">
                    <MessageSquare className="size-2.5" />
                    {i.comments}
                  </span>
                )}
                <RelativeTime date={i.updatedAt} className="ml-auto shrink-0" />
              </div>
            </div>
          </div>
          </LinkMenu>
        );
      })}
      </div>
    </>
  );
}
