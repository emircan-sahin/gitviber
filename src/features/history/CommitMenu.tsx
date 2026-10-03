import { ask } from "@/lib/app/ask";
import { ArrowDown, ArrowUp, Cherry, Combine, SearchCode, Scissors, GitCommitVertical, Copy, ExternalLink, Eye, EyeOff, FileDiff, FolderGit2, GitBranchPlus, GitCommitHorizontal, GitCompare, GitCompareArrows, History, Link, Pencil, RotateCcw, SquareDashedMousePointer, Tag, Trash2, Undo2, UploadCloud } from "lucide-react";
import { Fragment, useRef, useState } from "react";
import {
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuLabel,
  ContextMenuSeparator,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
} from "@/components/ui/context-menu";
import { api, type Commit, errorMessage, type RemoteTags, type ResetMode } from "@/lib/api";
import { forgetRemoteTags, remoteTags } from "@/lib/repo/remoteTags";
import { folderName } from "@/lib/path";
import { copyLink, openOnGitHub } from "@/lib/github/url";
import { copyLater, copyText } from "@/lib/app/clipboard";
import { openWorktreeDialog } from "@/features/worktrees/WorktreeDialogs";
import { startBisect } from "./BisectBar";
import { type Actions, checkoutDetached, commitUrl, dropsPushed, MERGE_WARNING, PUSHED_WARNING, undoCommit } from "./commitActions";
import { groupRefs } from "./groupRefs";
import { commitMark } from "@/lib/repo/compareMark";

/** The remote tags are pushed to and the ones it has, while asking it, or why that failed. */
type TagsThere = RemoteTags | { error: string } | "loading" | null;

/** A tag on a commit: push it, delete it here or on the remote. */
function TagMenu({ tag, remote, onOpen, actions }: { tag: string; remote: TagsThere; onOpen: () => void; actions: Actions }) {
  const { locked, run, runNet } = actions;
  const known = remote && typeof remote === "object" && "names" in remote ? remote : null;
  const there = known?.names.includes(tag);
  const where = known?.remote ?? "the remote";
  const deleteRemote = async () => {
    const ok = await ask(`Delete tag ${tag} from ${where}? Clones that fetched it keep their copy, and GitViber can't undo this. The tag here stays.`, {
      title: "Delete remote tag",
      kind: "warning",
      okLabel: "Delete",
    });
    if (ok) await runNet("Delete remote tag", (op) => api.deleteRemoteTag(tag, op).then(forgetRemoteTags), `Deleted ${tag} from ${where}`);
  };
  return (
    <ContextMenuSub onOpenChange={(o) => o && onOpen()}>
      <ContextMenuSubTrigger>
        <Tag /> <span className="max-w-48 truncate font-mono">{tag}</span>
      </ContextMenuSubTrigger>
      <ContextMenuSubContent>
        <ContextMenuLabel className="normal-case">
          {remote === "loading" || remote === null ? "Checking the remote…" : known ? (there ? `On ${where}` : `Not on ${where} yet`) : `Couldn't ask the remote: ${"error" in remote ? remote.error : ""}`}
        </ContextMenuLabel>
        <ContextMenuItem disabled={there} onSelect={() => runNet("Push tag", (op) => api.pushTags([tag], op).then(forgetRemoteTags), `Pushed tag ${tag}`)}>
          <UploadCloud /> Push tag{known ? ` to ${where}` : ""}
        </ContextMenuItem>
        <ContextMenuItem disabled={locked} onSelect={() => run("Delete tag", () => api.deleteTag(tag), `Deleted tag ${tag}`)}>
          <Trash2 /> Delete tag
        </ContextMenuItem>
        <ContextMenuItem disabled={known ? !there : false} className="text-destructive" onSelect={deleteRemote}>
          <Trash2 /> Delete from {where}…
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem onSelect={() => copyText(tag, "Tag name copied")}>
          <Copy /> Copy name
        </ContextMenuItem>
      </ContextMenuSubContent>
    </ContextMenuSub>
  );
}

