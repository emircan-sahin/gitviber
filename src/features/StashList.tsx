import { ask } from "@tauri-apps/plugin-dialog";
import { Archive, ArchiveRestore, ChevronRight, PackageOpen, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuTrigger } from "@/components/ui/context-menu";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Tip } from "@/components/ui/tooltip";
import { api, type Commit, errorMessage, type FileChange, type RepoStatus, type Stash, type StashFiles } from "@/lib/api";
import { type Selection, selectionKey } from "@/lib/selection";
import { toast } from "@/lib/toast";
import { cn, relativeTime } from "@/lib/utils";
import { FileIcon } from "./FileIcon";
import { LineCounts, PathLabel, StatusLetter } from "./StatusBadge";

/** The stash list, reread whenever status is: `git stash list` is one cheap reflog walk. */
export function useStashes(status: RepoStatus) {
  const [list, setList] = useState<Stash[]>([]);
  useEffect(() => {
    let live = true;
    api.stashes().then(
      (l) => live && setList(l),
      () => {},
    );
    return () => {
      live = false;
    };
  }, [status]);
  return list;
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
  const [busy, setBusy] = useState(false);

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

  // Conflicts from an apply or pop land in Changes like a merge's; git keeps the stash then.
  const act = async (label: string, fn: () => Promise<boolean | void>, done: string, detail?: string) => {
    setBusy(true);
    try {
      if (await fn()) toast("info", `${label} stopped on conflicts`, "Resolve them in Changes. The stash is kept; drop it once you're done.");
      else toast("success", done, detail);
    } catch (e) {
      toast("error", `${label} failed`, errorMessage(e));
    } finally {
      setBusy(false);
      await refresh();
    }
  };
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
  };

  return (
    <>
      {stashes.map((s) => {
        const { text, branch } = describe(s);
        const expanded = open === s.sha;
        const list = files[s.sha];
        return (
          <div key={s.sha}>
            <ContextMenu>
              <ContextMenuTrigger asChild>
                <div
                  role="button"
                  aria-expanded={expanded}
                  onClick={() => setOpen(expanded ? null : s.sha)}
                  className={cn("group/row flex h-[26px] cursor-pointer items-center gap-1.5 pr-2 pl-2 text-[12px] data-[state=open]:bg-hover", expanded ? "bg-active" : "hover:bg-hover")}
                >
                  <ChevronRight className={cn("size-3 shrink-0 text-subtle transition-transform", expanded && "rotate-90")} />
                  <Archive className="size-3.5 shrink-0 text-subtle" />
                  <span className="min-w-0 flex-1 truncate" title={s.message}>
                    {text}
                  </span>
                  <span className="shrink-0 text-[10.5px] text-subtle group-hover/row:hidden">
                    {branch && <span className="font-mono">{branch} · </span>}
                    {relativeTime(s.timestamp)}
                  </span>
                  <div className="hidden items-center group-hover/row:flex" onClick={(e) => e.stopPropagation()}>
                    <StashAction label="Apply (keep the stash)" disabled={busy} onClick={() => apply(s, false)}>
                      <PackageOpen />
                    </StashAction>
                    <StashAction label="Pop (apply, then drop it)" disabled={busy} onClick={() => apply(s, true)}>
                      <ArchiveRestore />
                    </StashAction>
                    <StashAction label="Drop…" disabled={busy} onClick={() => drop(s)}>
                      <Trash2 />
                    </StashAction>
                  </div>
                </div>
              </ContextMenuTrigger>
              <ContextMenuContent>
                <ContextMenuItem disabled={busy} onSelect={() => apply(s, false)}>
                  <PackageOpen /> Apply stash
                </ContextMenuItem>
                <ContextMenuItem disabled={busy} onSelect={() => apply(s, true)}>
                  <ArchiveRestore /> Pop stash
                </ContextMenuItem>
                <ContextMenuSeparator />
                <ContextMenuItem disabled={busy} className="text-destructive" onSelect={() => drop(s)}>
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
    </>
  );
}

function StashAction({ label, disabled, onClick, children }: { label: string; disabled: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <Tip label={label}>
      <button
        onClick={onClick}
        disabled={disabled}
        aria-label={label}
        className="flex size-5 items-center justify-center rounded-sm text-muted-foreground outline-none hover:bg-active hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-40 [&_svg]:size-3.5"
      >
        {children}
      </button>
    </Tip>
  );
}

/** Sets local changes aside: tracked ones always, untracked files when asked. */
export function StashDialog({ status, onClose, refresh }: { status: RepoStatus; onClose: () => void; refresh: () => Promise<void> }) {
  const [message, setMessage] = useState("");
  const [untracked, setUntracked] = useState(false);
  // Nested repositories are never stashed; git skips them.
  const untrackedCount = status.unstaged.filter((f) => f.status === "?" && !f.nested).length;
  const submit = async () => {
    onClose();
    try {
      await api.stashPush(message, untracked);
      toast("success", "Changes stashed", message.trim() || undefined);
    } catch (e) {
      toast("error", "Stash failed", errorMessage(e));
    } finally {
      await refresh();
    }
  };
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogTitle>Stash changes</DialogTitle>
        <DialogDescription>Sets your uncommitted changes aside and leaves the working tree clean. Apply or pop them later from Stashes.</DialogDescription>
        <form
          className="mt-4"
          onSubmit={(e) => {
            e.preventDefault();
            void submit();
          }}
        >
          <Input autoFocus value={message} onChange={(e) => setMessage(e.target.value)} placeholder="Message (optional)" />
          <div className="mt-3 flex items-center gap-2">
            <label className={cn("flex items-center gap-1.5 text-[12px]", !untrackedCount && "text-subtle")}>
              <input type="checkbox" disabled={!untrackedCount} checked={untracked} onChange={(e) => setUntracked(e.target.checked)} className="accent-primary" />
              Include {untrackedCount ? `${untrackedCount} untracked ${untrackedCount === 1 ? "file" : "files"}` : "untracked files"}
            </label>
            <Button type="submit" className="ml-auto">
              Stash
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  );
}
