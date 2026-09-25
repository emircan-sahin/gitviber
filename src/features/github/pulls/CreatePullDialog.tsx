import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Tip } from "@/components/ui/tooltip";
import { api, type Branch, errorMessage, fullName, type GitHubAccess, github, type Pull, type RepoStatus } from "@/lib/api";
import { toast } from "@/lib/app/toast";
import { cn } from "@/lib/utils";
import { MarkdownInput } from "@/features/github/shared/MarkdownInput";
import { notifyPullsChanged } from "@/features/github/shared/changed";
import { Select } from "@/components/ui/select";

/**
 * `git push` sends the branch somewhere other than origin, where a PR's branch must be. Out of
 * the box a branch pushes where it pulls from: a fork's dev tracking upstream/dev, into the original.
 */
const pushesElsewhere = (s: RepoStatus) => (s.push && s.push.remote !== "origin" ? s.push.remote : null);

/** GitHub's title for a PR of several commits: the branch name, spaced and capitalized. */
const branchTitle = (branch: string) => {
  const t = branch.replace(/[-_]+/g, " ");
  return t.charAt(0).toUpperCase() + t.slice(1);
};

export function CreatePullDialog({
  target,
  origin,
  status,
  branches,
  defaultTitle,
  onClose,
  onCreated,
  onPushChanged,
}: {
  /** Where the PR goes: origin, or a fork's parent. */
  target: GitHubAccess;
  origin: GitHubAccess;
  status: RepoStatus;
  branches: Branch[];
  defaultTitle: string;
  onClose: () => void;
  onCreated: (p: Pull) => void;
  /** After push settings change: re-read the status. */
  onPushChanged: () => Promise<void>;
}) {
  const head = status.branch!;
  const upstream = fullName(target.repo) !== fullName(origin.repo);
  // The repo's default branch is always offered (and preselected), even if not fetched locally.
  // origin/* says nothing about the parent's branches: upstream, only its default is offered.
  const remote = upstream ? [] : branches.filter((b) => b.remote && b.name.startsWith("origin/")).map((b) => b.name.slice(7));
  const bases = [...new Set([target.defaultBranch, ...remote])].filter((b): b is string => !!b && (upstream || b !== head) && b !== "HEAD");
  const [title, setTitle] = useState(defaultTitle);
  const [body, setBody] = useState("");
  // Once typed in, the fields are the user's; the draft below stops filling them.
  const [typed, setTyped] = useState({ title: false, body: false });
  const [base, setBase] = useState(bases[0] ?? "main");
  const [draft, setDraft] = useState(false);
  const [maintainerEdits, setMaintainerEdits] = useState(true);
  const [busy, setBusy] = useState(false);

  // GitHub's defaults: one commit titles the PR with its subject and fills the body; more
  // take the branch name. Counted against the chosen base in the repository the PR goes to.
  useEffect(() => {
    let live = true;
    (async () => {
      const remote = upstream ? await github.originalRemote(fullName(target.repo), false) : "origin";
      if (!remote) return;
      const d = await api.pullDraft(`refs/remotes/${remote}/${base}`);
      if (!live) return;
      const one = d.commits === 1 && d.subject;
      if (!typed.title) setTitle(one ? d.subject! : branchTitle(head));
      if (!typed.body) setBody(one ? (d.body ?? "") : "");
    })().catch(() => {});
    return () => {
      live = false;
    };
    // Recounted per base; typing doesn't recount.
  }, [base, head, upstream, target.repo.owner, target.repo.name]);
  const elsewhere = pushesElsewhere(status);
  // Pushed already, and nothing new since: GitHub has the branch as it is.
  const needsPush = !status.push?.branch || status.push.ahead > 0;
  const [fixing, setFixing] = useState(false);
  const pushToOrigin = async () => {
    setFixing(true);
    try {
      await api.setPushDefault("origin");
      await onPushChanged();
    } catch (e) {
      toast("error", "Could not change where branches push", errorMessage(e));
    } finally {
      setFixing(false);
    }
  };

  const submit = async () => {
    setBusy(true);
    try {
      // GitHub can only open a PR from a branch it has; publish or push the latest first.
      if (needsPush) await api.push();
      onCreated(await github.create(upstream ? fullName(target.repo) : null, title.trim(), body, head, base, draft, maintainerEdits));
      toast("success", "Pull request created");
      notifyPullsChanged();
    } catch (e) {
      toast("error", "Could not create pull request", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-w-lg">
        <DialogTitle>New pull request</DialogTitle>
        <DialogDescription>
          <span className="font-mono">{upstream ? `${origin.repo.owner}:${head}` : head}</span> →{" "}
          <span className="font-mono">{upstream ? `${fullName(target.repo)}:${base}` : base}</span>
          {!elsewhere && needsPush && " · the branch will be pushed to origin first"}
        </DialogDescription>
        <form
          className="mt-4 space-y-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (title.trim() && !elsewhere) submit();
          }}
        >
          {elsewhere && (
            <div className="flex items-center gap-2 rounded-sm bg-removed/10 px-2 py-1.5 text-[11.5px] text-removed">
              <span className="min-w-0 flex-1">
                {head} pushes to <span className="font-mono">{elsewhere}</span>, but a pull request needs it on origin. Pulling can stay with {elsewhere}; only pushes
                move.
              </span>
              <Button type="button" size="sm" variant="secondary" className="shrink-0" disabled={fixing} onClick={pushToOrigin}>
                Push to origin from now on
              </Button>
            </div>
          )}
          <Input
            autoFocus
            value={title}
            onChange={(e) => {
              setTitle(e.target.value);
              setTyped((t) => ({ ...t, title: true }));
            }} placeholder="Title" />
          <MarkdownInput
            pull={{ url: `https://github.com/${fullName(target.repo)}`, number: null }}
            value={body}
            onChange={(e) => {
              setBody(e.target.value);
              setTyped((t) => ({ ...t, body: true }));
            }} placeholder="Description (markdown)" rows={6} />
          <div className="flex items-center gap-2 text-[12px]">
            <span className="text-muted-foreground">Base</span>
            <Select value={base} onChange={(e) => setBase(e.target.value)} className="font-mono text-[11.5px]">
              {(bases.length ? bases : [base]).map((b) => (
                <option key={b}>{b}</option>
              ))}
            </Select>
            {upstream && (
              <Tip label="Lets the original's maintainers push to your branch, as GitHub offers">
                <label className="ml-auto flex items-center gap-1.5 text-muted-foreground">
                  <input type="checkbox" checked={maintainerEdits} onChange={(e) => setMaintainerEdits(e.target.checked)} className="accent-primary" /> Maintainer edits
                </label>
              </Tip>
            )}
            <label className={cn("flex items-center gap-1.5 text-muted-foreground", !upstream && "ml-auto")}>
              <input type="checkbox" checked={draft} onChange={(e) => setDraft(e.target.checked)} className="accent-primary" /> Draft
            </label>
            <Button type="submit" disabled={busy || !title.trim() || !!elsewhere}>
              {busy ? "Creating…" : "Create"}
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  );
}
