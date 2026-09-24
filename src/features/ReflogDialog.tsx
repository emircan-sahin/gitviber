// Where HEAD has been (git reflog): a way back to commits a reset, rebase or amend left behind,
// by a branch made there, a reset of the current one to it, or a look at it (detached).
import { ask } from "@tauri-apps/plugin-dialog";
import { GitBranchPlus, GitCommitHorizontal, History } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Tip } from "@/components/ui/tooltip";
import { api, errorMessage } from "@/lib/api";
import { toast } from "@/lib/toast";
import { tracked, undoAction } from "@/lib/undo";
import { relativeTime } from "@/lib/utils";

type Entry = Awaited<ReturnType<typeof api.reflog>>[number];

export function ReflogDialog({ onClose }: { onClose: () => void }) {
  const [entries, setEntries] = useState<Entry[] | { error: string } | null>(null);
  // The entry a new branch is being named at.
  const [naming, setNaming] = useState<Entry | null>(null);
  const [name, setName] = useState("");
  useEffect(() => {
    api.reflog().then(setEntries, (e) => setEntries({ error: errorMessage(e) }));
  }, []);

  const run = async (label: string, fn: () => Promise<void>, done: string) => {
    try {
      const [, entry] = await tracked(fn);
      toast("success", done, undefined, undoAction(entry, () => {}));
      onClose();
    } catch (e) {
      toast("error", `${label} failed`, errorMessage(e));
    }
  };
  const list = Array.isArray(entries) ? entries : [];
  const head = list[0]?.sha ?? "";

  const reset = async (e: Entry) => {
    const ok = await ask(`Move the current branch to ${e.sha.slice(0, 7)} (${e.message})? Uncommitted changes to tracked files are discarded, and commits after it leave the branch.`, {
      title: "Reset to here",
      kind: "warning",
      okLabel: "Reset",
    });
    if (ok) await run("Reset", () => api.reset(e.sha, "hard", head), `Reset to ${e.sha.slice(0, 7)}`);
  };
  const checkout = (e: Entry) => run("Checkout", () => api.checkoutCommit(e.sha), `Checked out ${e.sha.slice(0, 7)}`);

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-w-2xl">
        <DialogTitle>Reflog</DialogTitle>
        <DialogDescription>Everywhere HEAD has been, newest first. Commits a reset, rebase or amend left behind are still here, until git's cleanup drops them.</DialogDescription>
        <div className="mt-3 max-h-[55vh] overflow-y-auto rounded-md border border-border">
          {!entries && <div className="px-3 py-2 text-[12px] text-subtle">Loading…</div>}
          {entries && "error" in entries && <div className="px-3 py-2 text-[12px] text-muted-foreground">{entries.error}</div>}
          {Array.isArray(entries) && !entries.length && <div className="px-3 py-2 text-[12px] text-subtle">Nothing yet.</div>}
          {list.map((e, i) => (
            <div key={e.selector} className="group flex h-8 items-center gap-2 border-b border-border px-3 text-[12px] last:border-0 hover:bg-hover">
              <span className="w-16 shrink-0 font-mono text-[11px] text-subtle">{e.sha.slice(0, 7)}</span>
              <span className="min-w-0 flex-1 truncate" title={e.message}>
                {e.message}
              </span>
              <span className="shrink-0 text-[11px] text-subtle group-hover:hidden">{i === 0 ? "now" : relativeTime(e.timestamp)}</span>
              <span className="hidden shrink-0 items-center gap-0.5 group-hover:flex">
                <Tip label="Create branch here…">
                  <Button variant="ghost" size="icon-sm" aria-label="Create branch here" onClick={() => setNaming(e)}>
                    <GitBranchPlus />
                  </Button>
                </Tip>
                {i > 0 && (
                  <>
                    <Tip label="Check out (detached)">
                      <Button variant="ghost" size="icon-sm" aria-label="Check out" onClick={() => void checkout(e)}>
                        <GitCommitHorizontal />
                      </Button>
                    </Tip>
                    <Tip label="Reset the current branch to here…">
                      <Button variant="ghost" size="icon-sm" aria-label="Reset to here" onClick={() => void reset(e)}>
                        <History />
                      </Button>
                    </Tip>
                  </>
                )}
              </span>
            </div>
          ))}
        </div>
        {naming && (
          <form
            className="mt-3 flex gap-2"
            onSubmit={(ev) => {
              ev.preventDefault();
              const n = name.trim();
              if (n) void run("Create branch", () => api.createBranchAt(n, naming.sha), `Switched to new branch ${n}`);
            }}
          >
            <Input autoFocus value={name} onChange={(ev) => setName(ev.target.value)} placeholder={`New branch at ${naming.sha.slice(0, 7)}`} spellCheck={false} />
            <Button type="submit" disabled={!name.trim()}>
              Create
            </Button>
          </form>
        )}
      </DialogContent>
    </Dialog>
  );
}
