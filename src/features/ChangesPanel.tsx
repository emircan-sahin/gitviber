import { ask } from "@tauri-apps/plugin-dialog";
import { ArrowLeftToLine, ArrowRightToLine, Check, ChevronDown, GitMerge, Minus, Plus, Undo2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Tip } from "@/components/ui/tooltip";
import { api, errorMessage, type FileChange, type RepoStatus } from "@/lib/api";
import { type Selection, selectionKey } from "@/lib/selection";
import { toast } from "@/lib/toast";
import { cn } from "@/lib/utils";
import { FileIcon } from "./FileIcon";
import { LineCounts, PathLabel, StatusLetter } from "./StatusBadge";

interface Props {
  status: RepoStatus;
  activeKey: string | null;
  onOpen: (s: Selection, pin?: boolean) => void;
  /** Hovering a row starts loading it, so the click feels instant. */
  onHover: (s: Selection) => void;
  refresh: () => Promise<void>;
  viewed: (s: Selection) => boolean;
  toggleViewed: (s: Selection) => void;
}

async function attempt(title: string, fn: () => Promise<unknown>) {
  try {
    await fn();
    return true;
  } catch (e) {
    toast("error", title, errorMessage(e));
    return false;
  }
}

/** Every reviewable change in display order; J/K walk this list. */
export function changeList(status: RepoStatus): (Selection & { kind: "conflict" | "staged" | "unstaged" })[] {
  return [
    ...status.conflicted.map((file) => ({ kind: "conflict" as const, file })),
    ...status.staged.map((file) => ({ kind: "staged" as const, file })),
    ...status.unstaged.map((file) => ({ kind: "unstaged" as const, file })),
  ];
}

