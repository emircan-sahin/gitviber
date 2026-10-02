import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { api, type CleanedUp, errorMessage, type IgnoredFiles } from "@/lib/api";
import { toast, type ToastAction } from "@/lib/app/toast";
import { formatBytes, plural } from "@/lib/format";
import { type Cleanable, shortPath } from "@/lib/git/worktrees";
import { folderName } from "@/lib/path";
import { tracked, undoAction } from "@/lib/repo/undo";
import { terminalsIn, useTerminals } from "@/lib/terminal/terminals";
import { cn } from "@/lib/utils";
import { type GitRun, useSubmit } from "@/hooks/useGitAction";

/** Ignored entries named before the rest are counted. */
const NAMED = 4;

type Read<T> = T | { error: string };
const failed = <T extends object>(r: Read<T> | undefined): r is { error: string } => !!r && "error" in r;

/**
 * Clean up: merged worktrees removed together, each with its branch where git (or the merged
 * pull request's head) shows nothing on it is lost. Their ignored files (.env, node_modules)
 * aren't changes to git, so `git worktree remove` deletes them without asking: they're listed here.
 */
export function CleanUpWorktrees({ list, main, run, onClose }: { list: Cleanable[]; main: string; run: GitRun; onClose: () => void }) {
  // Read as the dialog opens: what's uncommitted now (all at once: a row can be picked as soon
  // as it's in), then the ignored files, one worktree at a time as each walk sizes folders.
  const [states, setStates] = useState<Record<string, Read<{ uncommitted: number }>>>({});
  const [ignored, setIgnored] = useState<Record<string, Read<IgnoredFiles>>>({});
  const [unpicked, setUnpicked] = useState<Set<string>>(new Set());
  const { pending, send } = useSubmit(onClose);
  // A terminal opened in one meanwhile takes it off.
  useTerminals();
  useEffect(() => {
    let live = true;
    const fail = (e: unknown) => ({ error: errorMessage(e) });
    for (const { worktree: w } of list)
      void api.worktreeState(w.path, false).then(
        (s) => live && setStates((c) => ({ ...c, [w.path]: { uncommitted: s.uncommitted } })),
        (e) => live && setStates((c) => ({ ...c, [w.path]: fail(e) })),
      );
    void list.reduce(
      (chain, { worktree: w }) =>
        chain.then(async () => {
          if (!live) return;
          const read = await api.worktreeIgnored(w.path).catch(fail);
          if (live) setIgnored((c) => ({ ...c, [w.path]: read }));
        }),
      Promise.resolve(),
    );
    return () => {
      live = false;
    };
  }, [list]);

  const blocked = (c: Cleanable) => {
    const state = states[c.worktree.path];
    const sized = ignored[c.worktree.path];
    const terminals = terminalsIn(c.worktree.path);
    if (c.worktree.locked) return `Locked${c.worktree.lockReason ? `: ${c.worktree.lockReason}` : ""}`;
    if (terminals) return `${terminals === 1 ? "A terminal runs" : `${terminals} terminals run`} in it`;
    if (failed(state)) return state.error;
    if (state?.uncommitted) return `${plural(state.uncommitted, "uncommitted change")}: remove it from its row to drop them`;
    if (sized && !failed(sized) && sized.denied.length)
      return sized.denied[0] === "./" ? "Its folder can't be deleted (permission denied)" : `${sized.denied[0]} in it can't be deleted (permission denied)`;
    return null;
  };
  const chosen = list.filter((c) => !unpicked.has(c.worktree.path) && !blocked(c) && states[c.worktree.path]);
  const toggle = (path: string, on: boolean) =>
    setUnpicked((s) => {
      const next = new Set(s);
      if (on) next.delete(path);
      else next.add(path);
      return next;
    });

  const submit = () => {
    // Checked again here and in the backend: the list may be minutes old.
    const items = chosen.filter((c) => !terminalsIn(c.worktree.path)).map((c) => ({ path: c.worktree.path, mergedHead: c.mergedHead }));
    let result: CleanedUp | null = null;
    let entry: number | null = null;
    void send(async () => {
      const ok = await run("Clean up", async () => void ([result, entry] = await tracked(() => api.cleanUpWorktrees(items))));
      if (result) report(result, undoAction(entry, () => {}));
      return ok;
    });
  };

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (chosen.length && !pending) submit();
      }}
    >
      <DialogTitle>Clean up merged worktrees</DialogTitle>
      <DialogDescription>Removes each folder, and its branch where everything on it is merged. Ignored files in them go too, for good.</DialogDescription>
      <div className="-mx-1 mt-3 max-h-80 overflow-y-auto">
        {list.map((c) => {
          const w = c.worktree;
          const state = states[w.path];
          const sized = ignored[w.path];
          const why = blocked(c);
          return (
            <label key={w.path} className={cn("flex items-start gap-2 rounded-sm px-1 py-1.5", why ? "cursor-default opacity-60" : "hover:bg-hover")}>
              <input
                type="checkbox"
                className="mt-0.5 accent-primary"
                checked={!why && !!state && !unpicked.has(w.path)}
                disabled={!!why || !state}
                onChange={(e) => toggle(w.path, e.target.checked)}
              />
              <span className="min-w-0 flex-1">
                <span className="flex min-w-0 items-center gap-1.5">
                  <span className="truncate font-mono text-[12px]">{w.branch ?? folderName(w.path)}</span>
                  <span className="shrink-0 rounded-sm bg-active px-1 text-[10px] leading-4 text-renamed">{c.why}</span>
                </span>
                <span dir="rtl" className="block truncate text-left text-[11px] text-subtle">{`‎${shortPath(w.path, main)}‎`}</span>
                <span className={cn("block text-[11px]", why ? "text-removed" : "text-muted-foreground")}>{why ?? (failed(sized) ? `Couldn't list its ignored files: ${sized.error}` : sized ? ignoredLine(sized) : "Looking for ignored files…")}</span>
              </span>
            </label>
          );
        })}
      </div>
      <div className="mt-4 flex justify-end">
        <Button type="submit" variant="destructive" disabled={!chosen.length || pending}>
          Remove {chosen.length > 1 ? plural(chosen.length, "worktree") : "worktree"}
        </Button>
      </div>
    </form>
  );
}

/** "Deletes ignored node_modules/ (312 MB), .env (9 B) and 3 more · 315 MB in all". */
function ignoredLine(ignored: IgnoredFiles) {
  if (!ignored.entries.length) return "No ignored files in it";
  const { entries, complete } = ignored;
  const total = entries.reduce((n, e) => n + e.bytes, 0);
  const named = entries
    .slice(0, NAMED)
    .map((e) => `${e.path} (${formatBytes(e.bytes)})`)
    .join(", ");
  const more = entries.length > NAMED ? ` and ${entries.length - NAMED} more` : "";
  return `Deletes ignored ${named}${more} · ${complete ? "" : "at least "}${formatBytes(total)} in all`;
}

function report({ removed, deleted, kept, failed: left }: CleanedUp, undo: ToastAction | undefined) {
  const lines = [
    deleted.length && `Deleted ${deleted.length === 1 ? "branch" : "branches"} ${deleted.join(", ")}.`,
    kept.length && `Kept ${kept.length === 1 ? "branch" : "branches"} ${kept.join(", ")}: moved since, or out in another worktree.`,
    ...left.map(([path, why]) => `Left ${folderName(path)}: ${why}.`),
  ].filter(Boolean);
  const title = removed.length ? `Removed ${plural(removed.length, "worktree")}` : "No worktree removed";
  toast(left.length ? "info" : "success", title, lines.join("\n") || undefined, undo);
}
