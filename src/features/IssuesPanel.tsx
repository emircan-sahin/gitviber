import { CircleCheck, CircleDot, CircleSlash, MessageSquare, Plus, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Tip } from "@/components/ui/tooltip";
import { errorMessage, fullName, github, type Issue, type IssueLabel, isNotConnected, issues, type Target } from "@/lib/api";
import { invalidate, useGitHubData } from "@/lib/githubCache";
import { type Selection, selectionKey } from "@/lib/selection";
import { toast } from "@/lib/toast";
import { cn, relativeTime } from "@/lib/utils";
import { ConnectGitHub, isoToUnix, NewButton } from "./PullsPanel";
import { RepoPanes } from "./RepoPanes";

type Filter = "open" | "closed" | "all";

// Issue views elsewhere (close, edit, comment) tell the list to reload.
const listeners = new Set<() => void>();
export const notifyIssuesChanged = () => {
  invalidate("issues:");
  listeners.forEach((l) => l());
};

export function IssueStateIcon({ issue, className }: { issue: Pick<Issue, "state" | "stateReason">; className?: string }) {
  if (issue.state === "open") return <CircleDot className={cn("size-3.5 shrink-0 text-added", className)} />;
  if (issue.stateReason === "not_planned") return <CircleSlash className={cn("size-3.5 shrink-0 text-subtle", className)} />;
  return <CircleCheck className={cn("size-3.5 shrink-0 text-renamed", className)} />;
}

/** A label's color is whatever its author typed; only a plain hex reaches the style. */
export function LabelChip({ label }: { label: IssueLabel }) {
  const color = /^[0-9a-f]{6}$/i.test(label.color) ? `#${label.color}` : undefined;
  return (
    <span className="inline-flex max-w-40 items-center gap-1 rounded-full border border-border px-1.5 text-[10.5px] leading-4 text-muted-foreground">
      <span className="size-1.5 shrink-0 rounded-full bg-subtle" style={color ? { backgroundColor: color } : undefined} />
      <span className="truncate">{label.name}</span>
    </span>
  );
}

