import { GitMerge, GitPullRequest, GitPullRequestClosed, GitPullRequestDraft, Plus, RefreshCw, Terminal } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Tip } from "@/components/ui/tooltip";
import { api, type Branch, type Commit, errorMessage, fullName, type GitHubAccess, github, isNotConnected, type Pull, type RepoStatus } from "@/lib/api";
import { invalidate, useGitHubData } from "@/lib/githubCache";
import { type Selection, selectionKey } from "@/lib/selection";
import { toast } from "@/lib/toast";
import { cn, relativeTime } from "@/lib/utils";
import { RepoPanes } from "./RepoPanes";

type Filter = "open" | "closed" | "all";

// PR views elsewhere (merge, create) tell the list to reload.
const listeners = new Set<() => void>();
export const notifyPullsChanged = () => {
  invalidate("pulls:");
  listeners.forEach((l) => l());
};

export const isoToUnix = (iso: string) => Date.parse(iso) / 1000;

export function PullStateIcon({ pull, className }: { pull: Pick<Pull, "state" | "draft">; className?: string }) {
  if (pull.state === "merged") return <GitMerge className={cn("size-3.5 shrink-0 text-renamed", className)} />;
  if (pull.state === "closed") return <GitPullRequestClosed className={cn("size-3.5 shrink-0 text-removed", className)} />;
  if (pull.draft) return <GitPullRequestDraft className={cn("size-3.5 shrink-0 text-subtle", className)} />;
  return <GitPullRequest className={cn("size-3.5 shrink-0 text-added", className)} />;
}

interface Props {
  status: RepoStatus | null;
  branches: Branch[];
  /** HEAD: its subject titles a new PR, as GitHub does for a single commit. */
  lastCommit: Commit | null;
  activeKey: string | null;
  onOpen: (s: Selection, pin?: boolean) => void;
  refreshRepo: () => Promise<void>;
}

export function PullsPanel({ status, branches, lastCommit, activeKey, onOpen, refreshRepo }: Props) {
  const [filter, setFilter] = useState<Filter>("open");
  // The repository the new PR goes to: origin, or a fork's parent.
  const [creating, setCreating] = useState<GitHubAccess | null>(null);
  // The account only changes with a new sign-in: rechecked every 10 minutes and on every
  // manual refresh or retry (a 304 when nothing changed, so free).
  const acct = useGitHubData("account", github.account, 600_000);
  const account = acct.data ?? null;
  const upstream = account?.parent ? fullName(account.parent.repo) : null;
  // Origin's list loads alongside the account; a fork's parent is only known after it.
  const own = useGitHubData(`pulls:origin:${filter}`, useCallback(() => github.list(null, filter), [filter]));
  const up = useGitHubData(upstream && `pulls:${upstream}:${filter}`, useCallback(() => github.list(upstream, filter), [upstream, filter]));
  const failure = acct.error ?? own.error;
  const error = failure === undefined ? null : errorMessage(failure);
  const loading = acct.loading || own.loading || up.loading;

  const { refresh: refreshAccount } = acct;
  const { refresh: refreshOwn } = own;
  const { refresh: refreshUp } = up;
  const load = useCallback(() => {
    refreshAccount(true);
    refreshOwn(true);
    refreshUp(true);
  }, [refreshAccount, refreshOwn, refreshUp]);
  useEffect(() => {
    listeners.add(load);
    return () => {
      listeners.delete(load);
    };
  }, [load]);

  if (isNotConnected(failure)) return <ConnectGitHub onRetry={load} />;

  // This branch's open PR, in either repository. Another fork's same-named branch isn't it.
  const origin = account?.origin ?? null;
  const originName = origin ? fullName(origin.repo).toLowerCase() : null;
  const currentPull = status?.branch
    ? [...(own.data ?? []), ...(up.data ?? [])].find(
        (p) => p.headRef === status.branch && p.state === "open" && (!originName || p.headRepo?.toLowerCase() === originName),
      )
    : undefined;
  const newLabel = currentPull ? `#${currentPull.number} already open for this branch` : "New pull request";
  const canCreate = !!status?.branch && !currentPull;

  const ownRows = <PullRows pulls={own.data ?? null} error={error} filter={filter} activeKey={activeKey} onOpen={onOpen} roomy={!upstream} />;

  return (
    <div className="flex h-full flex-col">
      <div className="flex h-8 shrink-0 items-center gap-1 border-b border-border px-2">
        {(["open", "closed", "all"] as const).map((f) => (
          <button
            key={f}
            onClick={() => setFilter(f)}
            className={cn("h-5 rounded-sm px-1.5 text-[11.5px] capitalize", filter === f ? "bg-active text-foreground" : "text-subtle hover:text-foreground")}
          >
            {f}
          </button>
        ))}
        <div className="ml-auto flex items-center gap-0.5">
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
                  <Plus /> New
                </Button>
              </span>
            </Tip>
          )}
        </div>
      </div>
      {upstream && account?.parent ? (
        <div className="min-h-0 flex-1">
          <RepoPanes
            id="pulls"
            panes={[
              {
                id: "origin",
                title: "Your fork",
                detail: origin ? fullName(origin.repo) : "",
                actions: <NewButton label={newLabel} disabled={!origin || !canCreate} onClick={() => setCreating(origin)} />,
                children: ownRows,
              },
              {
                id: "parent",
                title: "Original",
                detail: upstream,
                actions: <NewButton label={newLabel} disabled={!canCreate} onClick={() => setCreating(account.parent)} />,
                children: (
                  <PullRows
                    pulls={up.data ?? null}
                    error={up.error === undefined ? null : errorMessage(up.error)}
                    filter={filter}
                    activeKey={activeKey}
                    onOpen={onOpen}
                    roomy={false}
                  />
                ),
              },
            ]}
          />
        </div>
      ) : (
        <div className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto py-1">{ownRows}</div>
      )}
      {account && (
        <div className="shrink-0 border-t border-border px-3 py-1.5 text-[10.5px] text-subtle">
          Signed in as <span className="text-muted-foreground">{account.login}</span> via {account.source === "gh" ? "GitHub CLI" : "git credentials"}
        </div>
      )}
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

