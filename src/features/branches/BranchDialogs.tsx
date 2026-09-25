import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { api, type Branch } from "@/lib/api";
import { shortRef } from "@/lib/git/refs";
import { Select } from "@/components/ui/select";
import { BaseSelect } from "./BaseSelect";
import { type GitRun, type NetRun } from "@/hooks/useGitAction";

/** The branch picker's actions that need more than a click. `base`: a full ref, or HEAD. */
export type BranchDialog = { kind: "rename"; branch: Branch } | { kind: "new"; base: string } | { kind: "upstream"; branch: Branch };

interface Props {
  dialog: BranchDialog;
  branches: Branch[];
  onClose: () => void;
  run: GitRun;
  /** For what reaches the remote: its progress shows in the top bar, with Cancel. */
  runNet: NetRun;
}

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
        Renames the local branch <span className="font-mono">{branch.name}</span>.
        {upstream && (
          <>
            {" "}
            It tracks <span className="font-mono">{upstream}</span>, which keeps its name unless you rename it on {remoteName} too.
          </>
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
  const n = name.trim();
  const label = from === "HEAD" ? "HEAD" : shortRef(from);
  const submit = () => {
    onClose();
    void run("Create branch", () => api.createBranch(n, from, switchTo), switchTo ? `Switched to new branch ${n}` : `Created ${n} from ${label}`);
  };
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
      <BaseSelect value={from} onChange={setFrom} branches={branches} head={base === "HEAD"} />
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
        <Select autoFocus value={upstream} onChange={(e) => setUpstream(e.target.value)} className="mt-4 w-full font-mono">
          {remotes.map((r) => (
            <option key={r} value={r}>
              {r}
            </option>
          ))}
        </Select>
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
