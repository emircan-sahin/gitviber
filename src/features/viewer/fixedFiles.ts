import { useEffect, useState } from "react";
import { api, errorMessage, type FileChange } from "@/lib/api";
import { type ChangesSelection, selectionKey } from "@/lib/repo/selection";

// A commit's files and a range's never change, so leaving the tab and coming back doesn't read
// them again. Few: a big commit's list is thousands of rows.
const MAX = 8;
const read = new Map<string, FileChange[]>();
const NONE = { files: null, error: null };

/** The files of a commit or a range (null while they load, or for the lists Changes keeps up to date), or why they didn't. */
export function useFixedFiles(changes: ChangesSelection): { files: FileChange[] | null; error: string | null } {
  const fixed = changes.list === "commit" || changes.list === "range";
  const key = fixed ? selectionKey(changes) : null;
  const [failed, setFailed] = useState<{ key: string; error: string } | null>(null);
  useEffect(() => {
    if (!key || read.has(key)) return;
    let live = true;
    const load = changes.list === "commit" ? api.commitFiles(changes.commit.sha) : changes.list === "range" ? api.rangeFiles(changes.range.base, changes.range.head) : null;
    load?.then(
      (files) => {
        read.set(key, files);
        if (read.size > MAX) read.delete(read.keys().next().value!);
        if (live) setFailed({ key, error: "" });
      },
      (e) => live && setFailed({ key, error: errorMessage(e) }),
    );
    return () => {
      live = false;
    };
  }, [key]);
  if (!key) return NONE;
  const files = read.get(key);
  if (files) return { files, error: null };
  return { files: null, error: failed?.key === key && failed.error ? failed.error : null };
}
