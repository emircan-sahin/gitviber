import { useCallback, useState } from "react";
import { api, type FileChange, type RepoStatus } from "@/lib/api";
import type { Selection } from "@/lib/repo/selection";
import type { loadWorkspace } from "@/lib/repo/session";
import type { RepoData } from "@/lib/repo/useRepo";
import { failed } from "@/lib/app/toast";
import { currentFile } from "@/features/changes/changeList";
import type { ReviewFiles } from "@/features/changes/BranchReview";

const fileSig = (f: FileChange) => `${f.status}:${f.oid ?? `${f.additions}:${f.deletions}`}`;

/** Review marks per changed file; one lapses when the file's content changes. */
export function useViewed(saved: ReturnType<typeof loadWorkspace>, status: RepoStatus | null, refresh: RepoData["refresh"], review: ReviewFiles | null) {
  // Viewed marks remember the file's content id; a new edit by the agent clears them.
  const [viewedMap, setViewedMap] = useState<Map<string, string>>(() => new Map(saved?.viewed));

  // The file as it is now, and its mark's key: a branch review's are per merge base.
  const marked = useCallback(
    (sel: Selection) => {
      if (sel.kind !== "branch") {
        const f = currentFile(status, sel);
        return f && { f, key: `${sel.kind}:${f.path}` };
      }
      const f = review?.base === sel.base ? review.files.find((x) => x.path === sel.file.path) : undefined;
      return f && { f, key: `branch:${sel.base}:${f.path}` };
    },
    [status, review],
  );

  const viewed = useCallback(
    (sel: Selection) => {
      // Staging is the act of accepting a file, so staged files always count as reviewed.
      if (sel.kind === "staged") return true;
      const m = marked(sel);
      return !!m && viewedMap.get(m.key) === fileSig(m.f);
    },
    [marked, viewedMap],
  );

  const setViewed = useCallback(
    (sels: Selection[], on: boolean) => {
      const files = sels.flatMap((sel) => {
        const m = marked(sel);
        return m ? [{ kind: sel.kind, ...m }] : [];
      });
      // Unchecking a staged file takes it back out of the commit; the tab follows it to Changes.
      const unstage = on ? [] : files.filter((x) => x.kind === "staged").map((x) => x.f.path);
      setViewedMap((m) => {
        const next = new Map(m);
        for (const { kind, f, key } of files) {
          // Drop the mark it had in Changes before staging, or it would come back already checked.
          if (kind === "staged") next.delete(`unstaged:${f.path}`);
          else if (on) next.set(key, fileSig(f));
          else next.delete(key);
        }
        return next;
      });
      // One call for all of them: parallel git calls would fight over index.lock.
      if (unstage.length) api.unstage(unstage).catch(failed("Unstage failed")).finally(() => refresh(false));
    },
    [marked, refresh],
  );

  const toggleViewed = useCallback((sel: Selection) => setViewed([sel], !viewed(sel)), [setViewed, viewed]);

  return { viewedMap, viewed, setViewed, toggleViewed };
}
