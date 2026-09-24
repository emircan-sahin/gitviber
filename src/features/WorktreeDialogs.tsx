import { open } from "@tauri-apps/plugin-dialog";
import { useState, useSyncExternalStore } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { api, type Branch, github, type NetOp, type Target, type Worktree } from "@/lib/api";
import { loadWorktreeDir, saveWorktreeDir } from "@/lib/session";
import { folderMoved, openTerminal, terminalsIn } from "@/lib/terminals";
import { folderName, shortPath } from "@/lib/worktrees";
import { BaseSelect } from "./BranchDialogs";

/** A pull request to check out, as PullView's Checkout would. */
export interface PullSource {
  target: Target;
  number: number;
  headRef: string;
  sameRepo: boolean;
  /** The local branch it lands on. */
  branch: string;
}

/** `base`: a full ref or a commit's full id; `pull`: the new worktree takes a PR's branch instead. */
export type WorktreeDialog = { kind: "new"; base?: string; pull?: PullSource } | { kind: "rename"; worktree: Worktree } | { kind: "lock"; worktree: Worktree };

// Opened from the top bar, History and pull requests alike; the top bar shows it.
let shown: WorktreeDialog | null = null;
const listeners = new Set<() => void>();
const show = (d: WorktreeDialog | null) => {
  shown = d;
  listeners.forEach((l) => l());
};
export const openWorktreeDialog = (d: WorktreeDialog) => show(d);

type Run = (label: string, fn: () => Promise<void>, done: string) => Promise<void>;

interface Props {
  branches: Branch[];
  /** The main worktree: new worktrees go beside it by default, and paths are shown from it. */
  main: string;
  run: Run;
  /** For a pull request's fetch: its progress shows in the top bar, with Cancel. */
  runNet: (label: string, fn: (op: NetOp) => Promise<void>, done: string) => Promise<void>;
  /** Opens a worktree in this window. */
  onOpen: (path: string) => void;
}

type Inner = Props & { onClose: () => void };

/** The folder `path` is in, with its trailing separator. */
const parentOf = (path: string) => path.slice(0, path.length - folderName(path).length);
/** Worktree folders are named after their branch, "/" being a folder separator. */
const folderFor = (branch: string) => branch.replaceAll("/", "-");
const isCommit = (base: string) => /^[0-9a-f]{40}([0-9a-f]{24})?$/i.test(base);

/** The default branch, as git.rs's default_branch finds it: local if there is one. */
function defaultBase(branches: Branch[]) {
  const remote = branches.find((b) => b.remoteDefault && b.name.startsWith("origin/"));
  const name = remote ? remote.name.slice("origin/".length) : "main";
  if (branches.some((b) => !b.remote && b.name === name)) return `refs/heads/${name}`;
  if (remote) return `refs/remotes/${remote.name}`;
  const current = branches.find((b) => b.current);
  return current ? `refs/heads/${current.name}` : "HEAD";
}

export function WorktreeDialogs(props: Props) {
  const dialog = useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => shown,
  );
  if (!dialog) return null;
  const inner = { ...props, onClose: () => show(null) };
  return (
    <Dialog open onOpenChange={(o) => !o && inner.onClose()}>
      <DialogContent>
        {dialog.kind === "new" && <NewWorktree base={dialog.base} pull={dialog.pull} {...inner} />}
        {dialog.kind === "rename" && <RenameWorktree worktree={dialog.worktree} {...inner} />}
        {dialog.kind === "lock" && <LockWorktree worktree={dialog.worktree} {...inner} />}
      </DialogContent>
    </Dialog>
  );
}

