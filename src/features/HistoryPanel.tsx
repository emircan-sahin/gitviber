import { ask } from "@tauri-apps/plugin-dialog";
import { Cloud, Copy, ExternalLink, GitBranchPlus, GitCommitHorizontal, History, Link, RotateCcw, Tag, Undo2 } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { api, type Commit, errorMessage, type FileChange, type RepoStatus, type ResetMode } from "@/lib/api";
import { type Selection, selectionKey } from "@/lib/selection";
import { toast } from "@/lib/toast";
import { tracked, undoAction } from "@/lib/undo";
import { cn, relativeTime } from "@/lib/utils";
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
  /** A commit to open when it shows up (blame's link to it). */
  openSha?: string;
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
}

/** GitHub only has commits that reached one of origin's branches (or all, for a fork's original). */
const commitUrl = (c: Commit, { webUrl, everyOnWeb }: Pick<Actions, "webUrl" | "everyOnWeb">) =>
  webUrl && (c.onOrigin || everyOnWeb) ? `${webUrl}/commit/${c.sha}` : undefined;

export function HistoryPanel({ commits, status, remotes, hasMore, loadMore, refresh, activeKey, onOpen, onHover, headSha, web, empty = "No commits yet.", openSha }: Props) {
  const [open, setOpen] = useState<string | null>(openSha ?? null);
  useEffect(() => {
    if (openSha) setOpen(openSha);
  }, [openSha]);
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
      toast("error", `${label} failed`, errorMessage(e));
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

  if (!commits.length) {
    return <div className="px-6 pt-20 text-center text-[12px] text-subtle">{empty}</div>;
  }

  return (
    <div ref={scroller} className="h-full overflow-x-hidden overflow-y-auto py-1">
      {commits.map((c, i) => (
        <CommitRow
          key={c.sha}
          commit={c}
          remotes={remotes}
          first={i === 0}
          last={i === commits.length - 1}
          open={open === c.sha}
          onToggle={(el) => toggle(c.sha, el)}
          activeKey={activeKey}
          onOpen={onOpen}
          onHover={onHover}
          url={commitUrl(c, actions)}
          menu={<CommitMenu commit={c} head={c.sha === head} actions={actions} />}
        />
      ))}
      {hasMore && (
        <div className="p-2">
          <Button variant="secondary" size="sm" className="w-full" onClick={() => loadMore().catch((e) => toast("error", "Could not load history", errorMessage(e)))}>
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

/** Right-click actions on a commit. `head`: the first row, i.e. the checked-out commit. */
function CommitMenu({ commit: c, head, actions }: { commit: Commit; head: boolean; actions: Actions }) {
  const { status, headSha, webUrl, locked, run } = actions;
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

  const copy = (text: string, what: string) =>
    navigator.clipboard.writeText(text).then(
      () => toast("success", what),
      (e) => toast("error", "Could not copy", errorMessage(e)),
    );

  return (
    // Focus has nowhere useful to return to, and restoring it would steal it from the name dialog.
    <ContextMenuContent onCloseAutoFocus={(e) => e.preventDefault()}>
      <ContextMenuItem disabled={locked || !head || !c.parents.length} onSelect={undo}>
        <Undo2 /> Undo commit
      </ContextMenuItem>
      {/* Reverting a commit HEAD never had would apply the opposite of a change that isn't there. */}
      <ContextMenuItem disabled={locked || c.notInHead} onSelect={() => run("Revert", () => api.revert(c.sha), `Reverted ${short}`)}>
        <RotateCcw /> Revert commit
      </ContextMenuItem>
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
      <ContextMenuItem disabled={locked} onSelect={() => actions.name("branch", c)}>
        <GitBranchPlus /> Create branch from here…
      </ContextMenuItem>
      <ContextMenuItem onSelect={() => actions.name("tag", c)}>
        <Tag /> Create tag here…
      </ContextMenuItem>
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
  const submit = () => {
    const n = name.trim();
    onClose();
    if (kind === "branch") run("Create branch", () => api.createBranchAt(n, commit.sha), `Switched to new branch ${n}`);
    else run("Create tag", () => api.createTag(n, commit.sha), `Tagged ${commit.shortSha} as ${n}`);
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
          className="mt-4 flex gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (name.trim()) submit();
          }}
        >
          <Input autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder={kind === "branch" ? "Branch name" : "Tag name, e.g. v1.2.0"} spellCheck={false} />
          <Button type="submit" disabled={!name.trim()}>
            Create
          </Button>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function CommitRow({
  commit,
  remotes,
  first,
  last,
  open,
  onToggle,
  activeKey,
  onOpen,
  onHover,
  url,
  menu,
}: {
  commit: Commit;
  remotes: Set<string>;
  first: boolean;
  last: boolean;
  open: boolean;
  onToggle: (row: HTMLElement) => void;
  activeKey: string | null;
  onOpen: (s: Selection, pin?: boolean) => void;
  onHover: (s: Selection) => void;
  url: string | undefined;
  menu: React.ReactNode;
}) {
  const [files, setFiles] = useState<FileChange[] | null>(null);

  useEffect(() => {
    if (!open || files) return;
    let alive = true;
    api
      .commitFiles(commit.sha)
      .then((f) => {
        if (!alive) return;
        setFiles(f);
        // Jump straight into the file (a file's history: that one) so one click shows code.
        const file = f.find((x) => x.path === commit.file) ?? f[0];
        if (file) onOpen({ kind: "commit", commit, file, url });
      })
      .catch((e) => alive && toast("error", "Could not load commit", errorMessage(e)));
    // Collapsing (or opening another commit) cancels the auto-open of a late reply.
    return () => {
      alive = false;
    };
  }, [open, files, commit, url, onOpen]);

  const merge = commit.parents.length > 1;
  const add = files?.reduce((n, f) => n + (f.additions ?? 0), 0) ?? 0;
  const del = files?.reduce((n, f) => n + (f.deletions ?? 0), 0) ?? 0;

  return (
    <div className="relative">
      <div className={cn("absolute left-[15px] w-px bg-border-strong", first ? "top-3" : "top-0", last && !open ? "h-3" : "bottom-0")} />
      <ContextMenu>
        <ContextMenuTrigger asChild>
          <div
            role="button"
            onClick={(e) => onToggle(e.currentTarget)}
            className={cn("relative flex cursor-pointer items-start gap-2.5 py-1.5 pr-2 pl-3 data-[state=open]:bg-hover", open ? "bg-active" : "hover:bg-hover")}
          >
            <span
              className={cn(
                "relative z-10 mt-[3px] size-[9px] shrink-0 rounded-full border-2",
                commit.unpushed ? "border-primary bg-primary" : commit.notInHead ? "border-added bg-added" : merge ? "border-renamed bg-sidebar" : "border-subtle bg-sidebar",
              )}
              title={commit.unpushed ? "Not pushed yet" : commit.notInHead ? "Not in your branch yet" : undefined}
            />
            <div className="min-w-0 flex-1">
              <div className={cn("truncate text-[12px] leading-4", open ? "font-medium text-foreground" : "text-foreground/90")}>{commit.subject}</div>
              <div className="mt-0.5 flex min-w-0 items-center gap-1.5 text-[10.5px] text-subtle">
                <span className="min-w-0 truncate">{commit.authorName}</span>
                <span>·</span>
                <span className="shrink-0">{relativeTime(commit.timestamp)}</span>
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
            const active = activeKey === selectionKey(sel);
            return (
              <div
                key={f.path}
                role="button"
                onClick={() => onOpen(sel)}
                onDoubleClick={() => onOpen(sel, true)}
                onMouseEnter={() => onHover(sel)}
                className={cn("relative flex h-[26px] cursor-pointer items-center gap-2 pr-2 pl-8 text-[12px]", active ? "bg-primary/15" : "hover:bg-hover")}
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

type Ref = { name: string; kind: "head" | "local" | "remote" | "tag"; synced: boolean };

/**
 * Groups decorations so they stay readable: a local branch and its remote twin on the same
 * commit (main + origin/main) become one "main ☁" badge; origin/HEAD is dropped.
 * `remotes` tells remote-tracking names apart, since local names can contain "/" too.
 */
function groupRefs(refs: string[], remotes: Set<string>): Ref[] {
  const head = refs.find((r) => r.startsWith("HEAD -> "))?.slice(8);
  const names = refs.map((r) => (r.startsWith("HEAD -> ") ? r.slice(8) : r));
  const isRemote = (r: string) => remotes.has(r) || (!remotes.size && r.startsWith("origin/"));
  const short = (r: string) => r.slice(r.indexOf("/") + 1);
  const out: Ref[] = [];
  for (const r of names) {
    if (r === "HEAD" || r.endsWith("/HEAD")) continue;
    if (r.startsWith("tag: ")) out.push({ name: r.slice(5), kind: "tag", synced: false });
    else if (isRemote(r)) {
      if (!names.includes(short(r))) out.push({ name: r, kind: "remote", synced: false });
    } else out.push({ name: r, kind: r === head ? "head" : "local", synced: names.some((x) => isRemote(x) && short(x) === r) });
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
