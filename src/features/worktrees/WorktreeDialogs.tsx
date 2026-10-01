import { open } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { api, type Branch, github, type Target, type Worktree } from "@/lib/api";
import { loadWorktreeDir, loadWorktreeRun, moveRoot, saveBranchIssue, saveWorktreeDir, saveWorktreeRun, sharedWorktreeDir } from "@/lib/repo/session";
import { issueBranchName, withIssue } from "@/lib/github/issueWork";
import { folderMoved, openTerminal, terminalsIn } from "@/lib/terminal/terminals";
import { localNames, refNameCheck, sameRef } from "@/lib/git/refs";
import { shortPath } from "@/lib/git/worktrees";
import { plural } from "@/lib/format";
import { folderName, parentFolder } from "@/lib/path";
import { createStore } from "@/lib/store";
import { BaseSelect } from "@/features/branches/BaseSelect";
import { NameHint } from "@/components/NameHint";
import { useAsyncValue } from "@/hooks/useAsyncValue";
import { type GitRun, type NetRun, useSubmit } from "@/hooks/useGitAction";

/** A pull request to check out, as PullView's Checkout would. */
export interface PullSource {
  target: Target;
  number: number;
  headRef: string;
  sameRepo: boolean;
  /** The local branch it lands on. */
  branch: string;
}

/** An issue to start work on: the branch is named after it, and a PR from it closes it. */
export interface IssueSource {
  number: number;
  title: string;
  url: string;
  /** Origin's owner/name, which the branch's issue is remembered under. */
  origin: string;
}

/** `base`: a full ref or a commit's full id; `pull`: the new worktree takes a PR's branch instead. */
export type WorktreeDialog = { kind: "new"; base?: string; pull?: PullSource; issue?: IssueSource } | { kind: "rename"; worktree: Worktree } | { kind: "lock"; worktree: Worktree };

// Opened from the top bar, History and pull requests alike; the top bar shows it.
const shown = createStore<WorktreeDialog | null>(null);
const show = shown.set;
export const openWorktreeDialog = (d: WorktreeDialog) => show(d);
/** The dialog showing now, if any. */
export const useWorktreeDialog = shown.use;

interface Props {
  branches: Branch[];
  /** The main worktree: new worktrees go beside it by default, and paths are shown from it. */
  main: string;
  run: GitRun;
  /** For a pull request's fetch: its progress shows in the top bar, with Cancel. */
  runNet: NetRun;
  /** Opens a worktree in this window. */
  onOpen: (path: string) => void;
}

type Inner = Props & { onClose: () => void };

/** Worktree folders are named after their branch, "/" being a folder separator. */
const folderFor = (branch: string) => branch.replaceAll("/", "-");
const isCommit = (base: string) => /^[0-9a-f]{40}([0-9a-f]{24})?$/i.test(base);

/**
 * The branch checked out here, as GitHub Desktop and VS Code start from. Detached, the default
 * branch, as git/branch.rs's default_branch finds it: local if there is one.
 */
function defaultBase(branches: Branch[]) {
  const current = branches.find((b) => b.current && !b.remote);
  if (current) return `refs/heads/${current.name}`;
  const remote = branches.find((b) => b.remoteDefault && b.name.startsWith("origin/"));
  const name = remote ? remote.name.slice("origin/".length) : "main";
  if (branches.some((b) => !b.remote && b.name === name)) return `refs/heads/${name}`;
  return remote ? `refs/remotes/${remote.name}` : "HEAD";
}

export function WorktreeDialogs(props: Props) {
  const dialog = useWorktreeDialog();
  if (!dialog) return null;
  const inner = { ...props, onClose: () => show(null) };
  return (
    <Dialog open onOpenChange={(o) => !o && inner.onClose()}>
      <DialogContent>
        {dialog.kind === "new" && <NewWorktree base={dialog.base} pull={dialog.pull} issue={dialog.issue} {...inner} />}
        {dialog.kind === "rename" && <RenameWorktree worktree={dialog.worktree} {...inner} />}
        {dialog.kind === "lock" && <LockWorktree worktree={dialog.worktree} {...inner} />}
      </DialogContent>
    </Dialog>
  );
}