export function ChangesPanel({ status, activeKey, onOpen, onHover, refresh, viewed, toggleViewed }: Props) {
  const act = async (title: string, fn: () => Promise<unknown>) => {
    await attempt(title, fn);
    await refresh();
  };

  const discard = async (files: FileChange[]) => {
    const tracked = files.filter((f) => f.status !== "?");
    if (!tracked.length) return;
    const what = tracked.length === 1 ? tracked[0].path : `${tracked.length} files`;
    const ok = await ask(`Discard changes to ${what}? This cannot be undone.`, { title: "Discard changes", kind: "warning", okLabel: "Discard" });
    if (ok) await act("Discard failed", () => api.discard(tracked.map((f) => f.path)));
  };

  const all = changeList(status);
  const reviewed = all.filter(viewed).length;
  const add = all.reduce((n, s) => n + (s.file.additions ?? 0), 0);
  const del = all.reduce((n, s) => n + (s.file.deletions ?? 0), 0);

  const row = (sel: Selection & { kind: "staged" | "unstaged" | "conflict" }, actions: React.ReactNode) => (
    <Row key={selectionKey(sel)} sel={sel} active={activeKey === selectionKey(sel)} viewed={viewed(sel)} onOpen={onOpen} onHover={onHover} onToggleViewed={() => toggleViewed(sel)}>
      {actions}
    </Row>
  );

  return (
    <div className="flex h-full flex-col">
      {status.operation && <OperationBanner status={status} refresh={refresh} />}
      {all.length > 0 && (
        <div className="shrink-0 border-b border-border px-3 py-2">
          <div className="flex items-center gap-2 text-[11.5px]">
            <span className="text-muted-foreground">
              <span className="font-semibold text-foreground">{all.length}</span> files
            </span>
            <span className="font-mono text-[11px]">
              <span className="text-added">+{add}</span> <span className="text-removed">-{del}</span>
            </span>
            <span className="ml-auto text-muted-foreground">
              <span className={cn("font-semibold", reviewed === all.length ? "text-added" : "text-foreground")}>{reviewed}</span>/{all.length} reviewed
            </span>
          </div>
          <div className="mt-1.5 h-[3px] overflow-hidden bg-border">
            <div className="h-full bg-added transition-[width] duration-300" style={{ width: `${(reviewed / all.length) * 100}%` }} />
          </div>
        </div>
      )}
      <div className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto pb-2">
        {!all.length && <AllCaughtUp />}
        {status.conflicted.length > 0 && (
          <Section title="Conflicts" count={status.conflicted.length} tone="text-conflict">
            {status.conflicted.map((file) =>
              row(
                { kind: "conflict", file },
                <>
                  <RowAction label="Mark resolved as it is" onClick={() => act("Stage failed", () => api.stage([file.path]))}>
                    <Check />
                  </RowAction>
                  <RowAction label="Take current version" onClick={() => act("Resolve failed", () => api.resolveSide(file.path, "ours"))}>
                    <ArrowLeftToLine />
                  </RowAction>
                  <RowAction label="Take incoming version" onClick={() => act("Resolve failed", () => api.resolveSide(file.path, "theirs"))}>
                    <ArrowRightToLine />
                  </RowAction>
                </>,
              ),
            )}
          </Section>
        )}
        {status.staged.length > 0 && (
          <Section
            title="Staged"
            count={status.staged.length}
            action={<SectionBtn onClick={() => act("Unstage failed", () => api.unstage(status.staged.map((f) => f.path)))}>Unstage all</SectionBtn>}
          >
            {status.staged.map((file) =>
              row(
                { kind: "staged", file },
                <RowAction label="Unstage" onClick={() => act("Unstage failed", () => api.unstage([file.path]))}>
                  <Minus />
                </RowAction>,
              ),
            )}
          </Section>
        )}
        {status.unstaged.length > 0 && (
          <Section
            title="Changes"
            count={status.unstaged.length}
            action={
              <>
                <SectionBtn onClick={() => discard(status.unstaged)}>Discard</SectionBtn>
                <SectionBtn onClick={() => act("Stage failed", () => api.stage(status.unstaged.map((f) => f.path)))}>Stage all</SectionBtn>
              </>
            }
          >
            {status.unstaged.map((file) =>
              row(
                { kind: "unstaged", file },
                <>
                  {file.status !== "?" && (
                    <RowAction label="Discard changes" onClick={() => discard([file])}>
                      <Undo2 />
                    </RowAction>
                  )}
                  <RowAction label="Stage" onClick={() => act("Stage failed", () => api.stage([file.path]))}>
                    <Plus />
                  </RowAction>
                </>,
              ),
            )}
          </Section>
        )}
      </div>
      {status.operation ? (
        // Committing by hand mid-rebase would splice an extra commit into the history.
        <div className="shrink-0 border-t border-border bg-panel px-3 py-2.5 text-[11.5px] text-muted-foreground">
          A {status.operation.kind} is in progress. Resolve the conflicts, then use <span className="font-medium text-foreground">Continue</span> above.
        </div>
      ) : (
        <CommitBox status={status} refresh={refresh} />
      )}
    </div>
  );
}

const OP_LABEL = { merge: "Merging", rebase: "Rebasing", "cherry-pick": "Cherry-picking", revert: "Reverting" } as const;

