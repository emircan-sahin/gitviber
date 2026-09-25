import { useState } from "react";
import type { Branch, GraphRefs } from "@/lib/api";
import { readJson, writeJson } from "@/lib/storage";

const ALL_KEY = "gitviber.history.allBranches";
const REFS_KEY = "gitviber.history.graphRefs";

export const EVERY_REF: GraphRefs = { local: true, remote: true, tags: true, hidden: [], only: null };

/** A branch as the graph's refs name it. */
export const fullRef = (b: Branch) => `${b.remote ? "refs/remotes/" : "refs/heads/"}${b.name}`;

/** Whether History lists every branch: one choice, for every repo. */
export function useAllBranchesSetting() {
  const [on, setOn] = useState(() => readJson<unknown>(ALL_KEY, false) === true);
  const set = (next: boolean) => {
    setOn(next);
    writeJson(ALL_KEY, next);
  };
  return [on, set] as const;
}

/**
 * Which refs the all-branches graph walks, per repository: `root` is its main worktree, as
 * every worktree of it shares its branches.
 */
export function useGraphRefs(root: string | undefined) {
  const load = (r: string | undefined): GraphRefs => {
    const v = r ? readJson<Record<string, Partial<GraphRefs>>>(REFS_KEY, {})[r] : undefined;
    if (!v || typeof v !== "object") return EVERY_REF;
    return {
      local: v.local !== false,
      remote: v.remote !== false,
      tags: v.tags !== false,
      hidden: Array.isArray(v.hidden) ? v.hidden.filter((h) => typeof h === "string") : [],
      only: typeof v.only === "string" ? v.only : null,
    };
  };
  const [state, setState] = useState(() => ({ root, refs: load(root) }));
  const refs = state.root === root ? state.refs : load(root);
  const set = (next: GraphRefs) => {
    setState({ root, refs: next });
    if (!root) return;
    const { [root]: _, ...rest } = readJson<Record<string, GraphRefs>>(REFS_KEY, {});
    writeJson(REFS_KEY, JSON.stringify(next) === JSON.stringify(EVERY_REF) ? rest : { ...rest, [root]: next });
  };
  return [refs, set] as const;
}

/** The graph's refs with these hidden too. Hiding the solo'd branch shows everything else. */
export function hideRefs(refs: GraphRefs, hide: string[]): GraphRefs {
  return { ...refs, only: null, hidden: [...new Set([...refs.hidden, ...hide])] };
}
