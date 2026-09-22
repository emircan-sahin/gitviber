import { Check, ChevronDown, CircleCheck, CircleDot, CircleSlash, MessageSquare, Plus, RefreshCw, Search, Tag, X } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
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
function LabelDot({ label, className }: { label: Pick<IssueLabel, "color">; className?: string }) {
  const color = /^[0-9a-f]{6}$/i.test(label.color) ? `#${label.color}` : undefined;
  return <span className={cn("size-1.5 shrink-0 rounded-full bg-subtle", className)} style={color ? { backgroundColor: color } : undefined} />;
}

/** With `onClick`, a button: the issue list filters by the label. */
export function LabelChip({ label, onClick }: { label: IssueLabel; onClick?: () => void }) {
  const El = onClick ? "button" : "span";
  return (
    <El
      {...(onClick && {
        title: "Filter by this label",
        onClick: (e: React.MouseEvent) => {
          e.stopPropagation();
          onClick();
        },
        // The row opens and pins its issue on a double-click.
        onDoubleClick: (e: React.MouseEvent) => e.stopPropagation(),
      })}
      className={cn(
        "inline-flex max-w-40 items-center gap-1 rounded-full border border-border px-1.5 text-[10.5px] leading-4 text-muted-foreground",
        onClick && "hover:border-border-strong hover:text-foreground",
      )}
    >
      <LabelDot label={label} />
      <span className="truncate">{label.name}</span>
    </El>
  );
}

