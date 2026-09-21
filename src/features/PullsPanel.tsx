import { GitMerge, GitPullRequest, GitPullRequestClosed, GitPullRequestDraft, Plus, RefreshCw, Terminal } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Tip } from "@/components/ui/tooltip";
import { api, type Branch, errorMessage, type GitHubAccount, github, isNotConnected, type Pull, type RepoStatus } from "@/lib/api";
import { invalidate, useGitHubData } from "@/lib/githubCache";
import { type Selection, selectionKey } from "@/lib/selection";
import { toast } from "@/lib/toast";
import { cn, relativeTime } from "@/lib/utils";

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
  lastSubject: string | null;
  activeKey: string | null;
  onOpen: (s: Selection, pin?: boolean) => void;
  refreshRepo: () => Promise<void>;
}

export function PullsPanel({ status, branches, lastSubject, activeKey, onOpen, refreshRepo }: Props) {
  const [filter, setFilter] = useState<Filter>("open");
  const [creating, setCreating] = useState(false);
  // The account only changes with a new sign-in: rechecked every 10 minutes and on every
  // manual refresh or retry (a 304 when nothing changed, so free).
  const acct = useGitHubData("account", github.account, 600_000);
  const list = useGitHubData(`pulls:${filter}`, useCallback(() => github.list(filter), [filter]));
  const account = acct.data ?? null;
  const pulls = list.data ?? null;
  const failure = acct.error ?? list.error;
  const error = failure === undefined ? null : errorMessage(failure);
  const loading = acct.loading || list.loading;

  const { refresh: refreshAccount } = acct;
  const { refresh: refreshList } = list;
  const load = useCallback(() => {
    refreshAccount(true);
    refreshList(true);
  }, [refreshAccount, refreshList]);
  useEffect(() => {
    listeners.add(load);
    return () => {
      listeners.delete(load);
    };
  }, [load]);

  if (isNotConnected(failure)) return <ConnectGitHub onRetry={load} />;

  const currentPull = status?.branch ? pulls?.find((p) => p.headRef === status.branch && p.state === "open") : undefined;

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
          <Tip label={currentPull ? `#${currentPull.number} already open for this branch` : "New pull request"}>
            <Button variant="secondary" size="sm" disabled={!account || !status?.branch || !!currentPull} onClick={() => setCreating(true)}>
              <Plus /> New
            </Button>
          </Tip>
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto py-1">
        {/* With a cached list on screen, a failed refresh is a note above it, not a blank panel. */}
        {error && !pulls && <div className="px-4 py-6 text-center text-[12px] text-muted-foreground">{error}</div>}
        {error && pulls && <div className="mx-2 mb-1 rounded-sm bg-removed/10 px-2 py-1.5 text-[11.5px] text-removed">{error}</div>}
        {pulls?.length === 0 && <div className="px-4 pt-16 text-center text-[12px] text-subtle">No {filter === "all" ? "" : filter} pull requests.</div>}
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
      </div>
      {account && (
        <div className="shrink-0 border-t border-border px-3 py-1.5 text-[10.5px] text-subtle">
          Signed in as <span className="text-muted-foreground">{account.login}</span> via {account.source === "gh" ? "GitHub CLI" : "git credentials"}
        </div>
      )}
      {creating && account && status?.branch && (
        <CreatePullDialog
          account={account}
          status={status}
          branches={branches}
          defaultTitle={lastSubject ?? status.branch}
          onClose={() => setCreating(false)}
          onCreated={async (p) => {
            setCreating(false);
            // The dialog's notifyPullsChanged reloads the list.
            await refreshRepo();
            onOpen({ kind: "pull", pull: p }, true);
          }}
        />
      )}
    </div>
  );
}

/** No token from the GitHub CLI or git's credential store: explain the two ways to connect. */
function ConnectGitHub({ onRetry }: { onRetry: () => void }) {
  return (
    <div className="px-5 pt-14 text-center">
      <GitPullRequest className="mx-auto size-7 text-subtle" />
      <div className="mt-3 text-[13px] font-medium">Connect GitHub to see pull requests</div>
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
  account,
  status,
  branches,
  defaultTitle,
  onClose,
  onCreated,
}: {
  account: GitHubAccount;
  status: RepoStatus;
  branches: Branch[];
  defaultTitle: string;
  onClose: () => void;
  onCreated: (p: Pull) => void;
}) {
  const head = status.branch!;
  // The repo's default branch is always offered (and preselected), even if not fetched locally.
  const bases = [...new Set([account.defaultBranch, ...branches.filter((b) => b.remote && b.name.startsWith("origin/")).map((b) => b.name.slice(7))])].filter(
    (b): b is string => !!b && b !== head && b !== "HEAD",
  );
  const [title, setTitle] = useState(defaultTitle);
  const [body, setBody] = useState("");
  const [base, setBase] = useState(bases[0] ?? "main");
  const [draft, setDraft] = useState(false);
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    setBusy(true);
    try {
      // GitHub can only open a PR from a branch it has; publish or push the latest first.
      if (!status.upstream || status.ahead > 0) await api.push();
      onCreated(await github.create(title.trim(), body, head, base, draft));
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
          <span className="font-mono">{head}</span> → <span className="font-mono">{base}</span>
          {(!status.upstream || status.ahead > 0) && " · the branch will be pushed first"}
        </DialogDescription>
        <form
          className="mt-4 space-y-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (title.trim()) submit();
          }}
        >
          <Input autoFocus value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Title" />
          <Textarea value={body} onChange={(e) => setBody(e.target.value)} placeholder="Description (markdown)" rows={6} />
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
            <label className="ml-auto flex items-center gap-1.5 text-muted-foreground">
              <input type="checkbox" checked={draft} onChange={(e) => setDraft(e.target.checked)} className="accent-primary" /> Draft
            </label>
            <Button type="submit" disabled={busy || !title.trim()}>
              {busy ? "Creating…" : "Create"}
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  );
}
