// A bisect under way, in Changes and History alike: mark the commit checked out good or bad (or
// skip it) until git names the first bad one, then stop to go back where it started.
import { ask } from "@tauri-apps/plugin-dialog";
import { SearchCode } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { api, errorMessage } from "@/lib/api";
import { toast } from "@/lib/app/toast";
import { createStore } from "@/lib/store";

// What git said last, shared by both bars.
const said = createStore<{ message: string; firstBad: string | null } | null>(null);
const tell = said.set;
const useSaid = said.use;

/** Starts looking for the first bad commit between `good` and HEAD (bad). */
export async function startBisect(good: string, refresh: () => unknown) {
  try {
    tell(await api.bisectStart(good));
    toast("info", "Bisecting", "Test the commit checked out, then mark it good or bad.");
  } catch (e) {
    toast("error", "Could not start the bisect", errorMessage(e));
  }
  await refresh();
}

export function BisectBar({ refresh }: { refresh: () => unknown }) {
  const last = useSaid();
  const [busy, setBusy] = useState(false);
  const act = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await fn();
    } catch (e) {
      toast("error", "Bisect failed", errorMessage(e));
    } finally {
      setBusy(false);
      await refresh();
    }
  };
  const mark = (verdict: "good" | "bad" | "skip") =>
    act(async () => {
      const step = await api.bisectMark(verdict);
      tell(step);
      if (step.firstBad) toast("success", `First bad commit: ${step.firstBad.slice(0, 7)}`, "Stop the bisect to go back to your branch.");
    });
  const stop = async () => {
    if (!last?.firstBad && !(await ask("Stop bisecting? You go back to where it started.", { title: "Stop bisect", okLabel: "Stop" }))) return;
    await act(async () => {
      await api.opAbort();
      tell(null);
    });
  };
  return (
    <div className="shrink-0 border-b border-primary/30 bg-primary/10 px-3 py-2">
      <div className="flex items-center gap-2 text-[12px]">
        <SearchCode className="size-3.5 shrink-0 text-primary" />
        <span className="font-semibold">Bisecting</span>
        <span className="min-w-0 truncate text-muted-foreground" title={last?.message}>
          {last?.firstBad ? `The first bad commit is ${last.firstBad.slice(0, 7)}.` : (last?.message ?? "Test the commit checked out, then say how it is.")}
        </span>
      </div>
      <div className="mt-2 flex gap-1">
        {!last?.firstBad && (
          <>
            <Button size="sm" className="flex-1" disabled={busy} onClick={() => void mark("good")}>
              Good
            </Button>
            <Button size="sm" variant="destructive" className="flex-1" disabled={busy} onClick={() => void mark("bad")}>
              Bad
            </Button>
            <Button size="sm" variant="secondary" disabled={busy} onClick={() => void mark("skip")}>
              Skip
            </Button>
          </>
        )}
        <Button size="sm" variant="secondary" className={last?.firstBad ? "flex-1" : undefined} disabled={busy} onClick={() => void stop()}>
          Stop
        </Button>
      </div>
    </div>
  );
}
