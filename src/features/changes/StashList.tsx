import { ask } from "@tauri-apps/plugin-dialog";
import { Archive, ArchiveRestore, ChevronRight, GitBranchPlus, PackageOpen, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuTrigger } from "@/components/ui/context-menu";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { api, type Commit, errorMessage, type FileChange, type RepoStatus, type Stash, type StashFiles } from "@/lib/api";
import { type Selection, selectionKey } from "@/lib/repo/selection";
import { toast } from "@/lib/app/toast";
import { useListNav } from "@/lib/ui/useListNav";
import { cn } from "@/lib/utils";
import { relativeTime } from "@/lib/format";
import { FileIcon } from "@/components/FileIcon";
import { LineCounts, PathLabel, StatusLetter } from "@/components/StatusBadge";
import { RowAction } from "@/components/RowAction";
import { useGitAction } from "@/hooks/useGitAction";
import { useAsyncValue } from "@/hooks/useAsyncValue";

/** The stash list, reread whenever status is: `git stash list` is one cheap reflog walk. */
export function useStashes(status: RepoStatus) {
  return useAsyncValue(api.stashes, [status], []);
}

/** "On main: message" → message and branch; "WIP on main: abc123 subject" keeps its subject. */
function describe(s: Stash) {
  const m = /^(WIP on|On) (.+?): (.*)$/.exec(s.message);
  if (!m) return { text: s.message, branch: null };
  return { text: m[1] === "WIP on" ? `WIP: ${m[3].replace(/^[0-9a-f]+ /, "")}` : m[3], branch: m[2] };
}

/** A stash is a commit: its files open like a commit's, against the commit it was made on. */
const asCommit = (s: Stash, sha: string): Commit => ({
  sha,
  shortSha: `stash@{${s.index}}`,
  authorName: s.author,
  authorEmail: "",
  timestamp: s.timestamp,
  committerName: s.author,
  committedAt: s.timestamp,
  parents: [],
  refs: [],
  subject: describe(s).text,
  body: "",
  unpushed: false,
  onOrigin: false,
  notInHead: false,
});

interface Props {
  stashes: Stash[];
  activeKey: string | null;
  onOpen: (s: Selection, pin?: boolean) => void;
  onHover: (s: Selection) => void;
  refresh: () => Promise<void>;
}

/** Stashes, newest first: apply, pop or drop one, or open it to see its files. */
export function StashList({ stashes, activeKey, onOpen, onHover, refresh }: Props) {
  const [open, setOpen] = useState<string | null>(null);
  const [files, setFiles] = useState<Record<string, StashFiles>>({});
  // Conflicts from an apply or pop land in Changes like a merge's; git keeps the stash then.
  const { busy, run: act } = useGitAction({ refresh, tracked: false, conflicts: "Resolve them in Changes. The stash is kept; drop it once you're done." });
  const [branching, setBranching] = useState<Stash | null>(null);
  const nav = useListNav({ activeKey });

  useEffect(() => {
    if (!open || files[open]) return;
    let live = true;
    api.stashFiles(open).then(
      (f) => live && setFiles((m) => ({ ...m, [open]: f })),
      (e) => live && toast("error", "Could not load stash", errorMessage(e)),
    );
    return () => {
      live = false;
    };
  }, [open, files]);

  const apply = (s: Stash, pop: boolean) => act(pop ? "Pop" : "Apply", () => api.stashApply(s.sha, pop), pop ? "Stash popped" : "Stash applied");
  const drop = async (s: Stash) => {
    const ok = await ask(`Drop "${describe(s).text}"? Its changes are thrown away; GitViber can't undo this.`, { title: "Drop stash", kind: "warning", okLabel: "Drop" });
    // Until git's garbage collection, the commit is still there to apply by its id.
    if (ok) await act("Drop", () => api.stashDrop(s.sha), "Stash dropped", `To get it back: git stash apply ${s.sha.slice(0, 10)}`);
  };

  if (!stashes.length) return <div className="py-1 pl-8 text-[11.5px] text-subtle">Nothing stashed</div>;

  const fileRow = (s: Stash, sha: string, f: FileChange) => {
    const sel: Selection = { kind: "commit", commit: asCommit(s, sha), file: f };
    const active = activeKey === selectionKey(sel);
    return (
      <div
        key={`${sha}:${f.path}`}
        role="treeitem"
        aria-level={2}
        aria-selected={active}
        tabIndex={-1}
        data-row={selectionKey(sel)}
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
  };

  return (
    <div role="tree" aria-label="Stashes" {...nav}>
      {stashes.map((s) => {
        const { text, branch } = describe(s);
        const expanded = open === s.sha;
        const list = files[s.sha];
        return (
          <div key={s.sha}>
            <ContextMenu>
              <ContextMenuTrigger asChild>
                <div
                  role="treeitem"
                  aria-level={1}
                  aria-expanded={expanded}
                  tabIndex={-1}
                  data-row={`stash:${s.sha}`}
                  onClick={() => setOpen(expanded ? null : s.sha)}
                  className={cn(
                    "group/row flex h-[26px] cursor-pointer items-center gap-1.5 pr-2 pl-2 text-[12px] outline-none focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset data-[state=open]:bg-hover",
                    expanded ? "bg-active" : "hover:bg-hover focus:bg-hover",
                  )}
                >
                  <ChevronRight className={cn("size-3 shrink-0 text-subtle transition-transform", expanded && "rotate-90")} />
                  <Archive className="size-3.5 shrink-0 text-subtle" />
                  <span className="min-w-0 flex-1 truncate" title={s.message}>
                    {text}
                  </span>
                  <span className="shrink-0 text-[10.5px] text-subtle group-focus-within/row:hidden group-hover/row:hidden">
                    {branch && <span className="font-mono">{branch} · </span>}
                    {relativeTime(s.timestamp)}
                  </span>
                  <div className="hidden items-center group-focus-within/row:flex group-hover/row:flex" onClick={(e) => e.stopPropagation()}>
                    <RowAction label="Apply (keep the stash)" disabled={!!busy} onClick={() => apply(s, false)}>
                      <PackageOpen />
                    </RowAction>
                    <RowAction label="Pop (apply, then drop it)" disabled={!!busy} onClick={() => apply(s, true)}>
                      <ArchiveRestore />
                    </RowAction>
                    <RowAction label="Drop…" disabled={!!busy} onClick={() => drop(s)}>
                      <Trash2 />
                    </RowAction>
                  </div>
                </div>
              </ContextMenuTrigger>
              <ContextMenuContent>
                <ContextMenuItem disabled={!!busy} onSelect={() => apply(s, false)}>
                  <PackageOpen /> Apply stash
                </ContextMenuItem>
                <ContextMenuItem disabled={!!busy} onSelect={() => apply(s, true)}>
                  <ArchiveRestore /> Pop stash
                </ContextMenuItem>
                <ContextMenuItem disabled={!!busy} onSelect={() => setBranching(s)}>
                  <GitBranchPlus /> Create Branch from Stash…
                </ContextMenuItem>
                <ContextMenuSeparator />
                <ContextMenuItem disabled={!!busy} className="text-destructive" onSelect={() => drop(s)}>
                  <Trash2 /> Drop stash…
                </ContextMenuItem>
              </ContextMenuContent>
            </ContextMenu>
            {expanded && (
              <div className="border-y border-border bg-panel py-0.5">
                {!list && <div className="py-1 pl-8 text-[11.5px] text-subtle">Loading…</div>}
                {list?.files.map((f) => fileRow(s, s.sha, f))}
                {list?.untrackedSha && list.untracked.map((f) => fileRow(s, list.untrackedSha!, f))}
              </div>
            )}
          </div>
        );
      })}
      {branching && (
        <StashBranchDialog
          stash={branching}
          onClose={() => setBranching(null)}
          onCreate={(name) => act("Branch from stash", () => api.stashBranch(name, branching.sha), `Switched to new branch ${name}`, "The stash was applied there and dropped.")}
        />
      )}
    </div>
  );
}

/** A name for the branch `git stash branch` makes where the stash was taken. */
function StashBranchDialog({ stash, onClose, onCreate }: { stash: Stash; onClose: () => void; onCreate: (name: string) => void }) {
  const [name, setName] = useState("");
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogTitle>Create branch from stash</DialogTitle>
        <DialogDescription>
          A new branch at the commit “{describe(stash).text}” was made on, with its changes applied there. They can't clash with what came since; the stash is dropped once applied.
        </DialogDescription>
        <form
          className="mt-4 flex gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (!name.trim()) return;
            onClose();
            onCreate(name.trim());
          }}
        >
          <Input autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="Branch name" />
          <Button type="submit" disabled={!name.trim()}>
            Create
          </Button>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/**
 * Sets local changes aside: tracked ones always, untracked files when asked, or only what's
 * staged. With `paths`, only those files (untracked ones among them go too).
 */
