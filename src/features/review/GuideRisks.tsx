import { Bug, LoaderCircle, RotateCw, Square, TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { relativeTime } from "@/lib/format";
import type { Risk, Severity } from "@/lib/review/risks";
import { cn } from "@/lib/utils";
import type { Run } from "./guides";
import { GuideMarkdown, Notice } from "./GuideSection";

const SEVERITY_UI: Record<Severity, { label: string; dot: string }> = {
  high: { label: "High", dot: "bg-destructive" },
  medium: { label: "Medium", dot: "bg-modified" },
  low: { label: "Low", dot: "bg-subtle" },
};

interface Props {
  /** The risks as written; null when the answer wasn't the JSON asked for. */
  risks: Risk[] | null;
  /** The answer as kept, for its raw text, who wrote it and when. */
  saved: { text: string; head: string; program: string; at: number };
  /** The change has moved since the risks were found. */
  outdated: boolean;
  running: boolean;
  onFind: () => void;
  /** Scrolls to the risk's file in the guide. */
  onGo: (r: Risk) => void;
  /** The risk's way to the agent. */
  action: (r: Risk) => ReactNode;
}

/** Find Risks' answer: the risks most severe first, each with its place and its way to the agent. */
export function GuideRisks({ risks, saved, outdated, running, onFind, onGo, action }: Props) {
  return (
    <section aria-labelledby="guide-risks" className="mb-6 rounded-lg border border-border p-3">
      <div className="mb-2 flex items-center gap-2">
        <Bug className="size-3.5 shrink-0 text-subtle" />
        <h2 id="guide-risks" className="text-[13px] font-semibold">
          Risks{risks && ` (${risks.length})`}
        </h2>
        <span className="min-w-0 flex-1 truncate text-[12px] text-muted-foreground">
          · found by {saved.program} at <span className="font-mono">{saved.head.slice(0, 7)}</span>, {relativeTime(saved.at / 1000)}
        </span>
        {!running && (
          <Button size="sm" variant="ghost" onClick={onFind}>
            <RotateCw /> Find Again
          </Button>
        )}
      </div>
      {outdated && !running && (
        <Notice icon={<TriangleAlert />} className="text-modified">
          Outdated: the change has moved since these were found. Find again to check where it is now.
        </Notice>
      )}
      {!risks ? (
        <>
          <Notice icon={<TriangleAlert />}>The answer wasn't in the shape asked for; here it is as written.</Notice>
          <GuideMarkdown text={saved.text} />
        </>
      ) : !risks.length ? (
        <p className="text-[12.5px] text-muted-foreground">No risks found in this change.</p>
      ) : (
        <ol className="flex flex-col divide-y divide-border">
          {risks.map((r, i) => (
            <li key={i} className="flex items-start gap-3 py-2.5 first:pt-1 last:pb-0">
              <span aria-label={`${SEVERITY_UI[r.severity].label} severity`} className={cn("mt-1.5 size-2 shrink-0 rounded-full", SEVERITY_UI[r.severity].dot)} />
              <div className="min-w-0 flex-1 text-[12.5px]">
                <div className="flex flex-wrap items-baseline gap-x-2">
                  <span className="font-medium select-text">{r.title || "Untitled"}</span>
                  {r.path && (
                    <button className="min-w-0 truncate font-mono text-[11.5px] text-primary hover:underline" title={`Go to ${r.path}`} onClick={() => onGo(r)}>
                      {r.path}
                      {r.line ? `:${r.line}` : ""}
                      {r.line && r.side === "old" ? " (removed)" : ""}
                    </button>
                  )}
                </div>
                {r.why && <p className="mt-0.5 text-muted-foreground select-text">{r.why}</p>}
                {r.check && <p className="mt-0.5 text-[12px] text-subtle select-text">Check: {r.check}</p>}
              </div>
              {action(r)}
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}

/** While risks are being found, stopped before, or failed: its own notices, beside the guide's. */
export function RisksRun({ program, run, onCancel }: { program: string; run: Run | null; onCancel: () => void }) {
  if (run === "running")
    return (
      <Notice role="status" icon={<LoaderCircle className="animate-spin" />}>
        Asking {program} to find risks in this change. It goes on while you look at other tabs.{" "}
        <button className="text-primary hover:underline" onClick={onCancel}>
          <Square className="mr-0.5 inline size-3" />
          Cancel
        </button>
      </Notice>
    );
  if (run === "stopped") return <Notice icon={<Square />}>Stopped before {program} found the risks.</Notice>;
  if (run)
    return (
      <Notice role="alert" icon={<TriangleAlert />} className="text-destructive">
        <span className="whitespace-pre-wrap">Couldn't find risks: {run.error}</span>
      </Notice>
    );
  return null;
}