/**
 * `git push` sends the branch somewhere other than origin, where a PR's branch must be. Out of
 * the box a branch pushes where it pulls from: a fork's dev tracking upstream/dev, into the original.
 */
const pushesElsewhere = (s: RepoStatus) => (s.push && s.push.remote !== "origin" ? s.push.remote : null);

/** GitHub's title for a PR of several commits: the branch name, spaced and capitalized. */
const branchTitle = (branch: string) => {
  const t = branch.replace(/[-_]+/g, " ");
  return t.charAt(0).toUpperCase() + t.slice(1);
};

/** A pane header's "new" action. */
export function NewButton({ label, disabled, onClick }: { label: string; disabled?: boolean; onClick: () => void }) {
  return (
    // A disabled button gets no hover, so the wrapper carries the tooltip that explains it.
    <Tip label={label}>
      <span>
        <Button variant="ghost" size="icon-sm" aria-label={label} disabled={disabled} onClick={onClick}>
          <Plus />
        </Button>
      </span>
    </Tip>
  );
}

function PullRows({
  pulls,
  error,
  filter,
  activeKey,
  onOpen,
  roomy,
}: {
  pulls: Pull[] | null;
  error: string | null;
  filter: Filter;
  activeKey: string | null;
  onOpen: (s: Selection, pin?: boolean) => void;
  /** The whole panel, not a pane: the empty note sits lower. */
  roomy: boolean;
}) {
  return (
    <>
      {/* With a cached list on screen, a failed refresh is a note above it, not a blank panel. */}
      {error && !pulls && <div className="px-4 py-6 text-center text-[12px] text-muted-foreground">{error}</div>}
      {error && pulls && <div className="mx-2 mb-1 rounded-sm bg-removed/10 px-2 py-1.5 text-[11.5px] text-removed">{error}</div>}
      {pulls?.length === 0 && (
        <div className={cn("px-4 text-center text-[12px] text-subtle", roomy ? "pt-16" : "py-3")}>No {filter === "all" ? "" : filter} pull requests.</div>
      )}
      {pulls?.map((p) => {
        const sel: Selection = { kind: "pull", pull: p };
        const active = activeKey === selectionKey(sel);
        return (
          <div
            key={p.number}
            role="button"
            onClick={() => onOpen(sel)}
            onDoubleClick={() => onOpen(sel, true)}
            className={cn("relative flex cursor-pointer gap-2.5 py-1.5 pr-2 pl-3", active ? "bg-primary/15" : "hover:bg-hover")}
          >
            {active && <span className="absolute inset-y-0 left-0 w-0.5 bg-primary" />}
            <PullStateIcon pull={p} className="mt-0.5" />
            <div className="min-w-0 flex-1">
              <div className="truncate text-[12px] leading-4 text-foreground/90">{p.title}</div>
              <div className="mt-0.5 flex items-center gap-1.5 text-[10.5px] text-subtle">
                <span className="font-mono">#{p.number}</span>
                <span>·</span>
                <span className="truncate">{p.author}</span>
                <span>·</span>
                <span className="min-w-0 truncate font-mono">{p.headRef}</span>
                <span className="ml-auto shrink-0">{relativeTime(isoToUnix(p.updatedAt))}</span>
              </div>
            </div>
          </div>
        );
      })}
      {/* github.rs `list` asks for one page of 100; say so rather than look complete. */}
      {pulls?.length === PR_LIST_LIMIT && (
        <div className="px-4 py-2 text-center text-[11px] text-subtle">Showing the {PR_LIST_LIMIT} most recently updated</div>
      )}
    </>
  );
}

