import { ask } from "@tauri-apps/plugin-dialog";
import { GitMerge } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { api, type RepoStatus } from "@/lib/api";
import { gitFailed } from "@/hooks/useGitAction";
import { toast } from "@/lib/app/toast";
import { tracked, undoAction } from "@/lib/repo/undo";
import { plural } from "@/lib/format";

const OP_LABEL = { merge: "Merging", rebase: "Rebasing", "cherry-pick": "Cherry-picking", revert: "Reverting", bisect: "Bisecting" } as const;

/** Shown while a merge/rebase waits for the user: what's happening, what's left, and the way out. */
export function OperationBanner({ status, refresh }: { status: RepoStatus; refresh: () => Promise<void> }) {
  const op = status.operation!;
  const [busy, setBusy] = useState(false);
  const left = status.conflicted.length;
  const run = async (title: string, fn: () => Promise<boolean | void>) => {
    setBusy(true);
    try {
      const [stopped, entry] = await tracked(fn);
      if (stopped) toast("info", "Stopped on new conflicts", "Resolve them to continue.");
      // Finished: the entry is the whole merge or rebase, from where it started.
      else if (entry !== null) toast("success", `${op.kind[0].toUpperCase()}${op.kind.slice(1)} finished`, undefined, undoAction(entry, refresh));
    } catch (e) {
      gitFailed(title, e);
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
        {left ? `${plural(left, "conflict")} left. Resolve them, then continue.` : "All conflicts resolved. Continue to finish."}
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
