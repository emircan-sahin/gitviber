import { ask } from "@tauri-apps/plugin-dialog";
import { Cherry, Cloud, Copy, ExternalLink, Eye, EyeOff, GitBranchPlus, GitCommitHorizontal, History, Link, RotateCcw, ShieldAlert, ShieldCheck, ShieldX, Tag, Trash2, Undo2, UploadCloud } from "lucide-react";
import { Fragment, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuLabel,
  ContextMenuSeparator,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Tip } from "@/components/ui/tooltip";
import { api, CANCELLED, type Commit, type CommitDetails, errorMessage, type FileChange, type RemoteTags, type RepoStatus, type ResetMode, type Worktree } from "@/lib/api";
import { type GraphRow, type Lane, graphRows } from "@/lib/commitGraph";
import { matchesCommand } from "@/lib/keybindings";
import { pointerMoved } from "@/lib/pointer";
import { withNetActivity } from "@/lib/netActivity";
import { forgetRemoteTags, remoteTags } from "@/lib/remoteTags";
import { type Selection, selectionKey } from "@/lib/selection";
import { toast } from "@/lib/toast";
import { tracked, undoAction } from "@/lib/undo";
import { useListNav } from "@/lib/useListNav";
import { cn, relativeTime } from "@/lib/utils";
import { folderName } from "@/lib/worktrees";
import { FileIcon } from "./FileIcon";
import { copyLink, openOnGitHub } from "./PullsPanel";
import { LineCounts, PathLabel, StatusLetter } from "./StatusBadge";

interface Props {
  commits: Commit[];
  status: RepoStatus | null;
  /** Remote-tracking branch names (origin/main…), to group decorations. */
  remotes: Set<string>;
  hasMore: boolean;
  loadMore: () => Promise<void>;
  refresh: () => Promise<void>;
  activeKey: string | null;
  onOpen: (s: Selection, pin?: boolean) => void;
  onHover: (s: Selection) => void;
  /** HEAD, when the list is another branch's history (a fork's original) rather than HEAD's. */
  headSha?: string;
  /** Where these commits live on GitHub, when that's not origin: a fork's original has them all. */
  web?: string;
  /** What an empty list says. */
  empty?: string;
  /** Draw branches and merges; off for search results, whose neighbours aren't parent and child. */
  graph?: boolean;
  /** Blame's link: open this commit and this file in it, once per `id` (each click is new). */
  reveal?: Reveal | null;
  /** This repo's worktrees: a commit can be picked onto the branch checked out in another. */
  worktrees?: Worktree[];
  /** Opens a worktree in this window. */
  onOpenRepo?: (path: string) => void;
  /** Every branch is listed: HEAD's keeps the first lane even below newer branches' tips. */
  pinHead?: boolean;
  /** Scroll to this commit and open it, once per `id`. */
  jump?: Jump | null;
  /** The all-branches graph's choices, offered on the refs a commit is decorated with. */
  refMenu?: RefMenu;
}

/** Blame's link to a commit: open it and `path` (the file's name there). `id` is new per click. */
export interface Reveal {
  sha: string;
  path: string;
  id: number;
}

export interface Jump {
  sha: string;
  id: number;
}

/** Refs are full names (refs/heads/…). */
export interface RefMenu {
  hide: (refs: string[]) => void;
  only: (ref: string) => void;
}

/** What a commit's context menu needs from the panel. */
interface Actions {
  status: RepoStatus | null;
  /** HEAD as the history shows it; the backend refuses to move HEAD if it has changed since. */
  headSha: string;
  webUrl: string | null;
  /** An action is running or a merge/rebase/revert waits: nothing else may move HEAD. */
  locked: boolean;
  run: (label: string, fn: () => Promise<void | boolean>, done: string) => Promise<void>;
  name: (kind: "branch" | "tag", commit: Commit) => void;
  /** `webUrl` has every listed commit, not only those reached from origin's branches. */
  everyOnWeb: boolean;
  /** Other worktrees with a branch checked out, to cherry-pick onto. */
  pickTargets: Worktree[];
  pickInto: (w: Worktree, commit: Commit) => Promise<void>;
  remotes: Set<string>;
  refMenu?: RefMenu;
}