const PR_LIST_LIMIT = 100;

/** No token from the GitHub CLI or git's credential store: explain the two ways to connect. */
export function ConnectGitHub({ onRetry, subject = "pull requests" }: { onRetry: () => void; subject?: string }) {
  return (
    <div className="px-5 pt-14 text-center">
      <GitPullRequest className="mx-auto size-7 text-subtle" />
      <div className="mt-3 text-[13px] font-medium">Connect GitHub to see {subject}</div>
      <div className="mt-1 text-[12px] leading-relaxed text-muted-foreground">
        GitViber uses the login you already have. It never stores a token itself.
      </div>
      <div className="mt-5 space-y-2 text-left">
        <Step n={1} title="With the GitHub CLI (recommended)">
          <code className="block rounded-sm bg-background px-2 py-1 font-mono text-[11.5px]">brew install gh && gh auth login</code>
        </Step>
        <Step n={2} title="Or push once over HTTPS">
          <span className="text-[11.5px] text-muted-foreground">If git has stored your github.com login (Keychain, GitHub Desktop, Git Credential Manager), it's picked up automatically.</span>
        </Step>
      </div>
      <Button variant="secondary" size="sm" className="mt-5" onClick={onRetry}>
        <Terminal /> I've signed in — retry
      </Button>
    </div>
  );
}

function Step({ n, title, children }: { n: number; title: string; children: React.ReactNode }) {
  return (
    <div className="rounded-md border border-border bg-panel p-2.5">
      <div className="mb-1.5 flex items-center gap-2 text-[12px] font-medium">
        <span className="flex size-4 items-center justify-center rounded-sm bg-elevated font-mono text-[10px]">{n}</span>
        {title}
      </div>
      {children}
    </div>
  );
}