export function IssuesPanel({ activeKey, onOpen }: { activeKey: string | null; onOpen: (s: Selection, pin?: boolean) => void }) {
  const [filter, setFilter] = useState<Filter>("open");
  const [labels, setLabels] = useState<IssueLabel[]>([]);
  const addLabel = (label: IssueLabel) => setLabels((l) => (l.some((m) => m.name === label.name) ? l : [...l, label]));
  // The repository the new issue goes to: origin (null), or a fork's parent.
  const [creating, setCreating] = useState<{ target: Target } | null>(null);
  // Same cache entry as the PRs panel's.
  const acct = useGitHubData("account", github.account, 600_000);
  const account = acct.data ?? null;
  const origin = account?.origin ?? null;
  const parent = account?.parent ?? null;
  const upstream = parent ? fullName(parent.repo) : null;
  // Forks start with issues off: nothing is listed where they are.
  const query = `${filter}:${JSON.stringify(labels.map((l) => l.name))}`;
  const own = useGitHubData(`issues:origin:${query}`, useCallback(() => issues.list(null, filter, labels.map((l) => l.name)), [filter, labels]));
  const up = useGitHubData(
    upstream && parent?.issues ? `issues:${upstream}:${query}` : null,
    useCallback(() => issues.list(upstream, filter, labels.map((l) => l.name)), [upstream, filter, labels]),
  );
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

  const rowProps = { filter, labels, onLabel: addLabel, onClearLabels: () => setLabels([]), activeKey, onOpen };
  const ownRows = (roomy: boolean) => <IssueRows items={own.data ?? null} error={error} roomy={roomy} {...rowProps} />;

  return (
    <div className="flex h-full flex-col">
      {/* A container: at the panel's narrowest the Label and New buttons drop their words. */}
      <div className="@container flex h-8 shrink-0 items-center gap-1 border-b border-border px-2">
        {(["open", "closed", "all"] as const).map((f) => (
          <button
            key={f}
            onClick={() => setFilter(f)}
            className={cn("h-5 rounded-sm px-1.5 text-[11.5px] capitalize", filter === f ? "bg-active text-foreground" : "text-subtle hover:text-foreground")}
          >
            {f}
          </button>
        ))}
        <LabelFilter upstream={parent?.issues ? upstream : null} selected={labels} onChange={setLabels} />
        <div className="ml-auto flex items-center gap-0.5">
          <Tip label="Refresh">
            <Button variant="ghost" size="icon-sm" onClick={load} disabled={loading}>
              <RefreshCw className={cn(loading && "animate-spin")} />
            </Button>
          </Tip>
          {!parent && (
            <Tip label="New issue">
              <Button variant="secondary" size="sm" disabled={!origin} onClick={() => setCreating({ target: null })}>
                <Plus /> <span className="@max-[300px]:hidden">New</span>
              </Button>
            </Tip>
          )}
        </div>
      </div>
      {labels.length > 0 && (
        <div className="flex shrink-0 flex-wrap items-center gap-1 border-b border-border px-2 py-1.5">
          {labels.map((l) => (
            <span key={l.name} className="inline-flex max-w-48 items-center gap-1 rounded-full border border-border-strong bg-active pr-0.5 pl-1.5 text-[10.5px] leading-4">
              <LabelDot label={l} />
              <span className="truncate">{l.name}</span>
              <button
                aria-label={`Remove ${l.name}`}
                onClick={() => setLabels((ls) => ls.filter((m) => m.name !== l.name))}
                className="flex size-3.5 shrink-0 items-center justify-center rounded-full text-subtle hover:bg-hover hover:text-foreground"
              >
                <X className="size-2.5" />
              </button>
            </span>
          ))}
          {labels.length > 1 && (
            <button onClick={() => setLabels([])} className="ml-auto px-1 text-[10.5px] text-subtle hover:text-foreground">
              Clear
            </button>
          )}
        </div>
      )}
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
  labels,
  onLabel,
  onClearLabels,
  activeKey,
  onOpen,
  roomy,
}: {
  items: Issue[] | null;
  error: string | null;
  filter: Filter;
  labels: IssueLabel[];
  onLabel: (label: IssueLabel) => void;
  onClearLabels: () => void;
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
      {items?.length === 0 && (
        <div className={cn("px-4 text-center text-[12px] text-subtle", roomy ? "pt-16" : "py-3")}>
          No {filter === "all" ? "" : filter} issues{labels.length > 0 && (labels.length === 1 ? " with this label" : " with all these labels")}.
          {labels.length > 0 && (
            <button onClick={onClearLabels} className="ml-1 font-medium text-primary hover:underline">
              Clear labels
            </button>
          )}
        </div>
      )}
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
                    <LabelChip key={l.name} label={l} onClick={() => onLabel(l)} />
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

/**
 * GitHub's label filter: any number of labels, and the list keeps issues carrying all of them.
 * Lists every label the repository defines (both, for a fork), loaded when first opened.
 */
function LabelFilter({ upstream, selected, onChange }: { upstream: string | null; selected: IssueLabel[]; onChange: (labels: IssueLabel[]) => void }) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  // Selected labels go first, as of opening: reordering under the pointer would move the row just clicked.
  const [first, setFirst] = useState<IssueLabel[]>([]);
  const listRef = useRef<HTMLDivElement>(null);
  const own = useGitHubData(open ? "issues:labels:origin" : null, useCallback(() => issues.labels(null), []), 600_000);
  const up = useGitHubData(open && upstream ? `issues:labels:${upstream}` : null, useCallback(() => issues.labels(upstream), [upstream]), 600_000);
  const failure = own.error ?? up.error;

  const defined = [...(own.data ?? []), ...(up.data ?? [])].filter((l, i, all) => all.findIndex((m) => m.name === l.name) === i);
  const isFirst = (l: IssueLabel) => first.some((m) => m.name === l.name);
  // One renamed or deleted since it was picked still shows, so it can be taken off.
  const gone = first.filter((l) => !defined.some((m) => m.name === l.name));
  const q = query.trim().toLowerCase();
  const shown = [...gone, ...defined]
    .sort((a, b) => Number(isFirst(b)) - Number(isFirst(a)))
    .filter((l) => !q || l.name.toLowerCase().includes(q) || l.description.toLowerCase().includes(q));
  const loaded = own.data !== undefined && (!upstream || up.data !== undefined);

  const isOn = (l: IssueLabel) => selected.some((m) => m.name === l.name);
  const toggle = (l: IssueLabel) => onChange(isOn(l) ? selected.filter((m) => m.name !== l.name) : [...selected, l]);
  const move = (i: number) => {
    setIndex(i);
    listRef.current?.children[i]?.scrollIntoView({ block: "nearest" });
  };

  return (
    <Popover
      open={open}
      onOpenChange={(o) => {
        setOpen(o);
        if (o) {
          setFirst(selected);
          setQuery("");
          setIndex(0);
        }
      }}
    >
      <PopoverTrigger asChild>
        {/* Compact: the chosen labels show in a row of their own under the header. */}
        <button
          className={cn(
            "flex h-5 shrink-0 items-center gap-1 rounded-sm px-1.5 text-[11.5px] @max-[300px]:px-1",
            selected.length ? "bg-active text-foreground" : "text-subtle hover:text-foreground data-[state=open]:text-foreground",
          )}
        >
          <Tag className="size-3" />
          <span className="@max-[300px]:hidden">Label</span>
          {selected.length > 0 ? (
            <span className="rounded-full bg-primary/20 px-1 text-[10px] leading-3.5 font-medium text-primary">{selected.length}</span>
          ) : (
            <ChevronDown className="size-3 opacity-70 @max-[300px]:hidden" />
          )}
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" className="flex w-72 flex-col overflow-hidden">
        <div className="flex h-9 shrink-0 items-center gap-2 border-b border-border px-2.5">
          <Search className="size-3.5 shrink-0 text-subtle" />
          <input
            autoFocus
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setIndex(0);
            }}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown" || e.key === "ArrowUp") {
                e.preventDefault();
                if (shown.length) move((index + (e.key === "ArrowDown" ? 1 : shown.length - 1)) % shown.length);
              } else if (e.key === "Enter" && shown[index]) {
                e.preventDefault();
                toggle(shown[index]);
              }
            }}
            placeholder="Filter labels…"
            className="h-full min-w-0 flex-1 bg-transparent text-[12px] outline-none placeholder:text-subtle"
          />
        </div>
        <div ref={listRef} className="max-h-80 min-h-0 flex-1 overflow-x-hidden overflow-y-auto p-1">
          {failure !== undefined && !loaded ? (
            <div className="px-2 py-3 text-center text-[12px] text-muted-foreground">{errorMessage(failure)}</div>
          ) : !loaded && shown.length === 0 ? (
            <div className="px-2 py-3 text-center text-[12px] text-subtle">Loading labels…</div>
          ) : shown.length === 0 ? (
            <div className="px-2 py-3 text-center text-[12px] text-subtle">{q ? "No labels match" : "This repository has no labels"}</div>
          ) : (
            shown.map((l, i) => {
              const on = isOn(l);
              return (
                <button
                  key={l.name}
                  // Keep the focus in the search box.
                  onMouseDown={(e) => e.preventDefault()}
                  onMouseEnter={() => setIndex(i)}
                  onClick={() => toggle(l)}
                  className={cn("flex w-full items-start gap-2 rounded-sm px-2 py-1.5 text-left", i === index && "bg-hover")}
                >
                  <Check className={cn("mt-0.5 size-3.5 shrink-0 text-primary", !on && "invisible")} />
                  <LabelDot label={l} className="mt-1 size-2.5" />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[12px] leading-4">{l.name}</span>
                    {l.description && <span className="block truncate text-[10.5px] leading-4 text-subtle">{l.description}</span>}
                  </span>
                </button>
              );
            })
          )}
        </div>
        <div className="flex shrink-0 items-center gap-2 border-t border-border px-2.5 py-1.5 text-[10.5px] text-subtle">
          <span className="min-w-0 flex-1 truncate">{selected.length > 1 ? "Issues with all of them" : "↑↓ navigate · ↵ select"}</span>
          {selected.length > 0 && (
            <button onClick={() => onChange([])} className="shrink-0 rounded-sm px-1.5 py-0.5 hover:bg-hover hover:text-foreground">
              Clear
            </button>
          )}
        </div>
      </PopoverContent>
    </Popover>
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