/** GitHub only has commits that reached one of origin's branches (or all, for a fork's original). */
const commitUrl = (c: Commit, { webUrl, everyOnWeb }: Pick<Actions, "webUrl" | "everyOnWeb">) =>
  webUrl && (c.onOrigin || everyOnWeb) ? `${webUrl}/commit/${c.sha}` : undefined;

export function HistoryPanel({ commits, status, remotes, hasMore, loadMore, refresh, activeKey, onOpen, onHover, headSha, web, empty = "No commits yet.", graph = true, reveal = null, worktrees = [], onOpenRepo, pinHead = false, jump = null, refMenu }: Props) {
  const [open, setOpen] = useState<string | null>(reveal?.sha ?? null);
  useEffect(() => {
    if (reveal) setOpen(reveal.sha);
  }, [reveal]);
  const scroller = useRef<HTMLDivElement>(null);
  const anchor = useRef<{ el: HTMLElement; top: number } | null>(null);
  const [busy, setBusy] = useState(false);
  const [webUrl, setWebUrl] = useState<string | null>(null);
  const [naming, setNaming] = useState<{ kind: "branch" | "tag"; commit: Commit } | null>(null);

  // `remotes` is rebuilt on every git refresh, including the one `git remote set-url` causes.
  useEffect(() => {
    api.githubWebUrl().then(setWebUrl, () => setWebUrl(null));
  }, [remotes]);

  // Operations that can stop on conflicts resolve to true; Workspace then brings Changes into view.
  const run = async (label: string, fn: () => Promise<void | boolean>, done: string) => {
    setBusy(true);
    try {
      const [stopped, entry] = await tracked(fn);
      if (stopped) toast("info", `${label} stopped on conflicts`, "Resolve them in Changes, then continue.");
      else toast("success", done, undefined, undoAction(entry, refresh));
    } catch (e) {
      if (e === CANCELLED) toast("info", `${label} cancelled`);
      else toast("error", `${label} failed`, errorMessage(e));
    } finally {
      setBusy(false);
      await refresh();
    }
  };

  // git runs in that worktree, and the entry lands in its undo history, not this one's. A pick
  // stopped on conflicts waits there, for its own Changes panel to finish.
  const pickInto = async (w: Worktree, c: Commit) => {
    const branch = w.branch ?? folderName(w.path);
    const where = folderName(w.path);
    const go = onOpenRepo && { label: "Switch to worktree", run: () => onOpenRepo(w.path) };
    setBusy(true);
    try {
      if (await api.cherryPickInto(w.path, c.sha)) toast("info", `Cherry-pick onto ${branch} stopped on conflicts`, `It waits in ${where}: switch there to resolve them and continue.`, go);
      else toast("success", `Cherry-picked ${c.shortSha} onto ${branch}`, `In ${where}; undo it from there.`, go);
    } catch (e) {
      toast("error", `Cherry-pick onto ${branch} failed`, errorMessage(e));
    } finally {
      setBusy(false);
      await refresh();
    }
  };

  // HEAD's own history starts at the HEAD the user sees.
  const head = headSha ?? commits[0]?.sha ?? "";
  const actions: Actions = {
    status,
    headSha: head,
    webUrl: web ?? webUrl,
    locked: busy || !!status?.operation,
    run,
    name: (kind, commit) => setNaming({ kind, commit }),
    everyOnWeb: !!web,
    pickTargets: worktrees.filter((w) => !w.current && !w.bare && !w.prunable && w.branch),
    pickInto,
    remotes,
    refMenu,
  };

  // Opening a commit collapses the one above it; WebKit has no scroll anchoring, so without
  // this the clicked row jumps up by the collapsed file list, often out of view.
  useLayoutEffect(() => {
    const a = anchor.current;
    anchor.current = null;
    if (a && scroller.current) scroller.current.scrollTop += a.el.getBoundingClientRect().top - a.top;
  }, [open]);

  const toggle = (sha: string, el: HTMLElement) => {
    anchor.current = { el, top: el.getBoundingClientRect().top };
    setOpen(open === sha ? null : sha);
  };

  // Without the graph, one line joins each row to the next.
  const rows = useMemo(() => {
    if (!graph) return graphRows(commits.map((c, i) => ({ sha: c.sha, parents: i + 1 < commits.length ? [commits[i + 1].sha] : [] })));
    return graphRows(commits, pinHead && commits.some((c) => c.sha === head) ? head : undefined);
  }, [commits, graph, pinHead, head]);

  // A jump waits for its commit to be listed: the caller loads pages until it is.
  const jumped = useRef<number | null>(null);
  useEffect(() => {
    if (!jump || jumped.current === jump.id) return;
    const row = scroller.current?.querySelector(`[data-row="commit:${jump.sha}"]`);
    if (!row) return;
    jumped.current = jump.id;
    row.scrollIntoView({ block: "center" });
    setOpen(jump.sha);
  }, [jump, commits]);
  // Hovering a commit brings its branch forward, and a merge's merged-in one. Only the lines
  // involved change: rewriting a stylesheet restyled the whole app on each row a scroll carried by.
  const lit = useRef<{ row: GraphRow; lines: Element[] } | null>(null);
  const light = (row: GraphRow | null) => {
    const list = scroller.current;
    if (!list || (lit.current?.row ?? null) === row) return;
    for (const el of lit.current?.lines ?? []) el.removeAttribute("data-lit");
    const ids = row ? new Set([row.id, ...row.out.map((l) => l.id)]) : [];
    const lines = [...ids].flatMap((id) => [...list.querySelectorAll(`[data-lane="${id}"]`)]);
    for (const el of lines) el.setAttribute("data-lit", "");
    lit.current = row ? { row, lines } : null;
    list.toggleAttribute("data-dim", !!row);
  };
  // Rows scrolling under a still pointer get a mousemove too; only a real move lights a row.
  const point = (row: GraphRow, e: React.MouseEvent) => pointerMoved(e) && light(row);

  const more = () => loadMore().catch((e) => toast("error", "Could not load history", errorMessage(e)));
  const nav = useListNav({ activeKey, loadMore: hasMore ? more : null });

  if (!commits.length) {
    return <div className="px-6 pt-20 text-center text-[12px] text-subtle">{empty}</div>;
  }

  return (
    <div
      ref={scroller}
      onScroll={() => light(null)}
      onMouseLeave={() => light(null)}
      className="h-full overflow-x-hidden overflow-y-auto py-1 [&[data-dim]_[data-lane]:not([data-lit])]:opacity-25"
    >
      <div role="tree" aria-label="History" {...nav}>
      {commits.map((c, i) => (
        <CommitRow
          key={c.sha}
          commit={c}
          remotes={remotes}
          graph={rows[i]}
          isHead={c.sha === head}
          onPoint={point}
          open={open === c.sha}
          reveal={reveal?.sha === c.sha ? reveal : null}
          onToggle={(el) => toggle(c.sha, el)}
          activeKey={activeKey}
          onOpen={onOpen}
          onHover={onHover}
          url={commitUrl(c, actions)}
          menu={<CommitMenu commit={c} head={c.sha === head} actions={actions} />}
        />
      ))}
      </div>
      {hasMore && (
        <div className="p-2">
          <Button variant="secondary" size="sm" className="w-full" onClick={more}>
            Load more
          </Button>
        </div>
      )}
      {naming && <NameDialog {...naming} onClose={() => setNaming(null)} run={run} />}
    </div>
  );
}