function CreatePullDialog({
  target,
  origin,
  status,
  branches,
  defaultTitle,
  onClose,
  onCreated,
  onPushChanged,
}: {
  /** Where the PR goes: origin, or a fork's parent. */
  target: GitHubAccess;
  origin: GitHubAccess;
  status: RepoStatus;
  branches: Branch[];
  defaultTitle: string;
  onClose: () => void;
  onCreated: (p: Pull) => void;
  /** After push settings change: re-read the status. */
  onPushChanged: () => Promise<void>;
}) {
  const head = status.branch!;
  const upstream = fullName(target.repo) !== fullName(origin.repo);
  // The repo's default branch is always offered (and preselected), even if not fetched locally.
  // origin/* says nothing about the parent's branches: upstream, only its default is offered.
  const remote = upstream ? [] : branches.filter((b) => b.remote && b.name.startsWith("origin/")).map((b) => b.name.slice(7));
  const bases = [...new Set([target.defaultBranch, ...remote])].filter((b): b is string => !!b && (upstream || b !== head) && b !== "HEAD");
  const [title, setTitle] = useState(defaultTitle);
  const [body, setBody] = useState("");
  // Once typed in, the fields are the user's; the draft below stops filling them.
  const [typed, setTyped] = useState({ title: false, body: false });
  const [base, setBase] = useState(bases[0] ?? "main");
  const [draft, setDraft] = useState(false);
  const [maintainerEdits, setMaintainerEdits] = useState(true);
  const [busy, setBusy] = useState(false);

  // GitHub's defaults: one commit titles the PR with its subject and fills the body; more
  // take the branch name. Counted against the chosen base in the repository the PR goes to.
  useEffect(() => {
    let live = true;
    (async () => {
      const remote = upstream ? await github.originalRemote(fullName(target.repo), false) : "origin";
      if (!remote) return;
      const d = await api.pullDraft(`refs/remotes/${remote}/${base}`);
      if (!live) return;
      const one = d.commits === 1 && d.subject;
      if (!typed.title) setTitle(one ? d.subject! : branchTitle(head));
      if (!typed.body) setBody(one ? (d.body ?? "") : "");
    })().catch(() => {});
    return () => {
      live = false;
    };
    // Recounted per base; typing doesn't recount.
  }, [base, head, upstream, target.repo.owner, target.repo.name]);
  const elsewhere = pushesElsewhere(status);
  // Pushed already, and nothing new since: GitHub has the branch as it is.
  const needsPush = !status.push?.branch || status.push.ahead > 0;
  const [fixing, setFixing] = useState(false);
  const pushToOrigin = async () => {
    setFixing(true);
    try {
      await api.setPushDefault("origin");
      await onPushChanged();
    } catch (e) {
      toast("error", "Could not change where branches push", errorMessage(e));
    } finally {
      setFixing(false);
    }
  };

  const submit = async () => {
    setBusy(true);
    try {
      // GitHub can only open a PR from a branch it has; publish or push the latest first.
      if (needsPush) await api.push();
      onCreated(await github.create(upstream ? fullName(target.repo) : null, title.trim(), body, head, base, draft, maintainerEdits));
      toast("success", "Pull request created");
      notifyPullsChanged();
    } catch (e) {
      toast("error", "Could not create pull request", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-w-lg">
        <DialogTitle>New pull request</DialogTitle>
        <DialogDescription>
          <span className="font-mono">{upstream ? `${origin.repo.owner}:${head}` : head}</span> →{" "}
          <span className="font-mono">{upstream ? `${fullName(target.repo)}:${base}` : base}</span>
          {!elsewhere && needsPush && " · the branch will be pushed to origin first"}
        </DialogDescription>
        <form
          className="mt-4 space-y-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (title.trim() && !elsewhere) submit();
          }}
        >
          {elsewhere && (
            <div className="flex items-center gap-2 rounded-sm bg-removed/10 px-2 py-1.5 text-[11.5px] text-removed">
              <span className="min-w-0 flex-1">
                {head} pushes to <span className="font-mono">{elsewhere}</span>, but a pull request needs it on origin. Pulling can stay with {elsewhere}; only pushes
                move.
              </span>
              <Button type="button" size="sm" variant="secondary" className="shrink-0" disabled={fixing} onClick={pushToOrigin}>
                Push to origin from now on
              </Button>
            </div>
          )}
          <Input
            autoFocus
            value={title}
            onChange={(e) => {
              setTitle(e.target.value);
              setTyped((t) => ({ ...t, title: true }));
            }} placeholder="Title" />
          <Textarea
            value={body}
            onChange={(e) => {
              setBody(e.target.value);
              setTyped((t) => ({ ...t, body: true }));
            }} placeholder="Description (markdown)" rows={6} />
          <div className="flex items-center gap-2 text-[12px]">
            <span className="text-muted-foreground">Base</span>
            <select
              value={base}
              onChange={(e) => setBase(e.target.value)}
              className="h-7 rounded-md border border-border-strong bg-background px-2 font-mono text-[11.5px] outline-none focus:border-primary"
            >
              {(bases.length ? bases : [base]).map((b) => (
                <option key={b}>{b}</option>
              ))}
            </select>
            {upstream && (
              <Tip label="Lets the original's maintainers push to your branch, as GitHub offers">
                <label className="ml-auto flex items-center gap-1.5 text-muted-foreground">
                  <input type="checkbox" checked={maintainerEdits} onChange={(e) => setMaintainerEdits(e.target.checked)} className="accent-primary" /> Maintainer edits
                </label>
              </Tip>
            )}
            <label className={cn("flex items-center gap-1.5 text-muted-foreground", !upstream && "ml-auto")}>
              <input type="checkbox" checked={draft} onChange={(e) => setDraft(e.target.checked)} className="accent-primary" /> Draft
            </label>
            <Button type="submit" disabled={busy || !title.trim() || !!elsewhere}>
              {busy ? "Creating…" : "Create"}
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  );
}