/** Shown while a merge/rebase waits for the user: what's happening, what's left, and the way out. */
function OperationBanner({ status, refresh }: Pick<Props, "status" | "refresh">) {
  const op = status.operation!;
  const [busy, setBusy] = useState(false);
  const left = status.conflicted.length;
  const run = async (title: string, fn: () => Promise<boolean | void>) => {
    setBusy(true);
    try {
      const stopped = await fn();
      if (stopped) toast("info", "Stopped on new conflicts", "Resolve them to continue.");
    } catch (e) {
      toast("error", title, errorMessage(e));
    } finally {
      setBusy(false);
      await refresh();
    }
  };
  const abort = async () => {
    const ok = await ask(`Abort the ${op.kind}? Your branch goes back to how it was before it started.`, { title: `Abort ${op.kind}`, kind: "warning", okLabel: "Abort" });
    if (ok) await run("Abort failed", api.opAbort);
  };
  return (
    <div className="shrink-0 border-b border-conflict/40 bg-conflict/10 px-3 py-2">
      <div className="flex items-center gap-2 text-[12px]">
        <GitMerge className="size-3.5 shrink-0 text-conflict" />
        <span className="font-semibold">{OP_LABEL[op.kind]}</span>
        {op.step != null && op.total != null && (
          <span className="font-mono text-[11px] text-muted-foreground">
            {op.step}/{op.total}
          </span>
        )}
        {op.subject && <span className="min-w-0 truncate text-muted-foreground">{op.subject}</span>}
      </div>
      <div className="mt-1 text-[11.5px] text-muted-foreground">
        {left ? `${left} conflict${left > 1 ? "s" : ""} left. Resolve them, then continue.` : "All conflicts resolved. Continue to finish."}
      </div>
      <div className="mt-2 flex gap-1">
        <Button size="sm" className="flex-1" disabled={busy || left > 0} onClick={() => run("Continue failed", api.opContinue)}>
          Continue
        </Button>
        {op.kind === "rebase" && (
          <Button variant="secondary" size="sm" disabled={busy} onClick={() => run("Skip failed", api.rebaseSkip)}>
            Skip commit
          </Button>
        )}
        <Button variant="destructive" size="sm" disabled={busy} onClick={abort}>
          Abort
        </Button>
      </div>
    </div>
  );
}

function Section({ title, count, tone, action, children }: { title: string; count: number; tone?: string; action?: React.ReactNode; children: React.ReactNode }) {
  const [open, setOpen] = useState(true);
  return (
    <div>
      <div className="group sticky top-0 z-10 flex h-7 items-center gap-1 border-b border-border bg-panel pr-1.5 pl-2">
        <button className="flex items-center gap-1 text-[10.5px] font-semibold tracking-[0.08em] text-subtle uppercase hover:text-foreground" onClick={() => setOpen(!open)}>
          <ChevronDown className={cn("size-3 transition-transform", !open && "-rotate-90")} />
          <span className={tone}>{title}</span>
          <span className="ml-1 font-mono tracking-normal text-muted-foreground">{count}</span>
        </button>
        <div className="ml-auto flex gap-0.5 opacity-0 group-hover:opacity-100">{action}</div>
      </div>
      {open && <div className="py-0.5">{children}</div>}
    </div>
  );
}

function SectionBtn({ onClick, children }: { onClick: () => void; children: React.ReactNode }) {
  return (
    <button onClick={onClick} className="h-5 rounded-sm px-1.5 text-[11px] text-muted-foreground hover:bg-active hover:text-foreground">
      {children}
    </button>
  );
}

function Row({
  sel,
  active,
  viewed,
  onOpen,
  onHover,
  onToggleViewed,
  children,
}: {
  sel: Selection & { kind: "staged" | "unstaged" | "conflict" };
  active: boolean;
  viewed: boolean;
  onOpen: (s: Selection, pin?: boolean) => void;
  onHover: (s: Selection) => void;
  onToggleViewed: () => void;
  children?: React.ReactNode;
}) {
  const file = sel.file;
  const ref = useRef<HTMLDivElement>(null);
  // J/K can move the selection off-screen; follow it.
  useEffect(() => {
    if (active) ref.current?.scrollIntoView({ block: "nearest" });
  }, [active]);
  return (
    <div
      ref={ref}
      role="button"
      onClick={() => onOpen(sel)}
      onDoubleClick={() => onOpen(sel, true)}
      onMouseEnter={() => onHover(sel)}
      className={cn(
        "group/row relative flex h-[26px] cursor-pointer items-center gap-2 pr-2 pl-2 text-[12px]",
        active ? "bg-primary/15" : "hover:bg-hover",
      )}
    >
      {active && <span className="absolute inset-y-0 left-0 w-0.5 bg-primary" />}
      {sel.kind === "conflict" ? (
        <GitMerge className="size-3.5 shrink-0 text-conflict" />
      ) : (
      <Tip label={viewed ? "Mark as not viewed" : "Mark as viewed"}>
        <button
          onClick={(e) => {
            e.stopPropagation();
            onToggleViewed();
          }}
          className={cn(
            "flex size-3.5 shrink-0 items-center justify-center rounded-[3px] border",
            viewed ? "border-added bg-added text-black" : "border-border-strong hover:border-muted-foreground",
          )}
        >
          {viewed && <Check className="size-2.5" strokeWidth={3} />}
        </button>
      </Tip>
      )}
      <FileIcon path={file.path} />
      <PathLabel path={file.path} className={cn("flex-1", viewed && "opacity-45")} />
      <LineCounts file={file} className="group-hover/row:hidden" />
      <div className="hidden items-center group-hover/row:flex" onClick={(e) => e.stopPropagation()}>
        {children}
      </div>
      <StatusLetter status={file.status} />
    </div>
  );
}