const PUSHED_WARNING = "Some of these commits are already pushed, so you'd have to force-push, which rewrites history for everyone else on this branch.";
const MERGE_WARNING = "This is a merge: the merged-in commits leave the branch too, and all of their changes end up staged together.";

/** Asked at click time: only ancestry, not log order, tells which pushed commits a move drops. */
async function dropsPushed(sha: string) {
  try {
    return await api.dropsPushed(sha);
  } catch (e) {
    toast("error", "Could not compare with the upstream", errorMessage(e));
    return null;
  }
}

const copy = (text: string, what: string) =>
  navigator.clipboard.writeText(text).then(
    () => toast("success", what),
    (e) => toast("error", "Could not copy", errorMessage(e)),
  );

/** The remote tags are pushed to and the ones it has, while asking it, or why that failed. */
type TagsThere = RemoteTags | { error: string } | "loading" | null;

/** A tag on a commit: push it, delete it here or on the remote. */
function TagMenu({ tag, remote, onOpen, actions }: { tag: string; remote: TagsThere; onOpen: () => void; actions: Actions }) {
  const { locked, run } = actions;
  const known = remote && typeof remote === "object" && "names" in remote ? remote : null;
  const there = known?.names.includes(tag);
  const where = known?.remote ?? "the remote";
  const deleteRemote = async () => {
    const ok = await ask(`Delete tag ${tag} from ${where}? Clones that fetched it keep their copy, and GitViber can't undo this. The tag here stays.`, {
      title: "Delete remote tag",
      kind: "warning",
      okLabel: "Delete",
    });
    if (ok) await run("Delete remote tag", () => withNetActivity("Delete remote tag", (op) => api.deleteRemoteTag(tag, op).then(forgetRemoteTags)), `Deleted ${tag} from ${where}`);
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
        <ContextMenuItem disabled={there} onSelect={() => run("Push tag", () => withNetActivity("Push tag", (op) => api.pushTags([tag], op).then(forgetRemoteTags)), `Pushed tag ${tag}`)}>
          <UploadCloud /> Push tag{known ? ` to ${where}` : ""}
        </ContextMenuItem>
        <ContextMenuItem disabled={locked} onSelect={() => run("Delete tag", () => api.deleteTag(tag), `Deleted tag ${tag}`)}>
          <Trash2 /> Delete tag
        </ContextMenuItem>
        <ContextMenuItem disabled={known ? !there : false} className="text-destructive" onSelect={deleteRemote}>
          <Trash2 /> Delete from {where}…
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem onSelect={() => copy(tag, "Tag name copied")}>
          <Copy /> Copy name
        </ContextMenuItem>
      </ContextMenuSubContent>
    </ContextMenuSub>
  );
}

