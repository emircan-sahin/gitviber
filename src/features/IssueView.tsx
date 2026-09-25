import { ask } from "@tauri-apps/plugin-dialog";
import { ChevronDown, CircleCheck, CircleDot, CircleSlash, ExternalLink, Loader2, Pencil, RefreshCw, Tag, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { PageFind } from "@/components/FindBox";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { accessFor, type CloseReason, errorMessage, github, type Issue, type IssueLabel, issues, repoOf } from "@/lib/api";
import { listIsBehind, useGitHubData } from "@/lib/githubCache";
import { matchesCommand } from "@/lib/keybindings";
import { toast } from "@/lib/toast";
import { cn, relativeTime } from "@/lib/utils";
import { IssueStateIcon, LabelChip, LabelPicker, notifyIssuesChanged } from "./IssuesPanel";
import { CopyLinkButton, isoToUnix, openOnGitHub } from "./PullsPanel";
import { PullMarkdown, Section } from "./PullView";
import { MarkdownInput } from "./MarkdownInput";

const CLOSE: Record<CloseReason, { label: string; note: string }> = {
  completed: { label: "Close as completed", note: "Done, closed, fixed, resolved" },
  not_planned: { label: "Close as not planned", note: "Won't fix, can't repro, duplicate, stale" },
};

export function IssueView({ issue, onDeleted }: { issue: Issue; onDeleted: () => void }) {
  const [busy, setBusy] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);
  const [comment, setComment] = useState("");
  // The labels being picked, shown in place of the issue's until they're saved.
  const [labels, setLabels] = useState<IssueLabel[] | null>(null);
  // The repository the issue is in: origin, or a fork's parent.
  const target = repoOf(issue.url);
  const detail = useGitHubData(
    `issue:${issue.url}`,
    useCallback(() => issues.detail(target, issue.number), [target, issue.number]),
  );
  const d = detail.data ?? null;
  const error = detail.error === undefined ? null : errorMessage(detail.error);
  // Closed by a push or edited on GitHub: the list still shows it as it was.
  useEffect(() => {
    if (d && listIsBehind("issues:", d)) notifyIssuesChanged();
  }, [d]);
  // And the other way: the tab took a newer copy from a list refresh than this page shows.
  const { refresh } = detail;
  useEffect(() => {
    if (d && issue.updatedAt > d.updatedAt) refresh(true);
  }, [issue.updatedAt]);
  // Same cache entry as the Issues panel's, so this is normally already loaded.
  const account = useGitHubData("account", github.account, 600_000).data ?? null;
  const load = () => detail.refresh(true);

  /** Runs one write; returns whether it went through. */
  const act = async (label: string, fn: () => Promise<unknown>, done: string) => {
    setBusy(label);
    try {
      await fn();
      toast("success", done);
      notifyIssuesChanged();
      await load();
      return true;
    } catch (e) {
      toast("error", `${label} failed`, errorMessage(e));
      return false;
    } finally {
      setBusy(null);
    }
  };

  const i = d ?? issue;
  const text = comment.trim();
  const access = accessFor(account, i.url);
  // GitHub lets an issue's author edit, close and reopen it without write access, and
  // triagers close and reopen anyone's.
  const own = account?.login === i.author;
  const canEdit = !!access?.push || own;
  const canClose = canEdit || !!access?.triage;
  // But an author can't reopen what a maintainer closed.
  const canReopen = !!access?.push || !!access?.triage || (own && d?.closedBy === account?.login);
  // Unlike closing, labeling isn't the author's to do.
  const canLabel = !!access?.push || !!access?.triage;

  const post = async () => {
    if (await act("Comment", () => issues.comment(target, i.number, text), "Comment added")) setComment("");
  };

  // Like GitHub's "Close with comment": the comment goes first so it reads before the close.
  const setOpen = async (open: boolean, reason: CloseReason = "completed") => {
    const verb = open ? "Reopen" : "Close";
    const ok = await act(
      verb,
      async () => {
        if (text) await issues.comment(target, i.number, text);
        await issues.setOpen(target, i.number, open, reason);
      },
      `${open ? "Reopened" : "Closed"} #${i.number}`,
    );
    if (ok) setComment("");
  };

  // Like GitHub, the picked labels are saved together when the picker closes.
  const saveLabels = async () => {
    if (!labels) return;
    const same = labels.length === i.labels.length && labels.every((l) => i.labels.some((m) => m.name === l.name));
    if (!same) await act("Labels", () => issues.setLabels(target, i.number, labels.map((l) => l.name)), "Labels updated");
    setLabels(null);
  };

  const remove = async () => {
    const ok = await ask(`Delete #${i.number} "${i.title}" and all its comments? This can't be undone.`, {
      title: "Delete issue",
      okLabel: "Delete",
      kind: "warning",
    });
    if (!ok) return;
    setBusy("Delete");
    try {
      await issues.delete(target, i.number);
      toast("success", `Deleted #${i.number}`);
      notifyIssuesChanged();
      onDeleted();
    } catch (e) {
      toast("error", "Delete failed", errorMessage(e));
      setBusy(null);
    }
  };

  if (error && !d) {
    return <div className="p-8 text-center text-[12.5px] text-muted-foreground">{error}</div>;
  }

  return (
    // Focusable so the keyboard can scroll it (focusPanel("code") lands here).
    <div data-code-scroll tabIndex={0} className="min-h-0 flex-1 overflow-y-auto outline-none">
      <PageFind />
      <div className="mx-auto max-w-4xl px-6 py-5">
        {editing && d ? (
          <EditIssue
            issue={issue}
            detail={d}
            busy={!!busy}
            onCancel={() => setEditing(false)}
            onSave={async (title, body) => {
              if (await act("Edit", () => issues.edit(target, i.number, title, body), `Updated #${i.number}`)) setEditing(false);
            }}
          />
        ) : (
          <div className="flex items-start gap-3">
            <IssueStateIcon issue={i} className="mt-1.5 size-4" />
            <div className="min-w-0 flex-1">
              <h1 className="text-[17px] leading-snug font-semibold select-text">
                {i.title} <span className="font-normal text-subtle">#{i.number}</span>
              </h1>
              <div className="mt-1.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-[12px] text-muted-foreground">
                <StatePill issue={i} />
                <span className="text-foreground/85">{i.author}</span>
                <span>opened {relativeTime(isoToUnix(i.createdAt))}</span>
                {i.assignees.length > 0 && (
                  <>
                    <span className="text-subtle">·</span>
                    <span>assigned to {i.assignees.join(", ")}</span>
                  </>
                )}
                {(labels ?? i.labels).map((l) => (
                  <LabelChip key={l.name} label={l} />
                ))}
                {canLabel && (
                  <LabelPicker
                    repos={[target]}
                    selected={labels ?? i.labels}
                    onChange={setLabels}
                    onOpenChange={(o) => (o ? setLabels(i.labels) : saveLabels())}
                    hint="Saved when closed"
                  >
                    <button
                      disabled={!!busy || !d}
                      className="inline-flex items-center gap-1 rounded-full border border-dashed border-border-strong px-1.5 text-[10.5px] leading-4 text-subtle hover:text-foreground focus-visible:text-foreground disabled:opacity-50 data-[state=open]:text-foreground"
                    >
                      <Tag className="size-2.5" /> {(labels ?? i.labels).length ? "Edit labels" : "Add labels"}
                    </button>
                  </LabelPicker>
                )}
              </div>
            </div>
          </div>
        )}

        <div className="mt-4 flex flex-wrap items-center gap-1.5">
          {!editing && canEdit && (
            <Button variant="secondary" size="sm" disabled={!!busy || !d} onClick={() => setEditing(true)}>
              <Pencil /> Edit
            </Button>
          )}
          {(i.state === "open" ? canClose : canReopen) &&
            (i.state === "open" ? (
              <CloseButton busy={!!busy} withComment={!!text} onClose={(r) => setOpen(false, r)} />
            ) : (
              <Button variant="secondary" size="sm" disabled={!!busy} onClick={() => setOpen(true)}>
                <CircleDot /> {text ? "Reopen with comment" : "Reopen"}
              </Button>
            ))}
          <Button variant="secondary" size="sm" onClick={() => openOnGitHub(i.url)}>
            <ExternalLink /> Open on GitHub
          </Button>
          <CopyLinkButton url={i.url} />
          <Button variant="ghost" size="icon-sm" onClick={load} disabled={!!busy || detail.loading}>
            <RefreshCw className={cn(detail.loading && "animate-spin")} />
          </Button>
          {busy && (
            <span className="flex items-center gap-1.5 text-[12px] text-muted-foreground">
              <Loader2 className="size-3.5 animate-spin" /> {busy}…
            </span>
          )}
          {/* Cached data stays on screen; a failed refresh (a rate limit, say) is noted beside it. */}
          {error && !busy && (
            <span className="min-w-0 truncate text-[12px] text-removed" title={error}>
              Couldn't refresh: {error}
            </span>
          )}
          {/* GitHub lets repository admins alone delete issues. */}
          {access?.admin && (
            <Button variant="ghost" size="sm" className="ml-auto text-removed hover:text-removed focus-visible:text-removed" disabled={!!busy} onClick={remove}>
              <Trash2 /> Delete
            </Button>
          )}
        </div>

        {!editing && (
          <Section title="Description">
            <PullMarkdown pull={issue} idPrefix="is-d-" text={d?.body || ""} empty={d ? "No description provided." : ""} />
          </Section>
        )}

        {d && d.thread.length > 0 && (
          <Section title="Conversation" aside={`${d.thread.length}`}>
            {d.thread.map((c, n) => (
              <div key={n} className="border-b border-border px-3 py-2.5 last:border-0">
                <div className="flex items-center gap-2 text-[11.5px]">
                  <span className="font-semibold text-foreground">{c.author}</span>
                  <span className="text-subtle">{relativeTime(isoToUnix(c.createdAt))}</span>
                </div>
                {c.body && <PullMarkdown pull={issue} idPrefix={`is-c${n}-`} text={c.body} className="mt-1.5 px-0 py-0" />}
              </div>
            ))}
          </Section>
        )}

        <Section title="Add a comment">
          <div className="p-2">
            <MarkdownInput
              pull={issue}
              value={comment}
              onChange={(e) => setComment(e.target.value)}
              onKeyDown={(e) => {
                if (matchesCommand("github.postComment", e.nativeEvent) && text && !busy) {
                  e.preventDefault();
                  post();
                }
              }}
              placeholder="Leave a comment (markdown)"
              rows={4}
              className="text-[12px]"
            />
            <div className="mt-2 flex justify-end">
              <Button size="sm" disabled={!!busy || !text} onClick={post}>
                Comment
              </Button>
            </div>
          </div>
        </Section>
      </div>
    </div>
  );
}

