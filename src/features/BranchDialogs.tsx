import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { api, type Branch, type NetOp } from "@/lib/api";

/** The branch picker's actions that need more than a click. `base`: a full ref, or HEAD. */
export type BranchDialog = { kind: "rename"; branch: Branch } | { kind: "new"; base: string } | { kind: "upstream"; branch: Branch };

type Run = (label: string, fn: () => Promise<void>, done: string) => Promise<void>;

interface Props {
  dialog: BranchDialog;
  branches: Branch[];
  onClose: () => void;
  run: Run;
  /** For what reaches the remote: its progress shows in the top bar, with Cancel. */
  runNet: (label: string, fn: (op: NetOp) => Promise<void>, done: string) => Promise<void>;
}

const SELECT = "h-7 w-full rounded-md border border-border-strong bg-background px-2 font-mono text-[12px] text-foreground outline-none focus:border-primary";

export function BranchDialogs({ dialog, branches, onClose, run, runNet }: Props) {
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        {dialog.kind === "rename" && <Rename branch={dialog.branch} branches={branches} onClose={onClose} run={run} runNet={runNet} />}
        {dialog.kind === "new" && <NewBranch base={dialog.base} branches={branches} onClose={onClose} run={run} />}
        {dialog.kind === "upstream" && <Upstream branch={dialog.branch} branches={branches} onClose={onClose} run={run} />}
      </DialogContent>
    </Dialog>
  );
}

function Rename({ branch, branches, onClose, run, runNet }: { branch: Branch } & Omit<Props, "dialog">) {
  const [name, setName] = useState(branch.name);
  const [remote, setRemote] = useState(false);
  const n = name.trim();
  // Only an upstream that is still there can be renamed; a local one (remote ".") never.
  const upstream = branches.find((b) => b.remote && b.name === branch.upstream)?.name ?? null;
  const remoteName = upstream?.slice(0, upstream.indexOf("/"));
  const submit = () => {
    onClose();
    const done = `Renamed ${branch.name} to ${n}${remote ? ` here and on ${remoteName}` : ""}`;
    void (remote ? runNet("Rename branch", (op) => api.renameBranch(branch.name, n, true, op), done) : run("Rename branch", () => api.renameBranch(branch.name, n, false), done));
  };
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (n && n !== branch.name) submit();
      }}
    >
      <DialogTitle>Rename branch</DialogTitle>
      <DialogDescription>
        <span className="font-mono">{branch.name}</span>
        {upstream ? (
          <>
            {" "}
            tracks <span className="font-mono">{upstream}</span>.
          </>
        ) : (
          "."
        )}
      </DialogDescription>
      <Input autoFocus className="mt-4 font-mono" value={name} onChange={(e) => setName(e.target.value)} onFocus={(e) => e.currentTarget.select()} placeholder="New name" spellCheck={false} />
      {upstream && (
        <label className="mt-3 flex items-start gap-2 text-[12px]">
          <input type="checkbox" checked={remote} onChange={(e) => setRemote(e.target.checked)} className="mt-0.5 accent-primary" />
          <span>
            Rename it on {remoteName} too
            <span className="block text-[11.5px] text-muted-foreground">
              Pushes {n || "the new name"} and tracks it, then deletes {upstream} from {remoteName}. Pull requests from the old name get closed, and other clones keep tracking it.
            </span>
          </span>
        </label>
      )}
      <div className="mt-4 flex justify-end">
        <Button type="submit" disabled={!n || n === branch.name}>
          Rename
        </Button>
      </div>
    </form>
  );
}

function NewBranch({ base, branches, onClose, run }: { base: string } & Pick<Props, "branches" | "onClose" | "run">) {
  const [name, setName] = useState("");
  const [from, setFrom] = useState(base);
  const [switchTo, setSwitchTo] = useState(true);
  const [tags, setTags] = useState<string[]>([]);
  useEffect(() => {
    api.tags().then(setTags, () => setTags([]));
  }, []);
  const n = name.trim();
  const label = from === "HEAD" ? "HEAD" : from.replace(/^refs\/(heads|remotes|tags)\//, "");
  const submit = () => {
    onClose();
    void run("Create branch", () => api.createBranch(n, from, switchTo), switchTo ? `Switched to new branch ${n}` : `Created ${n} from ${label}`);
  };
  const group = (title: string, prefix: string, names: string[]) =>
    names.length > 0 && (
      <optgroup label={title}>
        {names.map((b) => (
          <option key={b} value={`${prefix}${b}`}>
            {b}
          </option>
        ))}
      </optgroup>
    );
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (n) submit();
      }}
    >
      <DialogTitle>New branch</DialogTitle>
      <DialogDescription>It starts where the base is and tracks nothing until you publish it.</DialogDescription>
      <Input autoFocus className="mt-4 font-mono" value={name} onChange={(e) => setName(e.target.value)} placeholder="Branch name" spellCheck={false} />
      <label className="mt-3 block text-[11.5px] text-muted-foreground">
        From
        <select value={from} onChange={(e) => setFrom(e.target.value)} className={`${SELECT} mt-1`}>
          {base === "HEAD" && <option value="HEAD">HEAD</option>}
          {group(
            "Local",
            "refs/heads/",
            branches.filter((b) => !b.remote).map((b) => b.name),
          )}
          {group(
            "Remote",
            "refs/remotes/",
            branches.filter((b) => b.remote).map((b) => b.name),
          )}
          {group("Tags", "refs/tags/", tags)}
        </select>
      </label>
      <div className="mt-4 flex items-center gap-2">
        <label className="flex items-center gap-1.5 text-[12px]">
          <input type="checkbox" checked={switchTo} onChange={(e) => setSwitchTo(e.target.checked)} className="accent-primary" />
          Switch to it
        </label>
        <Button type="submit" className="ml-auto" disabled={!n}>
          Create
        </Button>
      </div>
    </form>
  );
}

function Upstream({ branch, branches, onClose, run }: { branch: Branch } & Pick<Props, "branches" | "onClose" | "run">) {
  const remotes = branches.filter((b) => b.remote).map((b) => b.name);
  const guess = [branch.upstream, `origin/${branch.name}`].find((u) => u && remotes.includes(u)) ?? remotes.find((r) => r.endsWith(`/${branch.name}`)) ?? remotes[0] ?? "";
  const [upstream, setUpstream] = useState(guess);
  const submit = () => {
    onClose();
    void run("Set upstream", () => api.setUpstream(branch.name, upstream), `${branch.name} now tracks ${upstream}`);
  };
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (upstream) submit();
      }}
    >
      <DialogTitle>Set upstream</DialogTitle>
      <DialogDescription>
        What <span className="font-mono">{branch.name}</span> pulls from and compares against
        {branch.upstream ? (
          <>
            ; now <span className="font-mono">{branch.upstream}</span>
          </>
        ) : null}
        .
      </DialogDescription>
      {remotes.length ? (
        <select autoFocus value={upstream} onChange={(e) => setUpstream(e.target.value)} className={`${SELECT} mt-4`}>
          {remotes.map((r) => (
            <option key={r} value={r}>
              {r}
            </option>
          ))}
        </select>
      ) : (
        <div className="mt-4 text-[12px] text-muted-foreground">No remote branches. Fetch, or publish the branch first.</div>
      )}
      <div className="mt-4 flex justify-end">
        <Button type="submit" disabled={!upstream}>
          Set upstream
        </Button>
      </div>
    </form>
  );
}