function NewWorktree({ base, pull, issue, branches, main, onClose, run, runNet, onOpen }: { base?: string; pull?: PullSource; issue?: IssueSource } & Inner) {
  const beside = `${parentFolder(main)}${folderName(main)}.worktrees`;
  const fallback = sharedWorktreeDir(main) ?? beside;
  const [name, setName] = useState(() => (issue ? issueBranchName(issue.number, issue.title) : ""));
  const [from, setFrom] = useState(() => base ?? defaultBase(branches));
  // No branch to default to (an unborn or detached repo): HEAD is listed, not silently used.
  const [headOption] = useState(from === "HEAD");
  const [dir, setDir] = useState(() => loadWorktreeDir(main) ?? fallback);
  const [terminal, setTerminal] = useState(true);
  const [command, setCommand] = useState(() => loadWorktreeRun(main, !!issue));
  const [switchTo, setSwitchTo] = useState(false);
  const includes = useAsyncValue(api.worktreeIncludes, [], 0);
  const check = refNameCheck(name, localNames(branches));
  // A branch that exists is checked out as it is, unless it's checked out already: git keeps
  // a branch in one worktree at a time.
  const existing = !pull && check.taken ? branches.find((b) => !b.remote && sameRef(b.name, check.name)) : undefined;
  const held = existing && (existing.current ? "here" : existing.worktree && `in ${folderName(existing.worktree)}`);
  const hint = !existing
    ? check
    : held
      ? { hint: `${existing.name} is checked out ${held}; a branch can be in one worktree at a time.`, taken: true }
      : { hint: `Checks out the existing branch ${existing.name}.`, taken: false };
  const n = pull ? pull.branch : (existing?.name ?? check.name);
  const { pending, send } = useSubmit(onClose);
  const ready = !!n && !held && !pending;
  const choose = async () => {
    const picked = await open({ directory: true, defaultPath: dir, title: "Folder for new worktrees" });
    if (typeof picked === "string") setDir(picked);
  };
  const submit = () => {
    const where = dir === beside ? null : dir;
    const then = (path: string) => {
      // Remembered for the project once it worked, so its next worktree goes there too.
      saveWorktreeDir(main, dir === fallback ? null : dir);
      if (issue) saveBranchIssue(issue.origin, n, issue.url);
      if (terminal) {
        saveWorktreeRun(main, command.trim(), !!issue);
        openTerminal(path, issue ? withIssue(command.trim(), issue.number) : command.trim());
      }
      if (switchTo) onOpen(path);
    };
    if (pull) {
      // Nothing typed to keep, and the fetch's Cancel is in the top bar behind this dialog.
      onClose();
      const p = pull;
      void runNet("Check out PR", (op) => github.checkoutWorktree(p.target, p.number, p.headRef, p.sameRepo, where, op).then(then), `Checked out #${p.number} in worktree ${folderFor(n)}`);
    } else void send(() => run("Create worktree", () => api.addWorktree(n, existing ? null : from, where).then(then), existing ? `Checked out ${n} in a new worktree` : `Created worktree ${n}`));
  };
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (ready) submit();
      }}
    >
      <DialogTitle>{pull ? `Check out #${pull.number} in a new worktree` : issue ? `Start #${issue.number} in a new worktree` : "New worktree"}</DialogTitle>
      <DialogDescription>
        {pull ? (
          <>
            The pull request's branch, <span className="font-mono">{pull.branch}</span>, in its own folder; this one stays as it is.
          </>
        ) : issue ? (
          `A new branch for “${issue.title}”, in its own folder; its pull request will close #${issue.number}.`
        ) : (
          "A new branch, checked out in its own folder, side by side with this one."
        )}
      </DialogDescription>
      {!pull && <Input autoFocus className="mt-4 font-mono" value={name} onChange={(e) => setName(e.target.value)} placeholder="Branch name" spellCheck={false} />}
      {!pull && <NameHint {...hint} />}
      {!pull &&
        !existing &&
        (isCommit(from) ? (
          <div className="mt-3 text-[11.5px] text-muted-foreground">
            From commit <span className="font-mono text-foreground">{from.slice(0, 7)}</span>
          </div>
        ) : (
          <BaseSelect value={from} onChange={setFrom} branches={branches} head={headOption} />
        ))}
      <div className="mt-3 text-[11.5px] text-muted-foreground">
        Folder
        <div className="mt-1 flex items-center gap-2">
          {/* rtl cuts the start of a long path, keeping the branch's folder in view. The LRMs keep
              it reading left to right; a <bdi> did too, but WebKit drew the … over a letter. */}
          <span dir="rtl" className="min-w-0 flex-1 truncate text-left font-mono text-[12px] text-foreground" title={dir}>
            {`\u200e${shortPath(dir, main)}/${n ? folderFor(n) : "…"}\u200e`}
          </span>
          {dir !== fallback && (
            <Button type="button" variant="ghost" size="sm" onClick={() => setDir(fallback)}>
              Default
            </Button>
          )}
          <Button type="button" variant="outline" size="sm" onClick={choose}>
            Change…
          </Button>
        </div>
        {includes > 0 && (
          <div className="mt-1.5">
            Copies {plural(includes, "ignored file")} listed in <span className="font-mono">.worktreeinclude</span>
          </div>
        )}
      </div>
      <div className="mt-4 flex items-center gap-2">
        <label className="flex shrink-0 items-center gap-1.5 text-[12px]">
          <input type="checkbox" checked={terminal} onChange={(e) => setTerminal(e.target.checked)} className="accent-primary" />
          Open a terminal in it
        </label>
        <Input
          className="font-mono disabled:opacity-50"
          value={command}
          onChange={(e) => setCommand(e.target.value)}
          disabled={!terminal}
          placeholder={issue ? 'and run… (e.g. claude "Fix #{issue}")' : "and run… (e.g. claude)"}
          aria-label="Command to run in the terminal"
          spellCheck={false}
        />
      </div>
      {terminal && (issue || command.includes("{issue}")) && (
        <div className="mt-1.5 text-[11.5px] text-muted-foreground">
          {issue ? (
            <>
              <span className="font-mono">{"{issue}"}</span> in the command becomes {issue.number}, the issue's number.
            </>
          ) : (
            <>
              <span className="font-mono">{"{issue}"}</span> is filled in only when starting from an issue.
            </>
          )}
        </div>
      )}
      <div className="mt-3 flex items-center gap-3">
        <label className="flex items-center gap-1.5 text-[12px]">
          <input type="checkbox" checked={switchTo} onChange={(e) => setSwitchTo(e.target.checked)} className="accent-primary" />
          Switch to it
        </label>
        {/* A pull request has nothing to type, so the button takes the focus. */}
        <Button type="submit" className="ml-auto" disabled={!ready} autoFocus={!!pull}>
          {pull ? "Check out" : "Create"}
        </Button>
      </div>
    </form>
  );
}

