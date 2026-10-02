import { useAsyncValue } from "@/hooks/useAsyncValue";
import { api, type RepoStatus } from "@/lib/api";
import { changesView, type Mtimes } from "./changesView";

/** The changed files' modified times while Changes sorts by them, read again with each status (one stat per file). */
export function useMtimes(status: RepoStatus | null): Mtimes | null {
  const recent = changesView.use().sort === "recent";
  const mtimes = useAsyncValue(
    status && recent
      ? async () => {
          const paths = [...new Set([...status.conflicted, ...status.staged, ...status.unstaged].map((f) => f.path))];
          const times = await api.fileMtimes(paths);
          const out = new Map<string, number>();
          paths.forEach((p, i) => times[i] !== null && out.set(p, times[i]!));
          return out as Mtimes;
        }
      : null,
    [status, recent],
    null,
  );
  return recent ? mtimes : null;
}