/** Right-click actions on a commit. `head`: the first row, i.e. the checked-out commit. */
function CommitMenu({ commit: c, head, actions }: { commit: Commit; head: boolean; actions: Actions }) {
  const { status, headSha, webUrl, locked, run, refMenu } = actions;
  // A detached HEAD's badge names no ref to hide.
  const graphRefs = refMenu ? groupRefs(c.refs, actions.remotes).filter((r) => r.refs.length) : [];
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
      <ContextMenuItem disabled={locked || head} onSelect={checkout}>
        <GitCommitHorizontal /> Checkout commit
      </ContextMenuItem>
      <ContextMenuItem disabled={locked} onSelect={() => name("branch")}>
        <GitBranchPlus /> Create branch from here…
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
      <ContextMenuItem onSelect={() => copy(c.sha, "SHA copied")}>
        <Copy /> Copy SHA
      </ContextMenuItem>
      <ContextMenuItem onSelect={() => copy(short, "Short SHA copied")}>
        <Copy /> Copy short SHA
      </ContextMenuItem>
      <ContextMenuItem onSelect={() => copy(c.body ? `${c.subject}\n\n${c.body}` : c.subject, "Message copied")}>
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

function NameDialog({ kind, commit, onClose, run }: { kind: "branch" | "tag"; commit: Commit; onClose: () => void; run: Actions["run"] }) {
  const [name, setName] = useState("");
  const [message, setMessage] = useState("");
  const submit = () => {
    const n = name.trim();
    onClose();
    if (kind === "branch") run("Create branch", () => api.createBranchAt(n, commit.sha), `Switched to new branch ${n}`);
    else run("Create tag", () => api.createTag(n, commit.sha, message), `Tagged ${commit.shortSha} as ${n}`);
  };
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogTitle>{kind === "branch" ? "Create branch" : "Create tag"}</DialogTitle>
        <DialogDescription>
          At <span className="font-mono">{commit.shortSha}</span> {commit.subject}
          {kind === "branch" && ". You'll be switched to it; uncommitted changes come along."}
        </DialogDescription>
        <form
          className="mt-4 flex flex-wrap gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (name.trim()) submit();
          }}
        >
          <Input autoFocus className="min-w-0 flex-1" value={name} onChange={(e) => setName(e.target.value)} placeholder={kind === "branch" ? "Branch name" : "Tag name, e.g. v1.2.0"} spellCheck={false} />
          <Button type="submit" disabled={!name.trim()}>
            Create
          </Button>
          {kind === "tag" && (
            <Textarea
              value={message}
              onChange={(e) => setMessage(e.target.value)}
              // ⌘↵ submits from here too; a plain ↵ is a new line.
              onKeyDown={(e) => {
                if (matchesCommand("git.createTag", e.nativeEvent) && name.trim()) {
                  e.preventDefault();
                  submit();
                }
              }}
              rows={3}
              placeholder="Message (optional): makes an annotated tag, which Push with tags sends along"
            />
          )}
        </form>
      </DialogContent>
    </Dialog>
  );
}

