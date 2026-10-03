import { askStacked } from "@/features/history/StackedDialog";
import { ask } from "@/lib/app/ask";
import { api, type Branch, CANCELLED, errorMessage, type MainBack, type PullMode, type Worktree } from "@/lib/api";
import { toast } from "@/lib/app/toast";
import { openTerminal, terminalsIn } from "@/lib/terminal/terminals";
import { forgetRemoteTags } from "@/lib/repo/remoteTags";
import { worktreeDir } from "@/lib/repo/session";
import type { RepoData } from "@/lib/repo/useRepo";
import { plural } from "@/lib/format";
import { folderName } from "@/lib/path";
import { type GitRun, useGitAction } from "@/hooks/useGitAction";
import { undoCommit } from "@/features/history/commitActions";
import { secretCommits } from "@/lib/git/gitErrors";
import { shortRef } from "@/lib/git/refs";
import { defaultBranch, folderForBranch } from "@/lib/git/worktrees";

// Autostashed changes wait out a stopped merge or rebase (MERGE_AUTOSTASH), and git keeps them
// in the stash as well when they conflict coming back.
const stashedFor = (autostash: boolean, what = "pull") =>
  autostash
    ? `Resolve them in Changes. Your uncommitted changes were set aside for the ${what} and come back when it finishes (Continue or Abort, if it's waiting on you). If they conflict coming back, they're kept in Stashes too: drop that stash once resolved.`
    : undefined;
const retryStashed = (again: () => Promise<boolean>) => [{ label: "Retry with autostash", run: () => void again() }];

/** Merges `name` into HEAD's branch. A squash's changes would get the stash back before they're committed: no autostash for it. */
export const mergeInto = (run: GitRun, name: string, how: "ff" | "no-ff" | "squash" = "ff", autostash = false): Promise<boolean> =>
  how === "squash"
    ? run("Squash merge", () => api.merge(name, how), `Squashed ${shortRef(name)} into one commit`)
    : run("Merge", () => api.merge(name, how, autostash), `Merged ${shortRef(name)}`, undefined, {
        fixes: { autostash: retryStashed(() => mergeInto(run, name, how, true)) },
        conflicts: stashedFor(autostash, "merge"),
      });