function RowAction({ label, onClick, children }: { label: string; onClick: () => void; children: React.ReactNode }) {
  return (
    <Tip label={label}>
      <button onClick={onClick} className="flex size-5 items-center justify-center rounded-sm text-muted-foreground hover:bg-active hover:text-foreground [&_svg]:size-3.5">
        {children}
      </button>
    </Tip>
  );
}

function AllCaughtUp() {
  return (
    <div className="flex flex-col items-center gap-1.5 px-6 pt-20 text-center">
      <Check className="mb-1 size-6 text-border-strong" />
      <div className="text-[12.5px] text-muted-foreground">Working tree clean</div>
      <div className="text-[11.5px] leading-relaxed text-subtle">Anything your agent writes shows up here instantly.</div>
    </div>
  );
}

function CommitBox({ status, refresh }: Pick<Props, "status" | "refresh">) {
  const [summary, setSummary] = useState("");
  const [body, setBody] = useState("");
  const [amend, setAmend] = useState(false);
  const [busy, setBusy] = useState(false);

  const hasStaged = status.staged.length > 0;
  const hasAny = hasStaged || status.unstaged.length > 0;
  const canCommit = !busy && !status.conflicted.length && (amend || (summary.trim() !== "" && hasAny));
  const label = amend ? "Amend" : hasStaged ? `Commit ${status.staged.length} staged` : "Commit all";

  const commit = async () => {
    if (!canCommit) return;
    setBusy(true);
    const message = body.trim() ? `${summary.trim()}\n\n${body.trim()}` : summary.trim();
    const ok = await attempt("Commit failed", async () => {
      // Nothing staged means "commit everything", the common case after an agent run.
      if (!hasStaged && !amend) await api.stage(status.unstaged.map((f) => f.path));
      await api.commit(message, amend);
    });
    setBusy(false);
    if (ok) {
      setSummary("");
      setBody("");
      setAmend(false);
      toast("success", amend ? "Commit amended" : "Committed", message.split("\n")[0]);
    }
    await refresh();
  };

  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      commit();
    }
  };

  return (
    <div className="shrink-0 border-t border-border bg-panel p-2">
      <Input placeholder="Summary" value={summary} onChange={(e) => setSummary(e.target.value)} onKeyDown={onKey} className="font-medium" />
      <Textarea placeholder="Description" value={body} onChange={(e) => setBody(e.target.value)} onKeyDown={onKey} rows={2} className="mt-1.5 py-1.5 text-[12px]" />
      <div className="mt-1.5 flex items-center gap-2">
        <label className="flex items-center gap-1.5 text-[11.5px] text-muted-foreground">
          <input type="checkbox" checked={amend} onChange={(e) => setAmend(e.target.checked)} className="accent-primary" />
          Amend
        </label>
        <Tip label={status.branch ? `${label} to ${status.branch}` : label} shortcut="⌘↵">
          <Button className="ml-auto flex-1" disabled={!canCommit} onClick={commit}>
            {busy ? "Committing…" : label}
          </Button>
        </Tip>
      </div>
    </div>
  );
}