function CommitRow({
  commit,
  remotes,
  graph,
  isHead,
  onPoint,
  open,
  reveal,
  onToggle,
  activeKey,
  onOpen,
  onHover,
  url,
  menu,
}: {
  commit: Commit;
  remotes: Set<string>;
  graph: GraphRow;
  isHead: boolean;
  onPoint: (row: GraphRow, e: React.MouseEvent) => void;
  open: boolean;
  reveal: Reveal | null;
  onToggle: (row: HTMLElement) => void;
  activeKey: string | null;
  onOpen: (s: Selection, pin?: boolean) => void;
  onHover: (s: Selection) => void;
  url: string | undefined;
  menu: React.ReactNode;
}) {
  const [files, setFiles] = useState<FileChange[] | null>(null);
  const revealed = useRef<number | null>(null);

  useEffect(() => {
    if (!open || files) return;
    let alive = true;
    api
      .commitFiles(commit.sha)
      .then((f) => {
        if (!alive) return;
        setFiles(f);
        // A blame click waiting on these files opens its own file (below).
        if (reveal && revealed.current !== reveal.id) return;
        // Jump straight into the file (a file's history: that one) so one click shows code.
        const file = f.find((x) => x.path === commit.file) ?? f[0];
        if (file) onOpen({ kind: "commit", commit, file, url });
      })
      .catch((e) => alive && toast("error", "Could not load commit", errorMessage(e)));
    // Collapsing (or opening another commit) cancels the auto-open of a late reply.
    return () => {
      alive = false;
    };
  }, [open, files, commit, url, onOpen, reveal]);

  // Each blame click opens its file, also on a row that's open already or was loaded before.
  useEffect(() => {
    if (!reveal || !open || !files || revealed.current === reveal.id) return;
    revealed.current = reveal.id;
    const file = files.find((x) => x.path === reveal.path) ?? files[0];
    if (file) onOpen({ kind: "commit", commit, file, url });
  }, [reveal, open, files, commit, url, onOpen]);

  const add = files?.reduce((n, f) => n + (f.additions ?? 0), 0) ?? 0;
  const del = files?.reduce((n, f) => n + (f.deletions ?? 0), 0) ?? 0;
  const mark = commit.unpushed ? "Not pushed yet" : commit.notInHead ? "Not in your branch yet" : null;
  const dotLabel = isHead ? ["HEAD", mark].filter(Boolean).join(" · ") : (mark ?? undefined);

  return (
    <div className="relative" onMouseMove={(e) => onPoint(graph, e)}>
      <GraphLines row={graph} />
      <ContextMenu>
        <ContextMenuTrigger asChild>
          <div
            role="treeitem"
            aria-level={1}
            aria-expanded={open}
            tabIndex={-1}
            data-row={`commit:${commit.sha}`}
            onClick={(e) => onToggle(e.currentTarget)}
            className={cn(
              "relative flex cursor-pointer items-start gap-2.5 py-1.5 pr-2 pl-3 outline-none focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset data-[state=open]:bg-hover",
              open ? "bg-active" : "hover:bg-hover focus:bg-hover",
            )}
          >
            <span
              className={cn(
                "relative z-10 mt-[3px] size-[9px] shrink-0 rounded-full border-2",
                commit.unpushed ? "border-primary bg-primary" : commit.notInHead ? "border-added bg-added" : "border-subtle bg-sidebar",
                // Where you are, among every branch's commits.
                isHead && "outline-2 outline-offset-1 outline-primary",
              )}
              title={dotLabel}
              aria-label={dotLabel}
              role={dotLabel ? "img" : undefined}
              // In its lane and its colour, with the text after the row's last lane.
              style={{
                marginLeft: laneOf(graph.col) * LANE,
                marginRight: (lanesOf(graph) - 1 - laneOf(graph.col)) * LANE,
                borderColor: commit.unpushed || commit.notInHead || !graph.id ? undefined : laneColor(graph.id),
              }}
            />
            <div className="min-w-0 flex-1">
              <div className={cn("truncate text-[12px] leading-4", open ? "font-medium text-foreground" : "text-foreground/90")}>{commit.subject}</div>
              <div className="mt-0.5 flex min-w-0 items-center gap-1.5 text-[10.5px] text-subtle">
                <span className="min-w-0 truncate">{commit.authorName}</span>
                <span>·</span>
                <CommitTime commit={commit} />
                <span className="ml-auto shrink-0 font-mono">{commit.shortSha}</span>
              </div>
              <RefBadges refs={commit.refs} remotes={remotes} />
            </div>
          </div>
        </ContextMenuTrigger>
        {menu}
      </ContextMenu>
      {open && (
        <div className="relative border-y border-border bg-panel py-0.5">
          {!files && <div className="py-1 pl-8 text-[11.5px] text-subtle">Loading…</div>}
          {files && (
            <div className="flex h-6 items-center gap-2 pr-2 pl-8 text-[10.5px] text-subtle">
              <span className="font-semibold tracking-[0.08em] uppercase">Changed files</span>
              <span className="font-mono text-muted-foreground">{files.length}</span>
              <span className="ml-auto font-mono">
                <span className="text-added">+{add}</span> <span className="text-removed">-{del}</span>
              </span>
            </div>
          )}
          {files?.map((f) => {
            const sel: Selection = { kind: "commit", commit, file: f, url };
            const key = selectionKey(sel);
            const active = activeKey === key;
            return (
              <div
                key={f.path}
                role="treeitem"
                aria-level={2}
                aria-selected={active}
                tabIndex={-1}
                data-row={key}
                onClick={() => onOpen(sel)}
                onDoubleClick={() => onOpen(sel, true)}
                onMouseEnter={() => onHover(sel)}
                className={cn(
                  "relative flex h-[26px] cursor-pointer items-center gap-2 pr-2 pl-8 text-[12px] outline-none focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset",
                  active ? "bg-primary/15" : "hover:bg-hover focus:bg-hover",
                )}
              >
                {active && <span className="absolute inset-y-0 left-0 w-0.5 bg-primary" />}
                <FileIcon path={f.path} />
                <PathLabel path={f.path} className="flex-1" />
                <LineCounts file={f} />
                <StatusLetter status={f.status} />
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

/** `refs`: the full names the badge stands for (a local branch and its remote twin). */
type Ref = { name: string; kind: "head" | "local" | "remote" | "tag"; synced: boolean; refs: string[] };

// Lane geometry, matching the dot above: 12px row padding, 9px dot, its centre 13.5px down.
const LANE = 10;
const LANE_X = 16.5;
const DOT_Y = 13.5;
const BEND_Y = DOT_Y + 10;
// Past this many lanes the rest are cut off; their commits sit on the last one shown.
const MAX_LANES = 8;
const laneOf = (i: number) => Math.min(i, MAX_LANES - 1);
const lanesOf = (row: GraphRow) => Math.min(row.width, MAX_LANES);
const laneX = (i: number) => LANE_X + laneOf(i) * LANE;
// Colour follows the branch, not the column. The list's own branch is grey like its dots, but not
// border-faint: branches have to be seen joining it.
const LANE_COLORS = ["var(--renamed)", "var(--modified)", "var(--primary)", "var(--conflict)", "var(--added)"];
const laneColor = (id: number) => (id === 0 ? "var(--subtle)" : LANE_COLORS[(id - 1) % LANE_COLORS.length]);

/** A row's share of the graph: lines passing by, ending at the dot, and leaving it for its parents. */
function GraphLines({ row }: { row: GraphRow }) {
  const x = laneX(row.col);
  const shown = (l: Lane) => l.col < MAX_LANES || l.col === row.col;
  const curve = (l: Lane, d: string, key: string) => <path key={key} data-lane={l.id} d={d} stroke={laneColor(l.id)} />;
  const line = (l: Lane, from: number | string, to: number | string, key: string) => (
    <line key={key} data-lane={l.id} x1={laneX(l.col)} x2={laneX(l.col)} y1={from} y2={to} stroke={laneColor(l.id)} />
  );
  return (
    <svg aria-hidden className="pointer-events-none absolute inset-y-0 left-0 h-full" width={laneX(lanesOf(row) - 1) + LANE / 2} fill="none">
      {row.through.filter(shown).map((l) => line(l, 0, "100%", `t${l.col}`))}
      {row.into.filter(shown).map((l) =>
        l.col === row.col ? line(l, 0, DOT_Y, `i${l.col}`) : curve(l, `M${laneX(l.col)} 0C${laneX(l.col)} ${DOT_Y} ${x} 0 ${x} ${DOT_Y}`, `i${l.col}`),
      )}
      {row.out.filter(shown).map((l) =>
        l.col === row.col ? (
          line(l, DOT_Y, "100%", `o${l.col}`)
        ) : (
          <Fragment key={`o${l.col}`}>
            {curve(l, `M${x} ${DOT_Y}C${x} ${BEND_Y} ${laneX(l.col)} ${DOT_Y} ${laneX(l.col)} ${BEND_Y}`, "c")}
            {line(l, BEND_Y, "100%", "b")}
          </Fragment>
        ),
      )}
    </svg>
  );
}

/**
 * When the commit landed on the branch, which is the order the list is in. A rebase or cherry-pick
 * keeps the date it was written, which then reads out of order: that one is in the tooltip.
 */
function CommitTime({ commit: c }: { commit: Commit }) {
  const date = (t: number) => new Date(t * 1000).toLocaleString();
  const moved = relativeTime(c.timestamp) !== relativeTime(c.committedAt);
  const title = moved
    ? `Committed ${relativeTime(c.committedAt)} by ${c.committerName} (${date(c.committedAt)})\nAuthored ${relativeTime(c.timestamp)} by ${c.authorName} (${date(c.timestamp)})`
    : date(c.committedAt);
  return (
    <span className={cn("shrink-0", moved && "underline decoration-subtle/60 decoration-dotted underline-offset-2")} title={title}>
      {relativeTime(c.committedAt)}
    </span>
  );
}

/**
 * Groups decorations so they stay readable: a local branch and its remote twin on the same
 * commit (main + origin/main) become one "main ☁" badge; origin/HEAD is dropped, a detached
 * HEAD gets a badge of its own. `remotes` tells remote-tracking names apart, since local names
 * can contain "/" too.
 */
function groupRefs(refs: string[], remotes: Set<string>): Ref[] {
  const head = refs.find((r) => r.startsWith("HEAD -> "))?.slice(8);
  const names = refs.map((r) => (r.startsWith("HEAD -> ") ? r.slice(8) : r));
  const isRemote = (r: string) => remotes.has(r) || (!remotes.size && r.startsWith("origin/"));
  const short = (r: string) => r.slice(r.indexOf("/") + 1);
  const out: Ref[] = names.includes("HEAD") ? [{ name: "HEAD", kind: "head", synced: false, refs: [] }] : [];
  for (const r of names) {
    if (r === "HEAD" || r.endsWith("/HEAD")) continue;
    if (r.startsWith("tag: ")) out.push({ name: r.slice(5), kind: "tag", synced: false, refs: [`refs/tags/${r.slice(5)}`] });
    else if (isRemote(r)) {
      if (!names.includes(short(r))) out.push({ name: r, kind: "remote", synced: false, refs: [`refs/remotes/${r}`] });
    } else {
      const twins = names.filter((x) => isRemote(x) && short(x) === r);
      out.push({ name: r, kind: r === head ? "head" : "local", synced: twins.length > 0, refs: [`refs/heads/${r}`, ...twins.map((x) => `refs/remotes/${x}`)] });
    }
  }
  return out;
}

function RefBadges({ refs, remotes }: { refs: string[]; remotes: Set<string> }) {
  const list = groupRefs(refs, remotes);
  if (!list.length) return null;
  return (
    <div className="mt-1 flex flex-wrap gap-1">
      {list.map((r) => (
        <span
          key={`${r.kind}:${r.name}`}
          title={r.synced ? `${r.name} (in sync with remote)` : r.name}
          className={cn(
            "flex max-w-full items-center gap-1 rounded-[3px] px-1.5 font-mono text-[10px] leading-[18px] font-medium",
            r.kind === "head" && "bg-primary text-white",
            r.kind === "local" && "bg-renamed/15 text-renamed",
            r.kind === "remote" && "border border-border-strong text-muted-foreground",
            r.kind === "tag" && "bg-modified/15 text-modified",
          )}
        >
          {r.kind === "tag" ? <Tag className="size-2.5 shrink-0" /> : r.kind === "remote" ? <Cloud className="size-2.5 shrink-0" /> : null}
          <span className="truncate">{r.name}</span>
          {r.synced && <Cloud className="size-2.5 shrink-0 opacity-75" />}
        </span>
      ))}
    </div>
  );
}

// A commit's signature and trailers never change, and checking a signature runs gpg or ssh:
// ask once per commit, not per file opened in it.
const details = new Map<string, Promise<CommitDetails>>();

/** Signature status and trailers of the commit shown in the header; null while loading. */
export function useCommitDetails(sha: string) {
  const [d, setD] = useState<{ sha: string; details: CommitDetails } | null>(null);
  useEffect(() => {
    let alive = true;
    let p = details.get(sha);
    if (!p) {
      p = api.commitDetails(sha);
      details.set(sha, p);
      p.catch(() => details.delete(sha));
    }
    p.then(
      (x) => alive && setD({ sha, details: x }),
      () => {},
    );
    return () => {
      alive = false;
    };
  }, [sha]);
  return d?.sha === sha ? d.details : null;
}

const SIGNATURES: Record<string, { label: string; tone: string; icon: typeof ShieldCheck; tip: (signer: string) => string }> = {
  G: { label: "Verified", tone: "text-added", icon: ShieldCheck, tip: (s) => `Good signature from ${s}` },
  U: { label: "Signed", tone: "text-added", icon: ShieldCheck, tip: (s) => `Good signature from ${s}, whose key isn't marked as trusted` },
  X: { label: "Signed", tone: "text-modified", icon: ShieldAlert, tip: (s) => `Good signature from ${s}, but the signature has expired` },
  Y: { label: "Signed", tone: "text-modified", icon: ShieldAlert, tip: (s) => `Good signature from ${s}, made with a key that has since expired` },
  R: { label: "Revoked key", tone: "text-removed", icon: ShieldX, tip: (s) => `Signed by ${s} with a key that has been revoked` },
  B: { label: "Bad signature", tone: "text-removed", icon: ShieldX, tip: () => "The signature doesn't match this commit" },
  E: { label: "Signed", tone: "text-subtle", icon: ShieldAlert, tip: () => "Signed, but git couldn't check it: the key is missing or verification isn't set up" },
};

/** Nothing for an unsigned commit, unless commit.gpgSign says it should have been signed. */
export function SignatureBadge({ details: d }: { details: CommitDetails }) {
  const sig =
    SIGNATURES[d.signature] ??
    (d.signExpected ? { label: "Unsigned", tone: "text-modified", icon: ShieldAlert, tip: () => "commit.gpgSign is on, but this commit has no signature" } : null);
  if (!sig) return null;
  const Icon = sig.icon;
  return (
    // Focusable so the keyboard can read the tooltip too (Radix opens it on focus).
    <Tip label={sig.tip(d.signer || "an unknown key")}>
      <span tabIndex={0} className={cn("flex items-center gap-1 rounded-sm outline-none focus-visible:ring-1 focus-visible:ring-ring", sig.tone)}>
        <Icon className="size-3.5" />
        {sig.label}
      </span>
    </Tip>
  );
}

/** Co-authored-by, Signed-off-by and the like, with the name and not the email. */
export function TrailerChips({ details: d, className }: { details: CommitDetails; className?: string }) {
  if (!d.trailers.length) return null;
  return (
    <div className={cn("flex flex-wrap gap-1", className)}>
      {d.trailers.map(([key, value], i) => (
        <Tip key={i} label={`${key}: ${value}`}>
          <span tabIndex={0} className="flex h-5 max-w-72 items-center gap-1 rounded-[3px] bg-elevated px-1.5 text-[11px] outline-none focus-visible:ring-1 focus-visible:ring-ring">
            <span className="shrink-0 text-subtle">{key}</span>
            <span className="truncate text-muted-foreground">{value.replace(/\s*<[^>]*>$/, "") || value}</span>
          </span>
        </Tip>
      ))}
    </div>
  );
}