export function StashDialog({ status, paths = [], onClose, refresh }: { status: RepoStatus; paths?: string[]; onClose: () => void; refresh: () => Promise<void> }) {
  const [message, setMessage] = useState("");
  const [untracked, setUntracked] = useState(false);
  const [staged, setStaged] = useState(false);
  // Nested repositories are never stashed; git skips them.
  const untrackedCount = status.unstaged.filter((f) => f.status === "?" && !f.nested).length;
  const some = paths.length > 0;
  const someUntracked = some && status.unstaged.some((f) => f.status === "?" && paths.includes(f.path));
  const submit = async () => {
    onClose();
    try {
      await api.stashPush(message, some ? someUntracked : untracked && !staged, !some && staged, paths);
      toast("success", some ? `Stashed ${paths.length === 1 ? paths[0] : `${paths.length} files`}` : "Changes stashed", message.trim() || undefined);
    } catch (e) {
      toast("error", "Stash failed", errorMessage(e));
    } finally {
      await refresh();
    }
  };
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogTitle>{some ? `Stash ${paths.length === 1 ? paths[0] : `${paths.length} files`}` : "Stash changes"}</DialogTitle>
        <DialogDescription>
          {some ? "Sets the changes to these files aside and leaves the rest as they are." : "Sets your uncommitted changes aside and leaves the working tree clean."} Apply or pop them later from
          Stashes.
        </DialogDescription>
        <form
          className="mt-4"
          onSubmit={(e) => {
            e.preventDefault();
            void submit();
          }}
        >
          <Input autoFocus value={message} onChange={(e) => setMessage(e.target.value)} placeholder="Message (optional)" />
          <div className="mt-3 flex items-center gap-3">
            {!some && (
              <>
                <label className={cn("flex items-center gap-1.5 text-[12px]", (!untrackedCount || staged) && "text-subtle")}>
                  <input type="checkbox" disabled={!untrackedCount || staged} checked={untracked && !staged} onChange={(e) => setUntracked(e.target.checked)} className="accent-primary" />
                  Include {untrackedCount ? `${untrackedCount} untracked ${untrackedCount === 1 ? "file" : "files"}` : "untracked files"}
                </label>
                <label className={cn("flex items-center gap-1.5 text-[12px]", !status.staged.length && "text-subtle")}>
                  <input type="checkbox" disabled={!status.staged.length} checked={staged} onChange={(e) => setStaged(e.target.checked)} className="accent-primary" />
                  Only staged changes
                </label>
              </>
            )}
            <Button type="submit" className="ml-auto">
              Stash
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  );
}