/** Right-click actions on a commit. `head`: the first row, i.e. the checked-out commit. */
export function CommitMenu({ commit: c, head, actions }: { commit: Commit; head: boolean; actions: Actions }) {
  const { status, headSha, webUrl, locked, run, refMenu } = actions;
  // The badges shown; a detached HEAD's names no ref to hide.
  const graphRefs = refMenu ? groupRefs(c.refs, actions.remotes, actions.showRefs).filter((r) => r.refs.length) : [];
  const url = commitUrl(c, actions);
  const short = c.shortSha;
  const target = status?.branch ?? "HEAD";
  const root = status?.root;
  const mark = commitMark.use(root);
  const point = { sha: c.sha, label: short };
  const compare = actions.comparePoints;

  const undo = async () => {
    const drops = await dropsPushed(c.parents[0]);
    if (drops === null) return;
    const warnings = [...(c.parents.length > 1 ? [MERGE_WARNING] : []), ...(drops ? [PUSHED_WARNING] : [])];
    if (warnings.length && !(await ask(`Undo "${c.subject}"?\n\n${warnings.join("\n\n")}`, { title: "Undo commit", kind: "warning", okLabel: "Undo" }))) return;
    await undoCommit(c.sha, run);
  };

  const reset = async (mode: ResetMode) => {
    const drops = await dropsPushed(c.sha);
    if (drops === null) return;
    const lines = [`Move ${target} to ${short}?`];
    if (mode === "hard") lines.push("Uncommitted changes to tracked files are discarded, and commits after this one leave the branch. This cannot be undone from GitViber.");
    if (drops) lines.push(PUSHED_WARNING);
    if ((mode === "hard" || drops) && !(await ask(lines.join("\n\n"), { title: `${mode[0].toUpperCase()}${mode.slice(1)} reset`, kind: "warning", okLabel: "Reset" }))) return;
    await run("Reset", () => api.reset(c.sha, mode, headSha), `${target} reset to ${short}`);
  };

  // Which tags the remote has, asked when a tag's submenu opens (reused for a few seconds).
  // Only the latest opening's answer is shown.
  const tags = c.refs.filter((r) => r.startsWith("tag: ")).map((r) => r.slice(5));
  const [remote, setRemote] = useState<TagsThere>(null);
  const asked = useRef(0);
  const checkRemote = () => {
    const id = ++asked.current;
    setRemote("loading");
    const show = (r: TagsThere) => id === asked.current && setRemote(r);
    remoteTags().then(show, (e) => show({ error: errorMessage(e) }));
  };

  return (
    <ContextMenuContent>
      <ContextMenuItem disabled={locked || !head || !c.parents.length} onSelect={undo}>
        <Undo2 /> Undo commit
      </ContextMenuItem>
      <ContextMenuSub>
        {/* Only the branch's own commits: a commit HEAD lacks isn't its history to edit. */}
        <ContextMenuSubTrigger disabled={locked || c.notInHead}>
          <Pencil /> Edit history
        </ContextMenuSubTrigger>
        <ContextMenuSubContent>
          <ContextMenuItem onSelect={() => actions.reword(c)}>Reword…</ContextMenuItem>
          <ContextMenuItem disabled={!c.parents.length} onSelect={() => actions.squash([c], c.parents[0], true)}>
            Squash into Parent…
          </ContextMenuItem>
          <ContextMenuItem disabled={!c.parents.length} onSelect={() => actions.squash([c], c.parents[0], false)}>
            Fixup into Parent <span className="ml-auto pl-4 text-[11px] opacity-70">keeps its message</span>
          </ContextMenuItem>
          <ContextMenuSeparator />
          {/* Its changes come back unstaged on its parent, to commit in pieces; a merge or the first commit has none to take apart. */}
          <ContextMenuItem disabled={c.parents.length !== 1} onSelect={() => void actions.rewrite({ kind: "split", sha: c.sha }, [c])}>
            <Scissors /> Split Commit…
          </ContextMenuItem>
          <ContextMenuItem disabled={!status?.staged.length} onSelect={() => void actions.rewrite({ kind: "fixupStaged", sha: c.sha }, [c])}>
            <GitCommitVertical /> Fixup Staged Changes into This Commit
          </ContextMenuItem>
          <ContextMenuSeparator />
          <ContextMenuItem disabled={head} onSelect={() => void actions.rewrite({ kind: "move", sha: c.sha, up: true }, [c])}>
            <ArrowUp /> Move Up
          </ContextMenuItem>
          <ContextMenuItem disabled={!c.parents.length} onSelect={() => void actions.rewrite({ kind: "move", sha: c.sha, up: false }, [c])}>
            <ArrowDown /> Move Down
          </ContextMenuItem>
          <ContextMenuSeparator />
          {/* The only commit: dropping it would leave the branch with none. */}
          <ContextMenuItem disabled={head && !c.parents.length} className="text-destructive" onSelect={() => void actions.rewrite({ kind: "drop", shas: [c.sha] }, [c])}>
            <Trash2 /> Drop Commit…
          </ContextMenuItem>
        </ContextMenuSubContent>
      </ContextMenuSub>
      {/* Reverting a commit HEAD never had would apply the opposite of a change that isn't there. */}
      <ContextMenuItem disabled={locked || c.notInHead} onSelect={() => run("Revert", () => api.revert(c.sha), `Reverted ${short}`)}>
        <RotateCcw /> Revert commit
      </ContextMenuItem>
      {/* Only a commit HEAD lacks (a fork's original lists those): picking one it has changes nothing. */}
      {c.notInHead && (
        <ContextMenuItem disabled={locked} onSelect={() => run("Cherry-pick", () => api.cherryPick(c.sha), `Cherry-picked ${short} onto ${target}`)}>
          <Cherry /> Cherry-pick onto {target}
        </ContextMenuItem>
      )}
      {actions.pickTargets.length > 0 && (
        <ContextMenuSub>
          {/* Another worktree's lock is its own: only this one's action in progress holds it back. */}
          <ContextMenuSubTrigger disabled={actions.locked && !actions.status?.operation}>
            <Cherry /> Cherry-pick onto
          </ContextMenuSubTrigger>
          <ContextMenuSubContent>
            {actions.pickTargets.map((w) => (
              <ContextMenuItem key={w.path} onSelect={() => actions.pickInto(w, c)}>
                <span className="font-mono">{w.branch}</span>
                <span className="ml-auto pl-4 text-[11px] opacity-70">{folderName(w.path)}</span>
              </ContextMenuItem>
            ))}
          </ContextMenuSubContent>
        </ContextMenuSub>
      )}
      <ContextMenuSub>
        <ContextMenuSubTrigger disabled={locked}>
          <History /> Reset {target} to here
        </ContextMenuSubTrigger>
        <ContextMenuSubContent>
          <ContextMenuItem onSelect={() => reset("soft")}>Soft · keep changes staged</ContextMenuItem>
          <ContextMenuItem onSelect={() => reset("mixed")}>Mixed · keep changes unstaged</ContextMenuItem>
          <ContextMenuItem className="text-destructive" onSelect={() => reset("hard")}>
            Hard · discard changes
          </ContextMenuItem>
        </ContextMenuSubContent>
      </ContextMenuSub>
      {compare && root && (
        <>
          <ContextMenuSeparator />
          <ContextMenuItem onSelect={() => compare({ base: point, head: null })}>
            <GitCompareArrows /> Compare with Working Tree
          </ContextMenuItem>
          <ContextMenuItem onSelect={() => commitMark.set({ ...point, root })}>
            <SquareDashedMousePointer /> Select for Compare
          </ContextMenuItem>
          {mark && mark.sha !== c.sha && (
            <ContextMenuItem onSelect={() => compare({ base: mark, head: point })}>
              <GitCompare /> Compare with <span className="font-mono">{mark.label}</span>
            </ContextMenuItem>
          )}
        </>
      )}
      <ContextMenuSeparator />
      {/* HEAD has the bug, this commit didn't: git halves what's between until it finds where it came in. */}
      <ContextMenuItem disabled={locked || head || c.notInHead} onSelect={() => void startBisect(c.sha, actions.refresh)}>
        <SearchCode /> Find the Bad Commit Since Here…
      </ContextMenuItem>
      <ContextMenuItem disabled={locked || head} onSelect={() => checkoutDetached(c, run)}>
        <GitCommitHorizontal /> Checkout commit
      </ContextMenuItem>
      <ContextMenuItem disabled={locked} keepFocus onSelect={() => actions.name("branch", c)}>
        <GitBranchPlus /> Create branch from here…
      </ContextMenuItem>
      {/* Not held back by an operation here: it's another worktree's checkout. */}
      <ContextMenuItem onSelect={() => openWorktreeDialog({ kind: "new", base: c.sha })}>
        <FolderGit2 /> New worktree from here…
      </ContextMenuItem>
      <ContextMenuItem disabled={locked} keepFocus onSelect={() => actions.name("tag", c)}>
        <Tag /> Create tag here…
      </ContextMenuItem>
      {tags.map((t) => (
        <TagMenu key={t} tag={t} remote={remote} onOpen={checkRemote} actions={actions} />
      ))}
      {refMenu && graphRefs.length > 0 && (
        <>
          <ContextMenuSeparator />
          {graphRefs.map((r) => (
            <Fragment key={r.refs[0]}>
              <ContextMenuItem onSelect={() => refMenu.hide(r.refs)}>
                <EyeOff /> Hide <span className="font-mono">{r.name}</span> in graph
              </ContextMenuItem>
              {r.kind !== "tag" && (
                <ContextMenuItem onSelect={() => refMenu.only(r.refs[0])}>
                  <Eye /> Show only <span className="font-mono">{r.name}</span>
                </ContextMenuItem>
              )}
            </Fragment>
          ))}
        </>
      )}
      <ContextMenuSeparator />
      <ContextMenuItem onSelect={() => copyText(c.sha, "SHA copied")}>
        <Copy /> Copy SHA
      </ContextMenuItem>
      <ContextMenuItem onSelect={() => copyText(short, "Short SHA copied")}>
        <Copy /> Copy short SHA
      </ContextMenuItem>
      <ContextMenuItem onSelect={() => copyText(c.body ? `${c.subject}\n\n${c.body}` : c.subject, "Message copied")}>
        <Copy /> Copy message
      </ContextMenuItem>
      <ContextMenuItem disabled={c.parents.length > 1} onSelect={() => copyLater(() => api.commitPatch(c.sha), "Patch copied")}>
        <FileDiff /> Copy as Patch
      </ContextMenuItem>
      {webUrl && (
        <>
          <ContextMenuItem disabled={!url} onSelect={() => url && copyLink(url)}>
            <Link /> Copy link
          </ContextMenuItem>
          <ContextMenuItem disabled={!url} onSelect={() => url && openOnGitHub(url)}>
            <ExternalLink /> Open on GitHub
          </ContextMenuItem>
        </>
      )}
    </ContextMenuContent>
  );
}

