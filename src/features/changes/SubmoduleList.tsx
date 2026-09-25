// The repository's submodules and whether each is checked out at the commit it records, with
// Update to set them up and bring them there (git submodule update --init --recursive).
import { FolderGit2 } from "lucide-react";
import { api, CANCELLED, errorMessage, type RepoStatus } from "@/lib/api";
import { withNetActivity } from "@/lib/repo/netActivity";
import { toast } from "@/lib/app/toast";
import { cn } from "@/lib/utils";
import { useAsyncValue } from "@/hooks/useAsyncValue";

type Submodule = Awaited<ReturnType<typeof api.submodules>>[number];

/** Reread with status: `git submodule status` is quick, and a checkout may have moved them. */
export function useSubmodules(status: RepoStatus) {
  return useAsyncValue(() => api.submodules().catch((): Submodule[] => []), [status], []);
}

const LOOK: Record<string, [string, string]> = {
  missing: ["not set up", "text-modified"],
  moved: ["on another commit", "text-modified"],
  conflict: ["conflicted", "text-removed"],
  ok: ["", ""],
};

/** Sets up and updates them all, as a network command the top bar shows (with Cancel). */
export async function updateSubmodules(refresh: () => Promise<void>) {
  try {
    await withNetActivity("Update submodules", (op) => api.submoduleUpdate(op));
    toast("success", "Submodules updated");
  } catch (e) {
    if (e === CANCELLED) toast("info", "Submodule update cancelled");
    else toast("error", "Could not update submodules", errorMessage(e));
  } finally {
    await refresh();
  }
}

export function SubmoduleList({ submodules }: { submodules: Submodule[] }) {
  return (
    <div>
      {submodules.map((s) => {
        const [note, color] = LOOK[s.state] ?? LOOK.ok;
        return (
          <div key={s.path} className="flex h-[26px] items-center gap-2 pr-2 pl-4 text-[12px]" title={`${s.path} at ${s.sha}`}>
            <FolderGit2 className="size-3.5 shrink-0 text-subtle" />
            <span className="min-w-0 flex-1 truncate">{s.path}</span>
            {note ? <span className={cn("shrink-0 text-[11px]", color)}>{note}</span> : <span className="shrink-0 font-mono text-[10.5px] text-subtle">{s.sha.slice(0, 7)}</span>}
          </div>
        );
      })}
    </div>
  );
}