/** The top bar's git actions: switching, merging, deleting branches, worktrees, pull, push and publish. */
export function useRepoActions(repo: RepoData, root: string, main: string) {
  const { status, branches } = repo;
  const { busy, run, runNet } = useGitAction({ refresh: repo.refresh });

  // A terminal on another branch gets its own worktree rather than a checkout here, which
  // would pull the files out from under this window.
  const branchTerminal = async (name: string) => {
    if (name === status?.branch) return openTerminal(root);
    const dir = worktreeDir(main);
    const where = `${dir ?? `${folderName(main)}.worktrees`}/${folderForBranch(name)}`;
    // The default branch held by a worktree can't be switched to in the main folder.
    const keep = name === defaultBranch(branches) ? ` ${name} is best kept in ${folderName(main)}: a worktree that holds it blocks switching to it there.` : "";
    const ok = await ask(`${name} isn't checked out anywhere. Create a worktree for it at ${where}${dir ? "" : ", next to this project,"} and open a terminal there?${keep}`, {
      title: "Open terminal on branch",
      okLabel: "Create worktree",
    });
    if (ok) await run("Create worktree", async () => openTerminal(await api.addWorktree(name, null, dir)), `${name} checked out in ${where}`);
  };

  // Merged is deleted outright: nothing is lost; merged upstream too, once the backend checks it
  // again as it is now. Anything else needs a yes, then -D. A remote branch always asks: the
  // push takes it away for everyone.
  const deleteBranch = async (b: Branch, upstream = false) => {
    if (b.remote) {
      const [remote, ...rest] = b.name.split("/");
      const ok = await ask(`Delete ${rest.join("/")} from ${remote}? It goes for everyone who uses ${remote}; local branches stay.`, {
        title: "Delete remote branch",
        kind: "warning",
        okLabel: "Delete",
      });
      if (ok) await runNet("Delete remote branch", (op) => api.deleteRemoteBranch(b.name, op), `Deleted ${b.name}`);
      return;
    }
    const here = status?.branch ?? "HEAD";
    if (!b.merged && !upstream) {
      const ok = await ask(`${b.name} isn't known to be merged into ${here}. Deleting it loses any commits that exist only on it.`, {
        title: "Delete branch",
        kind: "warning",
        okLabel: "Delete",
      });
      if (!ok) return;
    }
    await run("Delete branch", () => (upstream ? api.deleteMerged([], [b.name]) : api.deleteBranches([b.name], !b.merged)), `Deleted ${b.name}`);
  };

  const cleanUp = async (merged: string[], upstream: string[]) => {
    const names = [...merged, ...upstream];
    const label = (n: string) => (upstream.includes(n) ? `${n} (merged upstream)` : n);
    const shown = names.slice(0, 12).map(label).join("\n") + (names.length > 12 ? `\n…and ${names.length - 12} more` : "");
    const here = status?.branch ?? "HEAD";
    const where = !upstream.length ? `already merged into ${here}` : merged.length ? `merged into ${here} or upstream` : "squash- or rebase-merged upstream";
    const count = `${names.length} ${names.length === 1 ? "branch" : "branches"}`;
    const ok = await ask(`Delete ${count} ${where}?\n\n${shown}`, {
      title: "Clean up merged branches",
      okLabel: "Delete",
    });
    if (ok) await run("Clean up", () => api.deleteMerged(merged, upstream), `Deleted ${count}`);
  };

  // A pull brings in the upstream, which can't help a push that goes elsewhere (a fork pulling
  // upstream/dev and pushing origin/dev).
  const pushesUpstream = !status?.push?.branch || status.push.branch === status.upstream;
  const pulls = (["merge", "rebase"] as const).map((mode) => ({ label: `Pull (${mode})`, run: () => void pull(mode) }));
  const behind = pushesUpstream ? pulls : undefined;
  // A push GitHub refused over a secret: when only the last commit has it, undoing that brings
  // the file back staged to fix. Only a plain commit not yet pushed, as History's Undo commit.
  const head = repo.commits[0];
  const undoLast = (message: string) => {
    const listed = secretCommits(message);
    const onlyHead = !!head && listed.length > 0 && listed.every((sha) => head.sha.startsWith(sha));
    return onlyHead && head.unpushed && head.parents.length === 1 && status?.head && head.sha.startsWith(status.head)
      ? [{ label: "Undo last commit", run: () => void undoCommit(head.sha, run) }]
      : undefined;
  };
  const pull = (mode: PullMode, autostash = false): Promise<boolean> =>
    runNet("Pull", (op) => api.pull(mode, op, autostash), mode === "ff" ? "Pulled" : `Pulled (${mode})`, {
      fixes: { diverged: pulls, autostash: retryStashed(() => pull(mode, true)) },
      conflicts: stashedFor(autostash),
    });
  const sync = (autostash = false): Promise<boolean> =>
    runNet("Sync", async (op) => (await api.pull("ff", op, autostash)) || api.push(false, undefined, op), "Synced", {
      fixes: { diverged: pulls, "fetch-first": behind, autostash: retryStashed(() => sync(true)), secret: undoLast },
      conflicts: stashedFor(autostash),
    });

  const publish = (remote: string) => runNet("Publish", (op) => api.push(false, remote, op), `Branch published to ${remote}`, { fixes: { secret: undoLast } });
  // Where Publish goes without asking: the preferred remote, or the only one.
  const publishTo = status?.branch && status.head ? (status.publish ?? (status.remotes.length === 1 ? status.remotes[0] : null)) : null;
  const merge = (name: string, how: "ff" | "no-ff" | "squash" = "ff", autostash = false) => mergeInto(run, name, how, autostash);
  // `updateRefs`: whether the branches on the replayed commits move along; unset, asked when there are any.
  const rebase = async (onto: string, autostash = false, updateRefs?: boolean): Promise<boolean> => {
    if (updateRefs === undefined) {
      const stacked = await api.rebaseStacked(onto).catch(() => null);
      if (stacked?.branches.length) {
        const move = await askStacked({ title: `Rebase onto ${onto}`, message: "", okLabel: "Rebase", branches: stacked.branches, checked: stacked.updateRefs });
        if (move === null) return false;
        updateRefs = move;
      }
    }
    return run("Rebase", () => api.rebase(onto, autostash, updateRefs), `Rebased onto ${onto}`, undefined, {
      fixes: { autostash: retryStashed(() => rebase(onto, true, updateRefs)) },
      conflicts: stashedFor(autostash, "rebase"),
    });
  };
  // Rejected as non-fast-forward: when the remote's extra commits are this branch's own from
  // before a rebase or amend, replacing them is a force push, so it asks first. Anyone else's
  // (already fetched in the background, so not "fetch first") want a pull, which the error
  // toast offers.
  // `tags`: --follow-tags, annotated tags on the pushed commits go along.
  const push = (tags = false) =>
    runNet(
      "Push",
      async (op) => {
        try {
          await api.push(false, undefined, op, tags);
        } catch (e) {
          if (!errorMessage(e).includes("non-fast-forward") || !(await api.remoteWasOurs())) throw e;
          const ok = await ask(
            "The remote branch has commits yours no longer has, as after a rebase or an amend. Replace them with yours?\n\nThis force-pushes (with lease): it is refused if someone pushed commits there that your branch never had. Anyone who pulled the old commits will have to reconcile.",
            { title: "Force push", kind: "warning", okLabel: "Force push" },
          );
          if (!ok) throw CANCELLED;
          await api.push(true, undefined, op, tags);
        }
        if (tags) forgetRemoteTags();
      },
      tags ? "Pushed with tags" : "Pushed",
      { fixes: { "fetch-first": behind, secret: undoLast } },
    );
  // Unknown until the push target has the branch; then a push is due.
  const pushAhead = status?.push ? (status.push.branch ? status.push.ahead : null) : (status?.ahead ?? 0);

  // Changes that the other branch's files would overwrite: git refuses, and GitHub Desktop's way
  // out is to leave them here in a stash (Stashes in Changes brings them back).
  const switching = (to: string, fn: () => Promise<void>) => async () => {
    try {
      await fn();
    } catch (e) {
      if (!errorMessage(e).includes("would be overwritten by checkout")) throw e;
      const here = status?.branch ?? "this commit";
      const ok = await ask(`Your changes to some files conflict with ${to}. Stash them and switch? They stay in Stashes, to bring back on ${here} or anywhere.`, {
        title: "Switch branch",
        okLabel: "Stash and Switch",
      });
      if (!ok) return;
      await api.stashPush(`Left on ${here} when switching to ${to}`, true);
      await fn();
    }
  };

  // Names the folder that switched: a branch another worktree holds opens that worktree instead,
  // and a bare "Switched to x" once read as having opened it.
  const switched = (name: string) => `Switched ${folderName(root)} to ${name}`;
  const switchBranch = (name: string) => run("Switch branch", switching(name, () => api.switchBranch(name, false)), switched(name));

  // A branch another worktree holds, checked out here as its commit: the files to read or build,
  // with no branch to move. Starting a new branch from it is the picker's menu.
  const detachHere = (b: Branch) =>
    run("Check out", switching(b.name, () => api.checkoutCommit(b.sha)), `Checked out ${b.name} in ${folderName(root)}, detached`, "No branch is moved: commits made here belong to no branch until you create one.");

  // upstream/dev → dev. A local dev that tracks something else (origin/dev, say) is a
  // different line of work; say so rather than switch to it silently.
  const switchRemote = async (b: Branch) => {
    const name = b.name.slice(b.name.indexOf("/") + 1);
    const local = branches.find((x) => !x.remote && x.name === name);
    if (local?.current) return;
    if (local && local.upstream !== b.name) {
      const tracks = local.upstream ? `tracks ${local.upstream}` : "tracks nothing";
      const ok = await ask(`A local ${name} already exists and ${tracks}, not ${b.name}. Switch to it as it is?`, { title: "Switch branch", okLabel: "Switch" });
      if (!ok) return;
    }
    await run("Switch branch", switching(name, () => api.switchTracking(b.name)), switched(name));
  };

  // A fresh count decides force: git refuses a dirty or locked worktree otherwise, and the
  // warning must say what gets lost. If counting fails, git's own refusal is the fallback.
  const removeWorktree = async (w: Worktree): Promise<boolean> => {
    const name = folderName(w.path);
    // A missing folder may only be on a drive that isn't plugged in; pruned, the link is gone
    // for good even once it's back. The branch stays either way.
    if (w.prunable && !w.locked) {
      const drive = /^(\/Volumes|\/media|\/run\/media|\/mnt)\//.test(w.path) ? " It was on another drive: if that's only unplugged, plug it in instead." : "";
      const ok = await ask(`${name}'s folder is gone. Prune it from the worktree list?${drive} The branch${w.branch ? ` ${w.branch}` : ""} stays.`, {
        title: "Prune worktree",
        okLabel: "Prune",
      });
      return ok && run("Prune worktree", () => api.removeWorktree(w.path, false), `Pruned ${name}`);
    }
    const changed = w.prunable ? 0 : await api.worktreeState(w.path, false).then((s) => s.uncommitted, () => 0);
    const branch = w.branch ? ` The branch ${w.branch} stays.` : "";
    const lost = changed ? ` Its ${changed} uncommitted ${changed === 1 ? "change" : "changes"} will be lost.` : "";
    const lock = w.inUse
      ? ` Something is working in it right now (${w.lockReason ?? "it holds the lock"}); deleting pulls the folder out from under it.`
      : w.locked
        ? ` It's locked${w.lockReason ? ` (${w.lockReason})` : ""}; this overrides the lock. The lock on its row unlocks it instead.`
        : "";
    const open = terminalsIn(w.path);
    const terminals = open ? ` ${open === 1 ? "A terminal runs" : `${open} terminals run`} in it; whatever runs there, like an agent or a dev server, loses its folder.` : "";
    const ok = await ask(
      w.prunable ? `${name}'s folder is gone, but it's locked${w.lockReason ? ` (${w.lockReason})` : ""}: its drive may only be unplugged. Prune it anyway?${branch}` : `Delete worktree ${name} and its folder?${lost}${lock}${terminals}${branch}`,
      { title: w.prunable ? "Prune worktree" : "Remove worktree", kind: "warning", okLabel: w.prunable ? "Prune" : "Delete worktree" },
    );
    if (!ok) return false;
    const force = changed > 0 || w.locked;
    return run(
      "Remove worktree",
      async () => {
        try {
          await api.removeWorktree(w.path, force);
        } catch (e) {
          // git refuses any worktree with submodules checked out, clean or not, unless forced.
          if (force || !errorMessage(e).includes("working trees containing submodules cannot be moved or removed")) throw e;
          const again = await ask(`${name} has submodules, which git only removes with force: everything in it goes, including commits made only inside its submodules. Delete anyway?`, { title: "Remove worktree", kind: "warning", okLabel: "Delete worktree" });
          if (!again) throw CANCELLED;
          await api.removeWorktree(w.path, true);
        }
      },
      `Worktree ${name} removed`,
    );
  };

  // Git keeps a branch for a worktree whose folder is gone until it's pruned, and refuses to
  // switch to it meanwhile; opening the folder would only fail.
  const pruneHolder = async (w: Worktree, branch: string) => {
    if (await removeWorktree(w)) await switchBranch(branch);
  };

  // The default branch back in the main folder, which git won't switch to while a linked worktree
  // holds it. Neither folder's files change, so Undo can hand it back as long as nothing moved on.
  const moveMainBack = async (branch: string) => {
    let plan: MainBack;
    try {
      plan = await api.mainBackPlan(branch);
    } catch (e) {
      toast("error", `Can't move ${branch} back`, errorMessage(e));
      return;
    }
    const [into, held] = [folderName(plan.main), folderName(plan.holder)];
    const shells = terminalsIn(plan.holder);
    const note = shells ? ` ${plural(shells, "terminal")} open in ${held} will be on a detached HEAD.` : "";
    const ok = await ask(
      `${into} is on ${plan.mainBranch}, and ${branch} is checked out in ${held}, so git won't switch ${into} to it.\n\nThis detaches ${held} where it stands (its files don't change), then switches ${into} to ${branch}. ${plan.mainBranch} stays as a branch, and nothing is deleted.${note}`,
      { title: `Move ${branch} back to ${into}`, okLabel: "Move" },
    );
    if (!ok) return;
    const undo = () => void run("Undo", () => api.undoMainBack(plan), `${into} is back on ${plan.mainBranch}, ${held} on ${branch}`);
    if (await run("Move main back", () => api.moveMainBack(branch))) toast("success", `${branch} is back in ${into}`, `${held} is detached at the same commit.`, { label: "Undo", run: undo });
  };

  const unlockWorktree = async (w: Worktree) => {
    const name = folderName(w.path);
    const why = w.lockReason ?? "it holds the lock";
    const msg = `Something is working in ${name} right now (${why}). Unlocked, it can be pruned, moved or removed while that runs.`;
    if (w.inUse && !(await ask(msg, { title: "Unlock worktree", kind: "warning", okLabel: "Unlock" }))) return;
    await run("Unlock worktree", () => api.unlockWorktree(w.path), `Unlocked ${name}`);
  };

  return { busy, run, runNet, pull, sync, branchTerminal, deleteBranch, cleanUp, publish, publishTo, merge, rebase, push, pushAhead, switchBranch, switchRemote, detachHere, pruneHolder, moveMainBack, removeWorktree, unlockWorktree };
}