function RenameWorktree({ worktree: w, branches, main, onClose, run }: { worktree: Worktree } & Inner) {
  const old = w.branch ?? "";
  const [name, setName] = useState(old);
  // The same checks rename_worktree makes; the folder of the open worktree would move out from under this window.
  const stays = w.main
    ? "The main worktree's folder stays where it is."
    : w.current
      ? "This window has it open; switch to another worktree to move its folder."
      : w.locked
        ? "It's locked; unlock it (the lock on its row) to move its folder."
        : null;
  const [move, setMove] = useState(!stays);
  const check = refNameCheck(name, localNames(branches, old), true);
  const n = check.name;
  const { pending, send } = useSubmit(onClose);
  const target = `${parentFolder(w.path)}${folderFor(n)}`;
  const moving = move && !stays && !!n && target !== w.path;
  const terminals = moving ? terminalsIn(w.path) : 0;
  const upstream = branches.find((b) => !b.remote && b.name === old)?.upstream;
  const ready = !!n && (n !== old || moving) && !check.taken && !pending;
  const submit = () => {
    const done = n === old ? `Moved ${folderName(w.path)} to ${folderFor(n)}` : `Renamed ${old} to ${n}${moving ? ", folder too" : ""}`;
    void send(() =>
      run(
        "Rename worktree",
        async () => {
          const to = await api.renameWorktree(w.path, n, moving);
          if (to === w.path) return;
          folderMoved(w.path, to);
          moveRoot(w.path, to);
        },
        done,
      ),
    );
  };
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (ready) submit();
      }}
    >
      <DialogTitle>Rename worktree</DialogTitle>
      <DialogDescription>
        Renames the branch <span className="font-mono">{old}</span> checked out in <span className="font-mono">{shortPath(w.path, main)}</span>.
        {upstream && (
          <>
            {" "}
            It keeps tracking <span className="font-mono">{upstream}</span>; the branch menu can rename that too.
          </>
        )}
      </DialogDescription>
      <Input autoFocus className="mt-4 font-mono" value={name} onChange={(e) => setName(e.target.value)} onFocus={(e) => e.currentTarget.select()} placeholder="New name" spellCheck={false} />
      <NameHint {...check} />
      <label className="mt-3 flex items-start gap-2 text-[12px]">
        <input type="checkbox" checked={move && !stays} disabled={!!stays} onChange={(e) => setMove(e.target.checked)} className="mt-0.5 accent-primary" />
        <span>
          Rename its folder to match
          <span className="block text-[11.5px] break-all text-muted-foreground">{stays ?? `${shortPath(w.path, main)} → ${folderFor(n) || "…"}`}</span>
        </span>
      </label>
      {terminals > 0 && (
        <div className="mt-3 rounded-md bg-removed/10 px-2.5 py-2 text-[11.5px] text-removed">
          {terminals === 1 ? "A terminal runs" : `${terminals} terminals run`} in this folder. The shell moves with it, but whatever holds on to the old path, like an agent or a dev server, may
          lose track of its files.
        </div>
      )}
      <div className="mt-4 flex justify-end">
        <Button type="submit" disabled={!ready}>
          {terminals > 0 ? "Rename and move" : "Rename"}
        </Button>
      </div>
    </form>
  );
}

function LockWorktree({ worktree: w, main, onClose, run }: { worktree: Worktree } & Inner) {
  const [reason, setReason] = useState("");
  const r = reason.trim();
  const { pending, send } = useSubmit(onClose);
  const submit = () => void send(() => run("Lock worktree", () => api.lockWorktree(w.path, r || null), `Locked ${folderName(w.path)}`));
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (!pending) submit();
      }}
    >
      <DialogTitle>Lock worktree</DialogTitle>
      <DialogDescription>
        Keeps <span className="font-mono">{shortPath(w.path, main)}</span> from being pruned, moved or removed until it's unlocked, as for a folder on a drive that isn't always plugged in.
      </DialogDescription>
      <Input autoFocus className="mt-4" value={reason} onChange={(e) => setReason(e.target.value)} placeholder="Reason (optional)" />
      <div className="mt-4 flex justify-end">
        <Button type="submit" disabled={pending}>
          Lock
        </Button>
      </div>
    </form>
  );
}
