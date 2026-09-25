import { ask } from "@tauri-apps/plugin-dialog";
import { api, type Branch, errorMessage, type PullMode, type Worktree } from "@/lib/api";
import { openTerminal } from "@/lib/terminal/terminals";
import { forgetRemoteTags } from "@/lib/repo/remoteTags";
import { worktreeDir } from "@/lib/repo/session";
import type { RepoData } from "@/lib/repo/useRepo";
import { folderName } from "@/lib/path";
import { useGitAction } from "@/hooks/useGitAction";

/** The top bar's git actions: switching, merging, deleting branches, worktrees, pull, push and publish. */
export function useRepoActions(repo: RepoData, root: string, main: string) {
  const { status, branches } = repo;
  const { busy, run, runNet } = useGitAction({ refresh: repo.refresh });

  // A terminal on another branch gets its own worktree rather than a checkout here, which
  // would pull the files out from under this window.
  const branchTerminal = async (name: string) => {
    if (name === status?.branch) return openTerminal(root);
    const dir = worktreeDir(main);
    const where = `${dir ?? `${folderName(main)}.worktrees`}/${name.replaceAll("/", "-")}`;
    const ok = await ask(`${name} isn't checked out anywhere. Create a worktree for it at ${where}${dir ? "" : ", next to this project,"} and open a terminal there?`, {
      title: "Open terminal on branch",
      okLabel: "Create worktree",
    });
    if (ok) await run("Create worktree", async () => openTerminal(await api.addWorktree(name, null, dir)), `${name} checked out in ${where}`);
  };

  // Merged is deleted outright: nothing is lost. Anything else needs a yes, then -D.
  // A remote branch always asks: the push takes it away for everyone.
  const deleteBranch = async (b: Branch) => {
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
    if (!b.merged) {
      const ok = await ask(`${b.name} isn't known to be merged into ${here}. Deleting it loses any commits that exist only on it.`, {
        title: "Delete branch",
        kind: "warning",
        okLabel: "Delete",
      });
      if (!ok) return;
    }
    await run("Delete branch", () => api.deleteBranches([b.name], !b.merged), `Deleted ${b.name}`);
  };

  const cleanUp = async (names: string[]) => {
    const shown = names.slice(0, 12).join("\n") + (names.length > 12 ? `\n…and ${names.length - 12} more` : "");
    const ok = await ask(`Delete ${names.length} branches already merged into ${status?.branch ?? "HEAD"}?\n\n${shown}`, {
      title: "Clean up merged branches",
      okLabel: "Delete",
    });
    if (ok) await run("Clean up", () => api.deleteBranches(names, false), `Deleted ${names.length} merged branches`);
  };

  // A pull brings in the upstream, which can't help a push that goes elsewhere (a fork pulling
  // upstream/dev and pushing origin/dev).
  const pushesUpstream = !status?.push?.branch || status.push.branch === status.upstream;
  const pulls = (["merge", "rebase"] as const).map((mode) => ({ label: `Pull (${mode})`, run: () => void pull(mode) }));
  const behind = pushesUpstream ? pulls : undefined;
  // Autostashed changes wait out a stopped merge or rebase (MERGE_AUTOSTASH), and git keeps them
  // in the stash as well when they conflict coming back.
  const stashedFor = (autostash: boolean) =>
    autostash
      ? "Resolve them in Changes. Your uncommitted changes were set aside for the pull and come back when it finishes (Continue or Abort, if it's waiting on you). If they conflict coming back, they're kept in Stashes too: drop that stash once resolved."
      : undefined;
  const pull = (mode: PullMode, autostash = false): Promise<boolean> =>
    runNet("Pull", (op) => api.pull(mode, op, autostash), mode === "ff" ? "Pulled" : `Pulled (${mode})`, {
      fixes: { diverged: pulls, autostash: [{ label: "Retry with autostash", run: () => void pull(mode, true) }] },
      conflicts: stashedFor(autostash),
    });
  const sync = (autostash = false): Promise<boolean> =>
    runNet("Sync", async (op) => (await api.pull("ff", op, autostash)) || api.push(false, undefined, op), "Synced", {
      fixes: { diverged: pulls, "fetch-first": behind, autostash: [{ label: "Retry with autostash", run: () => void sync(true) }] },
      conflicts: stashedFor(autostash),
    });

  const publish = (remote: string) => runNet("Publish", (op) => api.push(false, remote, op), `Branch published to ${remote}`);
  // Where Publish goes without asking: the preferred remote, or the only one.
  const publishTo = status?.branch && status.head ? (status.publish ?? (status.remotes.length === 1 ? status.remotes[0] : null)) : null;
  const merge = (name: string, how: "ff" | "no-ff" | "squash" = "ff") =>
    run(how === "squash" ? "Squash merge" : "Merge", () => api.merge(name, how), how === "squash" ? `Squashed ${name} into one commit` : `Merged ${name}`);
  // Rejected as non-fast-forward: the remote has commits this branch dropped, usually its own
  // old ones after a rebase or amend. Replacing them is a force push, so it asks first.
  // "fetch first" (commits not fetched yet) isn't offered: those want a pull, which the error
  // toast offers.
  // `tags`: --follow-tags, annotated tags on the pushed commits go along.
  const push = (tags = false) =>
    runNet(
      "Push",
      async (op) => {
        try {
          await api.push(false, undefined, op, tags);
        } catch (e) {
          if (!errorMessage(e).includes("non-fast-forward")) throw e;
          const ok = await ask(
            "The remote branch has commits yours no longer has, as after a rebase or an amend. Replace them with yours?\n\nThis force-pushes (with lease): it is refused if someone pushed commits there that your branch never had. Anyone who pulled the old commits will have to reconcile.",
            { title: "Force push", kind: "warning", okLabel: "Force push" },
          );
          if (!ok) throw e;
          await api.push(true, undefined, op, tags);
        }
        if (tags) forgetRemoteTags();
      },
      tags ? "Pushed with tags" : "Pushed",
      { fixes: { "fetch-first": behind } },
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
    await run("Switch branch", switching(name, () => api.switchTracking(b.name)), `Switched to ${name}`);
  };

  // A fresh count decides force: git refuses a dirty or locked worktree otherwise, and the
  // warning must say what gets lost. If counting fails, git's own refusal is the fallback.
  const removeWorktree = async (w: Worktree) => {
    const name = folderName(w.path);
    // A missing folder may only be on a drive that isn't plugged in; pruned, the link is gone
    // for good even once it's back. The branch stays either way.
    if (w.prunable && !w.locked) {
      const drive = /^(\/Volumes|\/media|\/run\/media|\/mnt)\//.test(w.path) ? " It was on another drive: if that's only unplugged, plug it in instead." : "";
      const ok = await ask(`${name}'s folder is gone. Prune it from the worktree list?${drive} The branch${w.branch ? ` ${w.branch}` : ""} stays.`, {
        title: "Prune worktree",
        okLabel: "Prune",
      });
      if (ok) await run("Prune worktree", () => api.removeWorktree(w.path, false), `Pruned ${name}`);
      return;
    }
    const changed = w.prunable ? 0 : await api.worktreeState(w.path).then((s) => s.uncommitted, () => 0);
    const branch = w.branch ? ` The branch ${w.branch} stays.` : "";
    const lost = changed ? ` Its ${changed} uncommitted ${changed === 1 ? "change" : "changes"} will be lost.` : "";
    const lock = w.inUse
      ? ` Something is working in it right now (${w.lockReason ?? "it holds the lock"}); deleting pulls the folder out from under it.`
      : w.locked
        ? ` It's locked${w.lockReason ? ` (${w.lockReason})` : ""}; this overrides the lock. The lock on its row unlocks it instead.`
        : "";
    const ok = await ask(
      w.prunable ? `${name}'s folder is gone, but it's locked${w.lockReason ? ` (${w.lockReason})` : ""}: its drive may only be unplugged. Prune it anyway?${branch}` : `Delete worktree ${name} and its folder?${lost}${lock}${branch}`,
      { title: w.prunable ? "Prune worktree" : "Remove worktree", kind: "warning", okLabel: w.prunable ? "Prune" : "Delete worktree" },
    );
    if (ok) await run("Remove worktree", () => api.removeWorktree(w.path, changed > 0 || w.locked), `Worktree ${name} removed`);
  };

  const unlockWorktree = async (w: Worktree) => {
    const name = folderName(w.path);
    const why = w.lockReason ?? "it holds the lock";
    const msg = `Something is working in ${name} right now (${why}). Unlocked, it can be pruned, moved or removed while that runs.`;
    if (w.inUse && !(await ask(msg, { title: "Unlock worktree", kind: "warning", okLabel: "Unlock" }))) return;
    await run("Unlock worktree", () => api.unlockWorktree(w.path), `Unlocked ${name}`);
  };

  return { busy, run, runNet, pull, sync, branchTerminal, deleteBranch, cleanUp, publish, publishTo, merge, push, pushAhead, switching, switchRemote, removeWorktree, unlockWorktree };
}