export function IssuesPanel({ activeKey, onOpen }: { activeKey: string | null; onOpen: (s: Selection, pin?: boolean) => void }) {
  const [filter, setFilter] = useState<Filter>("open");
  // The repository the new issue goes to: origin (null), or a fork's parent.
  const [creating, setCreating] = useState<{ target: Target } | null>(null);
  // Same cache entry as the PRs panel's.
  const acct = useGitHubData("account", github.account, 600_000);
  const account = acct.data ?? null;
  const origin = account?.origin ?? null;
  const parent = account?.parent ?? null;
  const upstream = parent ? fullName(parent.repo) : null;
  // Forks start with issues off: nothing is listed where they are.
  const own = useGitHubData(`issues:origin:${filter}`, useCallback(() => issues.list(null, filter), [filter]));
  const up = useGitHubData(upstream && parent?.issues ? `issues:${upstream}:${filter}` : null, useCallback(() => issues.list(upstream, filter), [upstream, filter]));
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

  if (isNotConnected(failure)) return <ConnectGitHub onRetry={load} subject="issues" />;

  const ownRows = (roomy: boolean) => <IssueRows items={own.data ?? null} error={error} filter={filter} activeKey={activeKey} onOpen={onOpen} roomy={roomy} />;

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
          {!parent && (
            <Tip label="New issue">
              <Button variant="secondary" size="sm" disabled={!origin} onClick={() => setCreating({ target: null })}>
                <Plus /> New
              </Button>
            </Tip>
          )}
        </div>
      </div>
      {parent && upstream ? (
        <div className="min-h-0 flex-1">
          <RepoPanes
            id="issues"
            panes={[
              {
                id: "origin",
                title: "Your fork",
                detail: origin ? fullName(origin.repo) : "",
                actions: origin?.issues && <NewButton label="New issue" onClick={() => setCreating({ target: null })} />,
                children: origin?.issues ? ownRows(false) : <IssuesOff repo={origin ? fullName(origin.repo) : "your fork"} />,
              },
              {
                id: "parent",
                title: "Original",
                detail: upstream,
                actions: parent.issues && <NewButton label="New issue" onClick={() => setCreating({ target: upstream })} />,
                children: parent.issues ? (
                  <IssueRows
                    items={up.data ?? null}
                    error={up.error === undefined ? null : errorMessage(up.error)}
                    filter={filter}
                    activeKey={activeKey}
                    onOpen={onOpen}
                    roomy={false}
                  />
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
      {account && (
        <div className="shrink-0 border-t border-border px-3 py-1.5 text-[10.5px] text-subtle">
          Signed in as <span className="text-muted-foreground">{account.login}</span> via {account.source === "gh" ? "GitHub CLI" : "git credentials"}
        </div>
      )}
      {creating && (
        <CreateIssueDialog
          target={creating.target}
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
  items,
  error,
  filter,
  activeKey,
  onOpen,
  roomy,
}: {
  items: Issue[] | null;
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
      {error && !items && <div className="px-4 py-6 text-center text-[12px] text-muted-foreground">{error}</div>}
      {error && items && <div className="mx-2 mb-1 rounded-sm bg-removed/10 px-2 py-1.5 text-[11.5px] text-removed">{error}</div>}
      {items?.length === 0 && <div className={cn("px-4 text-center text-[12px] text-subtle", roomy ? "pt-16" : "py-3")}>No {filter === "all" ? "" : filter} issues.</div>}
      {items?.map((i) => {
        const sel: Selection = { kind: "issue", issue: i };
        const active = activeKey === selectionKey(sel);
        return (
          <div
            key={i.number}
            role="button"
            onClick={() => onOpen(sel)}
            onDoubleClick={() => onOpen(sel, true)}
            className={cn("relative flex cursor-pointer gap-2.5 py-1.5 pr-2 pl-3", active ? "bg-primary/15" : "hover:bg-hover")}
          >
            {active && <span className="absolute inset-y-0 left-0 w-0.5 bg-primary" />}
            <IssueStateIcon issue={i} className="mt-0.5" />
            <div className="min-w-0 flex-1">
              <div className="truncate text-[12px] leading-4 text-foreground/90">{i.title}</div>
              {i.labels.length > 0 && (
                <div className="mt-1 flex flex-wrap gap-1">
                  {i.labels.map((l) => (
                    <LabelChip key={l.name} label={l} />
                  ))}
                </div>
              )}
              <div className="mt-0.5 flex items-center gap-1.5 text-[10.5px] text-subtle">
                <span className="font-mono">#{i.number}</span>
                <span>·</span>
                <span className="truncate">{i.author}</span>
                {i.comments > 0 && (
                  <span className="flex shrink-0 items-center gap-0.5">
                    <MessageSquare className="size-2.5" />
                    {i.comments}
                  </span>
                )}
                <span className="ml-auto shrink-0">{relativeTime(isoToUnix(i.updatedAt))}</span>
              </div>
            </div>
          </div>
        );
      })}
    </>
  );
}

function CreateIssueDialog({ target, onClose, onCreated }: { target: Target; onClose: () => void; onCreated: (i: Issue) => void }) {
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    setBusy(true);
    try {
      onCreated(await issues.create(target, title.trim(), body));
      toast("success", "Issue created");
      notifyIssuesChanged();
    } catch (e) {
      toast("error", "Could not create issue", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-w-lg">
        <DialogTitle>New issue</DialogTitle>
        <DialogDescription>{target ? `Opened on ${target}.` : "Opened on the repository's GitHub page."}</DialogDescription>
        <form
          className="mt-4 space-y-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (title.trim()) submit();
          }}
        >
          <Input autoFocus value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Title" />
          <Textarea value={body} onChange={(e) => setBody(e.target.value)} placeholder="Description (markdown)" rows={8} />
          <div className="flex justify-end">
            <Button type="submit" disabled={busy || !title.trim()}>
              {busy ? "Creating…" : "Create"}
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  );
}