/** Right-click actions on several commits picked together (newest first, as listed); `all`: every commit of the branch. */
export function PickedMenu({ commits, all, actions }: { commits: Commit[]; all: boolean; actions: Actions }) {
  const n = commits.length;
  const oldest = commits[n - 1];
  // Only the branch's own commits, as for one.
  const off = actions.locked || commits.some((c) => c.notInHead);
  // Only commits HEAD lacks: picking one it has changes nothing.
  const missing = commits.filter((c) => c.notInHead);
  const target = actions.status?.branch ?? "HEAD";
  return (
    <ContextMenuContent>
      <ContextMenuLabel className="normal-case">{n} commits</ContextMenuLabel>
      {missing.length > 0 && (
        <>
          <ContextMenuItem disabled={actions.locked} onSelect={() => void actions.pickMany(missing)}>
            <Cherry /> Cherry-pick {missing.length} Commits onto {target}
          </ContextMenuItem>
          <ContextMenuSeparator />
        </>
      )}
      <ContextMenuItem disabled={off} onSelect={() => actions.squash(commits, oldest.sha, true)}>
        <Combine /> Squash {n} Commits…
      </ContextMenuItem>
      <ContextMenuItem disabled={off} onSelect={() => actions.squash(commits, oldest.sha, false)}>
        <Combine /> Fixup into Oldest <span className="ml-auto pl-4 text-[11px] opacity-70">keeps its message</span>
      </ContextMenuItem>
      <ContextMenuSeparator />
      <ContextMenuItem disabled={off || all} className="text-destructive" onSelect={() => void actions.rewrite({ kind: "drop", shas: commits.map((c) => c.sha) }, commits)}>
        <Trash2 /> Drop {n} Commits…
      </ContextMenuItem>
      {n === 2 && actions.comparePoints && (
        <>
          <ContextMenuSeparator />
          {/* Newest first: the older one is the old side. */}
          <ContextMenuItem onSelect={() => actions.comparePoints?.({ base: { sha: commits[1].sha, label: commits[1].shortSha }, head: { sha: commits[0].sha, label: commits[0].shortSha } })}>
            <GitCompare /> Compare These Two
          </ContextMenuItem>
        </>
      )}
      <ContextMenuSeparator />
      <ContextMenuItem onSelect={() => copyText(commits.map((c) => c.sha).join("\n"), "SHAs copied")}>
        <Copy /> Copy SHAs
      </ContextMenuItem>
    </ContextMenuContent>
  );
}