/** GitHub's close button: completed by default, "not planned" from the menu. */
function CloseButton({ busy, withComment, onClose }: { busy: boolean; withComment: boolean; onClose: (r: CloseReason) => void }) {
  return (
    <div className="flex">
      <Button variant="secondary" size="sm" className="rounded-r-none" disabled={busy} onClick={() => onClose("completed")}>
        <CircleCheck className="text-renamed" /> {withComment ? "Close with comment" : "Close issue"}
      </Button>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="secondary" size="sm" className="w-6 rounded-l-none border-l border-border px-0" disabled={busy}>
            <ChevronDown className="size-3" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="w-64">
          {(Object.keys(CLOSE) as CloseReason[]).map((r) => (
            <DropdownMenuItem key={r} onSelect={() => onClose(r)} className="items-start">
              {r === "completed" ? <CircleCheck className="mt-0.5 text-renamed" /> : <CircleSlash className="mt-0.5 text-subtle" />}
              <span>
                <span className="block">{CLOSE[r].label}</span>
                <span className="block text-[11px] text-muted-foreground">{CLOSE[r].note}</span>
              </span>
            </DropdownMenuItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}

function EditIssue({
  issue,
  detail,
  busy,
  onCancel,
  onSave,
}: {
  issue: Issue;
  detail: { title: string; body: string };
  busy: boolean;
  onCancel: () => void;
  onSave: (title: string, body: string) => void;
}) {
  const [title, setTitle] = useState(detail.title);
  const [body, setBody] = useState(detail.body);
  return (
    <form
      className="space-y-2"
      onSubmit={(e) => {
        e.preventDefault();
        if (title.trim()) onSave(title.trim(), body);
      }}
    >
      <Input autoFocus value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Title" />
      <MarkdownInput pull={issue} value={body} onChange={(e) => setBody(e.target.value)} placeholder="Description (markdown)" rows={12} className="text-[12px]" />
      <div className="flex justify-end gap-1.5">
        <Button type="button" variant="ghost" size="sm" onClick={onCancel} disabled={busy}>
          Cancel
        </Button>
        <Button type="submit" size="sm" disabled={busy || !title.trim()}>
          Save
        </Button>
      </div>
    </form>
  );
}

function StatePill({ issue }: { issue: Pick<Issue, "state" | "stateReason"> }) {
  const [label, cls] =
    issue.state === "open"
      ? ["Open", "bg-added-fill text-on-status"]
      : issue.stateReason === "not_planned"
        ? ["Closed as not planned", "bg-elevated text-muted-foreground border border-border-strong"]
        : ["Closed", "bg-renamed-fill text-on-status"];
  return <span className={cn("rounded-sm px-1.5 py-px text-[10.5px] font-semibold", cls)}>{label}</span>;
}
