import { useState } from "react";
import { ask } from "@/lib/app/ask";
import { toast } from "@/lib/app/toast";
import { api, type Commit, type HistoryEdit } from "@/lib/api";
import type { GitRun } from "@/hooks/useGitAction";
import { dropsPushed, PUSHED_WARNING } from "./commitActions";
import { editedShas, keptUpTo, type Messaging } from "./edits";
import { askStacked } from "./StackedDialog";

/**
 * History's rewrites of the branch: each warns first when it makes pushed commits again, then
 * runs. `commits`: the list, newest first; `apart` is only judged on a whole one (`graph`).
 */
export function useHistoryEdits({ commits, head, graph, run }: { commits: Commit[]; head: string; graph: boolean; run: GitRun }) {
  const [messaging, setMessaging] = useState<Messaging | null>(null);
  const indexOf = (c: Commit) => commits.indexOf(c);

  // Everything from the edit's oldest commit on is made again: pushed ones would need a force-push.
  const rewrite = async (edit: HistoryEdit, about: Commit[]) => {
    // null: from the root, where every commit is made again.
    const drops = await dropsPushed(keptUpTo(editedShas(edit, about), commits));
    if (drops === null) return;
    const [c, n] = [about[0], about.length];
    const verb = { reword: "Reword", squash: edit.kind === "squash" && edit.message === null ? "Fixup" : "Squash", drop: "Drop", move: "Move", reorder: "Move", split: "Split", fixupStaged: "Fixup" }[edit.kind];
    const dropping = n === 1 ? `Drop "${c.subject}"? Its changes leave the branch.` : `Drop ${n} commits? Their changes leave the branch.`;
    const warnings = [...(edit.kind === "drop" ? [dropping] : []), ...(drops ? [PUSHED_WARNING] : [])];
    const title = `${verb} commit${n === 1 ? "" : "s"}`;
    // Branches on the commits it replays can move along: asked in the same dialog as the warnings.
    const stacked = await api.stackedBranches(edit).catch(() => null);
    let branches = false;
    if (stacked?.branches.length) {
      const move = await askStacked({ title, message: warnings.join("\n\n"), okLabel: verb, branches: stacked.branches, checked: stacked.updateRefs });
      if (move === null) return;
      branches = move;
    } else if (warnings.length && !(await ask(warnings.join("\n\n"), { title, kind: "warning", okLabel: verb }))) return;
    const what = n === 1 ? c.shortSha : `${n} commits`;
    const done = edit.kind === "squash" ? squashed(edit, about) : { reword: "Commit reworded", drop: `Dropped ${what}`, move: `Moved ${what}`, reorder: `Moved ${what}`, fixupStaged: `Fixed up ${c.shortSha} with the staged changes`, split: undefined }[edit.kind];
    let stashed = false;
    let splitting = false;
    const ok = await run(verb, async () => {
      const outcome = await api.rewrite(head, edit, branches);
      stashed = outcome === "stashConflicts";
      splitting = outcome === "split";
      return outcome === "conflicts";
    }, done);
    if (ok && splitting) toast("info", `Splitting ${c.shortSha}`, "Its changes are unstaged in Changes. Commit them in pieces with line staging, then Continue.");
    // Done, but the uncommitted changes set aside for it didn't come back cleanly.
    if (ok && stashed) toast("info", "Your uncommitted changes conflict with the new history", "They're marked conflicted in Changes: resolve them there. git keeps a copy in Stashes too; drop it once resolved.");
  };

  // As GitHub Desktop squashes: commits picked from elsewhere move to `onto` first, older ones
  // below it and newer ones above, then they become one there. `message`: ask for the message,
  // else the oldest one's stays (fixup).
  const squash = (about: Commit[], onto: string, message: boolean) => {
    const target = commits.find((c) => c.sha === onto);
    const all = [...new Set([...about, ...(target ? [target] : [])])].sort((a, b) => indexOf(b) - indexOf(a));
    const shas = [...new Set(about.map((c) => c.sha))].filter((s) => s !== onto);
    if (!message) return void rewrite({ kind: "squash", shas, onto, message: null }, [...all].reverse());
    // Only a whole list shows what lies between them.
    const [lo, hi] = [indexOf(all[all.length - 1]), indexOf(all[0])];
    const apart = graph && !!target && commits.slice(lo, hi + 1).some((c) => !c.notInHead && !all.includes(c));
    setMessaging({ kind: "squash", commits: all, shas, onto, apart });
  };

  // As the undo entry names it (commands/history.rs): every commit that ends up in the one.
  const squashed = (edit: HistoryEdit & { kind: "squash" }, about: Commit[]) => {
    const verb = edit.message === null ? "Fixed up" : "Squashed";
    if (edit.shas.length > 1) return `${verb} ${edit.shas.length + 1} commits into one`;
    const [sha, onto] = [edit.shas[0], edit.onto];
    const parent = about.find((c) => c.sha === sha)?.parents[0] === onto;
    return `${verb} ${sha.slice(0, 7)} into ${parent ? "its parent" : onto.slice(0, 7)}`;
  };

  const submit = (m: Messaging, message: string) =>
    m.kind === "reword"
      ? rewrite({ kind: "reword", sha: m.commit.sha, message }, [m.commit])
      : rewrite({ kind: "squash", shas: m.shas, onto: m.onto, message }, [...m.commits].reverse());

  return { rewrite, squash, reword: (commit: Commit) => setMessaging({ kind: "reword", commit }), messaging, closeMessage: () => setMessaging(null), submit };
}
