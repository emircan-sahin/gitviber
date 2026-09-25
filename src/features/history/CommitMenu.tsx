import { ask } from "@tauri-apps/plugin-dialog";
import { ArrowDown, ArrowUp, Cherry, SearchCode, Copy, ExternalLink, Eye, EyeOff, FolderGit2, GitBranchPlus, GitCommitHorizontal, History, Link, Pencil, RotateCcw, Tag, Trash2, Undo2, UploadCloud } from "lucide-react";
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
import { copyText } from "@/lib/app/clipboard";
import { openWorktreeDialog } from "@/features/worktrees/WorktreeDialogs";
import { startBisect } from "./BisectBar";
import { type Actions, commitUrl, dropsPushed, MERGE_WARNING, PUSHED_WARNING } from "./commitActions";
import { groupRefs } from "./groupRefs";

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

  const undo = async () => {
    const drops = await dropsPushed(c.parents[0]);
    if (drops === null) return;
    const warnings = [...(c.parents.length > 1 ? [MERGE_WARNING] : []), ...(drops ? [PUSHED_WARNING] : [])];
    if (warnings.length && !(await ask(`Undo "${c.subject}"?\n\n${warnings.join("\n\n")}`, { title: "Undo commit", kind: "warning", okLabel: "Undo" }))) return;
    await run("Undo", () => api.undoCommit(c.sha), "Commit undone; its changes are staged");
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

  const checkout = async () => {
    const ok = await ask(`Check out ${short} without a branch (detached HEAD)? New commits made there belong to no branch until you create one.`, {
      title: "Checkout commit",
      kind: "warning",
      okLabel: "Checkout",
    });
    if (ok) await run("Checkout", () => api.checkoutCommit(c.sha), `Checked out ${short}`);
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

  // Set by the naming items: focus going back to the row would steal it from the name dialog.
  const naming = useRef(false);
  const name = (kind: "branch" | "tag") => {
    naming.current = true;
    actions.name(kind, c);
  };

  return (
    <ContextMenuContent
      onCloseAutoFocus={(e) => {
        if (naming.current) e.preventDefault();
        naming.current = false;
      }}
    >
      <ContextMenuItem disabled={locked || !head || !c.parents.length} onSelect={undo}>
        <Undo2 /> Undo commit
      </ContextMenuItem>
      <ContextMenuSub>
        {/* Only the branch's own commits: a commit HEAD lacks isn't its history to edit. */}
        <ContextMenuSubTrigger disabled={locked || c.notInHead}>
          <Pencil /> Edit history
        </ContextMenuSubTrigger>
        <ContextMenuSubContent>
          <ContextMenuItem onSelect={() => actions.message("reword", c)}>Reword…</ContextMenuItem>
          <ContextMenuItem disabled={!c.parents.length} onSelect={() => actions.message("squash", c)}>
            Squash into Parent…
          </ContextMenuItem>
          <ContextMenuItem disabled={!c.parents.length} onSelect={() => void actions.rewrite({ kind: "squash", sha: c.sha, message: null }, c)}>
            Fixup into Parent <span className="ml-auto pl-4 text-[11px] opacity-70">keeps its message</span>
          </ContextMenuItem>
          <ContextMenuSeparator />
          <ContextMenuItem disabled={head} onSelect={() => void actions.rewrite({ kind: "move", sha: c.sha, up: true }, c)}>
            <ArrowUp /> Move Up
          </ContextMenuItem>
          <ContextMenuItem disabled={!c.parents.length} onSelect={() => void actions.rewrite({ kind: "move", sha: c.sha, up: false }, c)}>
            <ArrowDown /> Move Down
          </ContextMenuItem>
          <ContextMenuSeparator />
          <ContextMenuItem className="text-destructive" onSelect={() => void actions.rewrite({ kind: "drop", sha: c.sha }, c)}>
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
      <ContextMenuSeparator />
      {/* HEAD has the bug, this commit didn't: git halves what's between until it finds where it came in. */}
      <ContextMenuItem disabled={locked || head || c.notInHead} onSelect={() => void startBisect(c.sha, actions.refresh)}>
        <SearchCode /> Find the Bad Commit Since Here…
      </ContextMenuItem>
      <ContextMenuItem disabled={locked || head} onSelect={checkout}>
        <GitCommitHorizontal /> Checkout commit
      </ContextMenuItem>
      <ContextMenuItem disabled={locked} onSelect={() => name("branch")}>
        <GitBranchPlus /> Create branch from here…
      </ContextMenuItem>
      {/* Not held back by an operation here: it's another worktree's checkout. */}
      <ContextMenuItem onSelect={() => openWorktreeDialog({ kind: "new", base: c.sha })}>
        <FolderGit2 /> New worktree from here…
      </ContextMenuItem>
      <ContextMenuItem disabled={locked} onSelect={() => name("tag")}>
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