function NewWorktree({ base, pull, branches, main, onClose, run, runNet, onOpen }: { base?: string; pull?: PullSource } & Inner) {
  const fallback = `${parentOf(main)}${folderName(main)}.worktrees`;
  const [name, setName] = useState("");
  const [from, setFrom] = useState(() => base ?? defaultBase(branches));
  const [dir, setDir] = useState(() => loadWorktreeDir(main) ?? fallback);
  const [terminal, setTerminal] = useState(true);
  const [switchTo, setSwitchTo] = useState(false);
  const n = pull ? pull.branch : name.trim();
  const choose = async () => {
    const picked = await open({ directory: true, defaultPath: dir, title: "Folder for new worktrees" });
    if (typeof picked === "string") setDir(picked);
  };
  const submit = () => {
    onClose();
    // Remembered for the project, so its next worktree goes there too.
    saveWorktreeDir(main, dir === fallback ? null : dir);
    const where = dir === fallback ? null : dir;
    const then = (path: string) => {
      if (terminal) openTerminal(path);
      if (switchTo) onOpen(path);
    };
    if (pull) {
      const p = pull;
      void runNet("Check out PR", (op) => github.checkoutWorktree(p.target, p.number, p.headRef, p.sameRepo, where, op).then(then), `Checked out #${p.number} in worktree ${folderFor(n)}`);
    } else void run("Create worktree", () => api.addWorktree(n, from, where).then(then), `Created worktree ${n}`);
  };
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (n) submit();
      }}
    >
      <DialogTitle>{pull ? `Check out #${pull.number} in a new worktree` : "New worktree"}</DialogTitle>
      <DialogDescription>
        {pull ? (
          <>
            The pull request's branch, <span className="font-mono">{pull.branch}</span>, in its own folder; this one stays as it is.
          </>
        ) : (
          "A new branch, checked out in its own folder, side by side with this one."
        )}
      </DialogDescription>
      {!pull && <Input autoFocus className="mt-4 font-mono" value={name} onChange={(e) => setName(e.target.value)} placeholder="Branch name" spellCheck={false} />}
      {!pull &&
        (isCommit(from) ? (
          <div className="mt-3 text-[11.5px] text-muted-foreground">
            From commit <span className="font-mono text-foreground">{from.slice(0, 7)}</span>
          </div>
        ) : (
          <BaseSelect value={from} onChange={setFrom} branches={branches} head={false} />
        ))}
      <div className="mt-3 text-[11.5px] text-muted-foreground">
        Folder
        <div className="mt-1 flex items-center gap-2">
          <span className="min-w-0 flex-1 truncate font-mono text-[12px] text-foreground" title={dir}>
            {shortPath(dir, main)}/{n ? folderFor(n) : "…"}
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
      </div>
      <div className="mt-4 flex items-center gap-3">
        <label className="flex items-center gap-1.5 text-[12px]">
          <input type="checkbox" checked={terminal} onChange={(e) => setTerminal(e.target.checked)} className="accent-primary" />
          Open a terminal in it
        </label>
        <label className="flex items-center gap-1.5 text-[12px]">
          <input type="checkbox" checked={switchTo} onChange={(e) => setSwitchTo(e.target.checked)} className="accent-primary" />
          Switch to it
        </label>
        {/* A pull request has nothing to type, so the button takes the focus. */}
        <Button type="submit" className="ml-auto" disabled={!n} autoFocus={!!pull}>
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
  const n = name.trim();
  const target = `${parentOf(w.path)}${folderFor(n)}`;
  const moving = move && !stays && !!n && target !== w.path;
  const terminals = moving ? terminalsIn(w.path) : 0;
  const upstream = branches.find((b) => !b.remote && b.name === old)?.upstream;
  const submit = () => {
    onClose();
    const done = n === old ? `Moved ${folderName(w.path)} to ${folderFor(n)}` : `Renamed ${old} to ${n}${moving ? ", folder too" : ""}`;
    void run(
      "Rename worktree",
      async () => {
        const to = await api.renameWorktree(w.path, n, moving);
        if (to !== w.path) folderMoved(w.path, to);
      },
      done,
    );
  };
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (n && (n !== old || moving)) submit();
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
        <Button type="submit" disabled={!n || (n === old && !moving)}>
          {terminals > 0 ? "Rename and move" : "Rename"}
        </Button>
      </div>
    </form>
  );
}

function LockWorktree({ worktree: w, main, onClose, run }: { worktree: Worktree } & Inner) {
  const [reason, setReason] = useState("");
  const r = reason.trim();
  const submit = () => {
    onClose();
    void run("Lock worktree", () => api.lockWorktree(w.path, r || null), `Locked ${folderName(w.path)}`);
  };
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        submit();
      }}
    >
      <DialogTitle>Lock worktree</DialogTitle>
      <DialogDescription>
        Keeps <span className="font-mono">{shortPath(w.path, main)}</span> from being pruned, moved or removed until it's unlocked, as for a folder on a drive that isn't always plugged in.
      </DialogDescription>
      <Input autoFocus className="mt-4" value={reason} onChange={(e) => setReason(e.target.value)} placeholder="Reason (optional)" />
      <div className="mt-4 flex justify-end">
        <Button type="submit">Lock</Button>
      </div>
    </form>
  );
}
