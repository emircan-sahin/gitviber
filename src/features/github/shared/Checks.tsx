import { Check, CircleDashed, ClipboardCopy, Loader2, MinusCircle, RefreshCw, X } from "lucide-react";
import { Fragment, useCallback, useEffect, useReducer, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { type CiCheck, type CommitChecks, errorMessage, github, repoOf } from "@/lib/api";
import { copyText } from "@/lib/app/clipboard";
import { failureReport } from "@/lib/github/checkFailure";
import { checkNote, FAILING, sortChecks } from "@/lib/github/checks";
import { useGitHubData } from "@/lib/github/githubCache";
import { openTarget } from "@/lib/links/linkHost";
import { cn } from "@/lib/utils";
import { type MarkdownHome, PullMarkdown } from "@/features/github/shared/GitHubMarkdown";

/**
 * A commit's checks, failing first, each with what it waits on or how it ended; "Show failure"
 * reads why a check run failed. `home`: the PR, or the commit's repository, its output's links
 * are relative to; `where`: how "Copy for agent" names it (a PR's url, or commitPlace).
 */
export function CheckRows({ ci: { checks, checksError }, home, where }: { ci: CommitChecks; home: MarkdownHome; where: string }) {
  // The check run whose failure shows: read only when asked, one at a time.
  const [shown, setShown] = useState<number | null>(null);
  // A running check's time goes on without a new read.
  const [, tick] = useReducer((n: number) => n + 1, 0);
  const running = checks.some((c) => c.state === "pending");
  useEffect(() => {
    if (!running) return;
    const timer = window.setInterval(tick, 30_000);
    return () => window.clearInterval(timer);
  }, [running]);
  const rows = sortChecks(checks).map((c, i) => {
    const open = c.id !== null && shown === c.id;
    const note = checkNote(c);
    const started = c.startedAt && `${c.state === "pending" ? "Since" : "Started"} ${new Date(c.startedAt).toLocaleString()}`;
    return (
      <Fragment key={`${c.name}${i}`}>
        <div className="flex h-7 items-center gap-2 px-3 text-[12px]">
          <CheckIcon state={c.state} />
          <span className="min-w-0 truncate">{c.name}</span>
          {note && (
            <span className="min-w-0 truncate text-[11px] text-subtle" title={[note, c.app, started].filter(Boolean).join("\n")}>
              {note}
            </span>
          )}
          <span className="ml-auto flex shrink-0 items-center gap-3">
            {/* Statuses have no output to show; only check runs do. */}
            {FAILING.has(c.state) && c.id !== null && (
              <button onClick={() => setShown(open ? null : c.id)} aria-expanded={open} className="text-[11px] text-subtle hover:text-foreground focus-visible:text-foreground">
                {open ? "Hide failure" : "Show failure"}
              </button>
            )}
            {c.url?.startsWith("https://github.com/") && (
              <button onClick={() => github.openUrl(c.url!)} className="text-[11px] text-subtle hover:text-foreground focus-visible:text-foreground">
                Details
              </button>
            )}
          </span>
        </div>
        {open && <Failure home={home} where={where} check={c} id={c.id!} />}
      </Fragment>
    );
  });
  return (
    <>
      {checksError && <div className="px-3 py-2 text-[12px] text-removed">Could not load all checks: {checksError}</div>}
      {rows}
    </>
  );
}

/** A finished check run doesn't change (a re-run is a new one), so it's read once a session; Retry reads a part that failed again. */
const KEEP = 3_600_000;

function Failure({ home, where, check, id }: { home: MarkdownHome; where: string; check: CiCheck; id: number }) {
  const target = repoOf(home.url);
  const failure = useGitHubData(`check:${target}:${id}`, useCallback(() => github.checkFailure(target, id), [target, id]), KEEP);
  const f = failure.data;
  const log = useRef<HTMLPreElement>(null);
  // The error is at the log's end.
  useEffect(() => {
    if (log.current) log.current.scrollTop = log.current.scrollHeight;
  }, [f?.log]);

  return (
    <div className="border-y border-border bg-panel/50 px-3 py-2 text-[12px]">
      {!f ? (
        failure.error !== undefined && !failure.loading ? (
          <div className="flex items-center gap-2 text-removed">
            <span className="min-w-0 flex-1">Could not read the check: {errorMessage(failure.error)}</span>
            <Button variant="ghost" size="sm" onClick={() => void failure.refresh(true)}>
              <RefreshCw /> Retry
            </Button>
          </div>
        ) : (
          <div className="flex items-center gap-2 text-subtle">
            <Loader2 className="size-3.5 animate-spin" /> Reading its output and log…
          </div>
        )
      ) : (
        <>
          {f.title && <div className="font-semibold">{f.title}</div>}
          {f.summary && <PullMarkdown pull={home} idPrefix={`ck-${id}-`} text={f.summary} className="px-0 py-1" />}
          {f.annotations.map((a, i) => (
            <div key={i} className="flex gap-2 py-0.5">
              <span className={cn("shrink-0 text-[11px]", a.level === "failure" ? "text-removed" : a.level === "warning" ? "text-modified" : "text-subtle")}>{a.level}</span>
              <div className="min-w-0">
                {/* Actions puts its own errors ("exit code 1") on .github, a folder. */}
                {a.path && a.path !== ".github" ? (
                  <button onClick={() => openTarget({ path: a.path, line: a.line || undefined, column: 1 })} className="font-mono text-[11.5px] text-primary hover:underline">
                    {a.path}
                    {a.line ? `:${a.line}` : ""}
                  </button>
                ) : null}{" "}
                {a.title && <span className="font-semibold">{a.title}: </span>}
                <span className="whitespace-pre-wrap select-text">{a.message}</span>
              </div>
            </div>
          ))}
          {f.log && (
            <pre ref={log} className="mt-1.5 max-h-72 overflow-auto rounded-sm border border-border bg-background p-2 font-mono text-[11px] leading-4 whitespace-pre select-text">
              {f.log}
            </pre>
          )}
          {f.annotationsError && <div className="mt-1 text-removed">Could not read the annotations: {f.annotationsError}</div>}
          {f.logError && <div className="mt-1 text-removed">Could not read the job's log: {f.logError}</div>}
          {!f.title && !f.summary && !f.annotations.length && !f.log && !f.logError && !f.annotationsError && (
            <div className="text-subtle">The check left no output; its Details page may say more.</div>
          )}
          <div className="mt-2 flex gap-1.5">
            <Button variant="secondary" size="sm" onClick={() => void copyText(failureReport(check.name, where, f), "Copied for the agent", "Paste it into the agent's terminal.")}>
              <ClipboardCopy /> Copy for agent
            </Button>
            {(f.logError || f.annotationsError) && (
              <Button variant="ghost" size="sm" disabled={failure.loading} onClick={() => void failure.refresh(true)}>
                <RefreshCw className={cn(failure.loading && "animate-spin")} /> Retry
              </Button>
            )}
          </div>
        </>
      )}
    </div>
  );
}

function CheckIcon({ state }: { state: string }) {
  // Read out before the name, which alone doesn't say how the check went.
  const label = { "aria-label": state === "pending" ? "running" : state.replace(/_/g, " "), role: "img" };
  if (state === "success") return <Check {...label} className="size-3.5 shrink-0 text-added" />;
  if (state === "pending") return <CircleDashed {...label} className="size-3.5 shrink-0 animate-spin text-modified [animation-duration:3s]" />;
  if (FAILING.has(state)) return <X {...label} className="size-3.5 shrink-0 text-removed" />;
  return <MinusCircle {...label} className="size-3.5 shrink-0 text-subtle" />;
}
