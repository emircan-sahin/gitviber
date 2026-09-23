import { ask } from "@tauri-apps/plugin-dialog";
import { ArrowLeftToLine, ArrowRightToLine, Check, ChevronDown, Copy, Diff, EyeOff, File, FolderGit2, FolderSearch, GitMerge, History, ListTree, Minus, Plus, SquareCheck, Undo2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuTrigger } from "@/components/ui/context-menu";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Tip } from "@/components/ui/tooltip";
import { api, errorMessage, type FileChange, type RepoStatus } from "@/lib/api";
import { ignorePattern } from "@/lib/gitignore";
import { matchesCommand, useCommands, useShortcut } from "@/lib/keybindings";
import { type Selection, selectionKey } from "@/lib/selection";
import { toast } from "@/lib/toast";
import { tracked, undoAction } from "@/lib/undo";
import { cn } from "@/lib/utils";
import { NESTED_EXPLAINED, stageable } from "@/lib/worktrees";
import { FileIcon } from "./FileIcon";
import { LineCounts, PathLabel, StatusLetter } from "./StatusBadge";

interface Props {
  status: RepoStatus;
  activeKey: string | null;
  onOpen: (s: Selection, pin?: boolean) => void;
  /** Hovering a row starts loading it, so the click feels instant. */
  onHover: (s: Selection) => void;
  refresh: () => Promise<void>;
  viewed: (s: Selection) => boolean;
  setViewed: (s: Selection[], on: boolean) => void;
  /** Shows the file in the explorer, opening the panel if it's hidden. */
  onRevealInExplorer: (path: string) => void;
  /** History, filtered to this file's commits. */
  onShowHistory: (path: string) => void;
}

type Change = Selection & { kind: "conflict" | "staged" | "unstaged" };

async function attempt(title: string, fn: () => Promise<unknown>) {
  try {
    await fn();
    return true;
  } catch (e) {
    toast("error", title, errorMessage(e));
    return false;
  }
}

/** Every reviewable change in display order; J/K walk this list. Nested repos have no diff to review. */
export function changeList(status: RepoStatus): Change[] {
  return [
    ...status.conflicted.map((file) => ({ kind: "conflict" as const, file })),
    ...status.staged.map((file) => ({ kind: "staged" as const, file })),
    ...status.unstaged.filter((f) => !f.nested).map((file) => ({ kind: "unstaged" as const, file })),
  ];
}

const leftOut = (n: number) => `Left out ${n} nested ${n === 1 ? "repository" : "repositories"}`;
const files = (n: number) => `${n} ${n === 1 ? "file" : "files"}`;
const paths = (rows: Change[]) => rows.map((r) => r.file.path);

export function ChangesPanel({ status, activeKey, onOpen, onHover, refresh, viewed, setViewed, onRevealInExplorer, onShowHistory }: Props) {
  const act = async (title: string, fn: () => Promise<unknown>) => {
    await attempt(title, fn);
    await refresh();
  };

  const stageAll = () => {
    const { paths, skipped } = stageable(status.unstaged);
    if (skipped) toast("info", leftOut(skipped), NESTED_EXPLAINED);
    if (paths.length) act("Stage failed", () => api.stage(paths));
  };

  const stage = (rows: Change[]) => act("Stage failed", () => api.stage(paths(rows)));
  const unstage = (rows: Change[]) => act("Unstage failed", () => api.unstage(paths(rows)));
  const resolve = (rows: Change[], side: "ours" | "theirs") =>
    act("Resolve failed", async () => {
      for (const r of rows) await api.resolveSide(r.file.path, side);
    });

  // Nested repos can't be marked viewed, so every path here is stageable.
  const viewedPaths = status.unstaged.filter((file) => !file.nested && viewed({ kind: "unstaged", file })).map((f) => f.path);

  // Untracked files have nothing to restore; like VS Code, discarding one deletes it (to the Trash here).
  const discard = async (list: FileChange[]) => {
    const tracked = list.filter((f) => f.status !== "?");
    const untracked = list.filter((f) => f.status === "?");
    if (!list.length) return;
    const one = list.length === 1 ? list[0].path : null;
    const trashOnly = !tracked.length;
    const message = trashOnly
      ? one
        ? `Move ${one} to the Trash? It is untracked, so git has no copy of it.`
        : `Move ${untracked.length} untracked files to the Trash? Git has no copy of them.`
      : `Discard changes to ${one ?? files(tracked.length)}? This cannot be undone.${untracked.length ? ` ${files(untracked.length)} git doesn't track will be moved to the Trash.` : ""}`;
    const ok = await ask(message, trashOnly ? { title: one ? "Delete file" : "Delete files", kind: "warning", okLabel: "Move to Trash" } : { title: "Discard changes", kind: "warning", okLabel: "Discard" });
    if (!ok) return;
    await act(trashOnly ? "Could not move to Trash" : "Discard failed", async () => {
      if (tracked.length) await api.discard(tracked.map((f) => f.path));
      for (const f of untracked) await api.trashPath(f.path);
    });
  };

  const ignore = (list: FileChange[]) =>
    act("Could not update .gitignore", async () => {
      const cur = await api.readFile(".gitignore");
      if (cur.exists && (cur.binary || cur.lossy || cur.tooLarge)) throw new Error(".gitignore is not a plain text file");
      const sep = cur.text && !cur.text.endsWith("\n") ? "\n" : "";
      const lines = [...new Set(list.map((f) => ignorePattern(f.path)))].join("\n");
      await api.writeFile(".gitignore", `${cur.text}${sep}${lines}\n`);
    });

  const copy = (text: string, what: string) =>
    navigator.clipboard.writeText(text).then(
      () => toast("success", what),
      (e) => toast("error", "Could not copy", errorMessage(e)),
    );

  const all = changeList(status);
  const index = new Map(all.map((c, i) => [selectionKey(c), i]));
  const firstOfPath = new Map<string, Change>();
  for (const c of all) if (!firstOfPath.has(c.file.path)) firstOfPath.set(c.file.path, c);
  // A picked file that got staged or unstaged is found in its new list, like its tab.
  const here = (c: Change): Change | undefined => all[index.get(selectionKey(c)) ?? -1] ?? firstOfPath.get(c.file.path);
  const active = activeKey === null ? undefined : all[index.get(activeKey) ?? -1];

  // Rows picked with ⌘/⇧ around `focus`, the file the open tab was on. Once the tab moves to
  // another file (a plain click, J/K), the selection is just the open row again.
  const [picked, setPicked] = useState<{ rows: Change[]; anchor: Change; focus: string } | null>(null);
  const stale = !!picked && !!active && active.file.path !== picked.focus;
  const [listFocused, setListFocused] = useState(false);
  useEffect(() => {
    if (stale) setPicked(null);
  }, [stale]);
  const selected = new Map<string, Change>();
  for (const c of picked && !stale ? picked.rows : active ? [active] : []) {
    const row = here(c);
    if (row) selected.set(selectionKey(row), row);
  }
  const anchor = (picked && !stale && here(picked.anchor)) || active;
  const selectedOf: Record<Change["kind"], Change[]> = { conflict: [], staged: [], unstaged: [] };
  for (const c of selected.values()) selectedOf[c.kind].push(c);
  // With several rows of a section selected, its header acts on them instead of the whole section.
  const many = (kind: Change["kind"]) => (selectedOf[kind].length > 1 ? selectedOf[kind] : null);
  const [pickedConflicts, pickedStaged, pickedChanges] = [many("conflict"), many("staged"), many("unstaged")];
  /** What an action on a row covers: the selected rows of its kind if it's selected, else just the row. */
  const targets = (c: Change) => (selected.has(selectionKey(c)) ? selectedOf[c.kind] : [c]);

  const range = (from: Change, to: Change) => {
    const [i, j] = [index.get(selectionKey(from)) ?? -1, index.get(selectionKey(to)) ?? -1];
    return i < 0 || j < 0 ? [to] : all.slice(Math.min(i, j), Math.max(i, j) + 1);
  };

  // ⌘-click toggles a row, ⇧-click picks the range from the anchor. The open tab follows the clicked row either way.
  const pick = (c: Change, e: { metaKey: boolean; shiftKey: boolean }) => {
    const key = selectionKey(c);
    if (e.metaKey) setPicked({ rows: selected.has(key) ? [...selected.values()].filter((s) => selectionKey(s) !== key) : [...selected.values(), c], anchor: c, focus: c.file.path });
    else if (e.shiftKey && anchor) setPicked({ rows: range(anchor, c), anchor, focus: c.file.path });
    else setPicked(null);
    onOpen(c);
  };

  // Set by "Reveal in Explorer", so the closing menu doesn't pull focus back from the tree.
  const keepFocus = useRef(false);

  /** The row's right-click menu, modeled on VS Code's Source Control view. `rows`: what its git actions cover. */
  const menu = (sel: Change, rows: Change[]) => {
    const { file } = sel;
    const onDisk = file.status !== "D";
    const n = rows.length;
    const untracked = rows.filter((r) => r.file.status === "?").map((r) => r.file);
    const isViewed = viewed(sel);
    return (
      <ContextMenuContent
        onCloseAutoFocus={(e) => {
          if (keepFocus.current) e.preventDefault();
          keepFocus.current = false;
        }}
      >
        <ContextMenuItem onSelect={() => onOpen(sel, true)}>
          {sel.kind === "conflict" ? <GitMerge /> : <Diff />} {sel.kind === "conflict" ? "Open Conflict" : "Open Changes"}
        </ContextMenuItem>
        <ContextMenuItem disabled={!onDisk} onSelect={() => onOpen({ kind: "file", path: file.path }, true)}>
          <File /> Open File
        </ContextMenuItem>
        <ContextMenuItem disabled={file.status === "?"} onSelect={() => onShowHistory(file.path)}>
          <History /> Show History
        </ContextMenuItem>
        <ContextMenuSeparator />
        {sel.kind === "unstaged" && (
          <>
            <ContextMenuItem onSelect={() => stage(rows)}>
              <Plus /> {n > 1 ? `Stage ${n} Files` : "Stage Changes"}
            </ContextMenuItem>
            <ContextMenuItem onSelect={() => discard(rows.map((r) => r.file))}>
              <Undo2 /> {n > 1 ? `Discard ${n} Files…` : "Discard Changes"}
            </ContextMenuItem>
            {untracked.length > 0 && (
              <ContextMenuItem onSelect={() => ignore(untracked)}>
                <EyeOff /> {n > 1 ? `Add ${untracked.length} to .gitignore` : "Add to .gitignore"}
              </ContextMenuItem>
            )}
          </>
        )}
        {sel.kind === "staged" && (
          <ContextMenuItem onSelect={() => unstage(rows)}>
            <Minus /> {n > 1 ? `Unstage ${n} Files` : "Unstage Changes"}
          </ContextMenuItem>
        )}
        {sel.kind === "conflict" && (
          <>
            <ContextMenuItem onSelect={() => stage(rows)}>
              <Check /> {n > 1 ? `Mark ${n} as Resolved` : "Mark as Resolved"}
            </ContextMenuItem>
            <ContextMenuItem onSelect={() => resolve(rows, "ours")}>
              <ArrowLeftToLine /> {n > 1 ? `Take Current Version of ${n} Files` : "Take Current Version"}
            </ContextMenuItem>
            <ContextMenuItem onSelect={() => resolve(rows, "theirs")}>
              <ArrowRightToLine /> {n > 1 ? `Take Incoming Version of ${n} Files` : "Take Incoming Version"}
            </ContextMenuItem>
          </>
        )}
        {sel.kind === "unstaged" && (
          <ContextMenuItem onSelect={() => setViewed(rows, !isViewed)}>
            <SquareCheck /> {`Mark ${n > 1 ? `${n} ` : ""}as ${isViewed ? "Not Viewed" : "Viewed"}`}
          </ContextMenuItem>
        )}
        <ContextMenuSeparator />
        <ContextMenuItem
          disabled={!onDisk}
          onSelect={() => {
            keepFocus.current = true;
            onRevealInExplorer(file.path);
          }}
        >
          <ListTree /> Reveal in Explorer View
        </ContextMenuItem>
        <ContextMenuItem disabled={!onDisk} onSelect={() => api.revealPath(file.path).catch((e) => toast("error", "Could not reveal in Finder", errorMessage(e)))}>
          <FolderSearch /> Reveal in Finder
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem onSelect={() => copy(paths(rows).map((p) => `${status.root}/${p}`).join("\n"), n > 1 ? `${n} paths copied` : "Path copied")}>
          <Copy /> {n > 1 ? "Copy Paths" : "Copy Path"}
        </ContextMenuItem>
        <ContextMenuItem onSelect={() => copy(paths(rows).join("\n"), n > 1 ? `${n} relative paths copied` : "Relative path copied")}>
          <Copy /> {n > 1 ? "Copy Relative Paths" : "Copy Relative Path"}
        </ContextMenuItem>
      </ContextMenuContent>
    );
  };

  const reviewed = all.filter(viewed).length;
  const add = all.reduce((n, s) => n + (s.file.additions ?? 0), 0);
  const del = all.reduce((n, s) => n + (s.file.deletions ?? 0), 0);

  useCommands({
    // The tab and the selection follow the files into the other list, so pressing it again undoes it. Conflicts are left to their own actions.
    "git.toggleStage": active?.kind === "unstaged" ? () => stage(targets(active)) : active?.kind === "staged" ? () => unstage(targets(active)) : undefined,
    "git.discard": active?.kind === "unstaged" ? () => discard(targets(active).map((r) => r.file)) : undefined,
    // Only while several rows are selected: registering then puts it over Workspace's V, which marks
    // just the open file. Staged rows stay out, as there: unmarking one unstages it.
    "review.toggleViewed": active && active.kind !== "staged" && targets(active).length > 1 ? () => setViewed(targets(active), !viewed(active)) : undefined,
  });

  // One tab stop for the whole list (the active row), so Tab reaches its actions, not every row.
  const tabStop = active ? activeKey : all[0] && selectionKey(all[0]);

  // ↑/↓ from a focused row (clicking one focuses it), ⇧ to extend the selection, ⌘A for all of it, Esc
  // to let it go; ↵ keeps the preview tab, Space opens it like a click.
  const onListKey = (e: React.KeyboardEvent) => {
    const key = e.target instanceof HTMLElement ? e.target.dataset.row : undefined;
    const i = key === undefined ? -1 : (index.get(key) ?? -1);
    if (i < 0 || e.altKey || e.ctrlKey) return;
    const cur = all[i];
    if (e.key === "Escape") {
      // Only when there's a selection to drop; otherwise Esc isn't ours to take.
      if (!picked || stale) return;
      setPicked(null);
    } else if (e.metaKey) {
      if (e.shiftKey || e.key.toLowerCase() !== "a") return;
      setPicked({ rows: all, anchor: anchor ?? cur, focus: (active ?? cur).file.path });
      if (!active) onOpen(cur);
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      // A focused row that isn't open yet (Tab into a list with nothing open) opens first.
      const to = all[Math.max(0, Math.min(all.length - 1, key !== activeKey ? i : e.key === "ArrowDown" ? i + 1 : i - 1))];
      if (e.shiftKey) setPicked({ rows: range(anchor ?? cur, to), anchor: anchor ?? cur, focus: to.file.path });
      else setPicked(null);
      onOpen(to);
    } else if (e.shiftKey) return;
    else if (e.key === "Enter") onOpen(cur, true);
    else if (e.key === " ") pick(cur, e);
    else return;
    e.preventDefault();
  };

  const row = (sel: Change, actions: (rows: Change[]) => React.ReactNode) => {
    const key = selectionKey(sel);
    const rows = targets(sel);
    const n = rows.length > 1 ? `${rows.length} ` : "";
    const isViewed = viewed(sel);
    return (
      <Row
        key={key}
        sel={sel}
        active={activeKey === key}
        selected={selected.has(key)}
        dim={!listFocused && activeKey !== key}
        tabStop={tabStop === key}
        viewed={isViewed}
        checkLabel={sel.kind === "staged" ? (n ? `Unstage ${files(rows.length)}` : "Unstage") : `Mark ${n}as ${isViewed ? "not viewed" : "viewed"}`}
        onClick={(e) => pick(sel, e)}
        onOpen={onOpen}
        onHover={onHover}
        onToggleViewed={() => setViewed(rows, !isViewed)}
        menu={menu(sel, rows)}
      >
        {actions(rows)}
      </Row>
    );
  };

  return (
    <div className="flex h-full flex-col">
      {status.operation && <OperationBanner status={status} refresh={refresh} />}
      {all.length > 0 && (
        <div className="shrink-0 border-b border-border px-3 py-2">
          <div className="flex items-center gap-2 text-[11.5px]">
            <span className="text-muted-foreground">
              <span className="font-semibold text-foreground">{all.length}</span> files
            </span>
            <span className="font-mono text-[11px]">
              <span className="text-added">+{add}</span> <span className="text-removed">-{del}</span>
            </span>
            <span className="ml-auto text-muted-foreground">
              <span className={cn("font-semibold", reviewed === all.length ? "text-added" : "text-foreground")}>{reviewed}</span>/{all.length} reviewed
            </span>
          </div>
          <div className="mt-1.5 h-[3px] overflow-hidden bg-border">
            <div className="h-full bg-added transition-[width] duration-300" style={{ width: `${(reviewed / all.length) * 100}%` }} />
          </div>
        </div>
      )}
      <div
        onKeyDown={onListKey}
        // React focus events bubble out of portals too, so a row's open context menu still counts as the list.
        onFocus={() => setListFocused(true)}
        onBlur={(e) => setListFocused(e.currentTarget.contains(e.relatedTarget))}
        // The empty space below the rows lets go of the selection, like Finder.
        onClick={(e) => e.target === e.currentTarget && setPicked(null)}
        className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto pb-2 outline-none">
        {!all.length && !status.unstaged.length && <AllCaughtUp />}
        {status.conflicted.length > 0 && (
          <Section
            title="Conflicts"
            count={status.conflicted.length}
            tone="text-conflict"
            pinned={!!pickedConflicts}
            action={pickedConflicts && <SectionBtn onClick={() => stage(pickedConflicts)}>Mark {files(pickedConflicts.length)} resolved</SectionBtn>}
          >
            {status.conflicted.map((file) =>
              row({ kind: "conflict", file }, (rows) => (
                <>
                  <RowAction label={rows.length > 1 ? `Mark ${files(rows.length)} resolved as they are` : "Mark resolved as it is"} onClick={() => stage(rows)}>
                    <Check />
                  </RowAction>
                  <RowAction label={rows.length > 1 ? `Take current version of ${files(rows.length)}` : "Take current version"} onClick={() => resolve(rows, "ours")}>
                    <ArrowLeftToLine />
                  </RowAction>
                  <RowAction label={rows.length > 1 ? `Take incoming version of ${files(rows.length)}` : "Take incoming version"} onClick={() => resolve(rows, "theirs")}>
                    <ArrowRightToLine />
                  </RowAction>
                </>
              )),
            )}
          </Section>
        )}
        {status.staged.length > 0 && (
          <Section
            title="Staged"
            count={status.staged.length}
            pinned={!!pickedStaged}
            action={
              pickedStaged ? (
                <SectionBtn onClick={() => unstage(pickedStaged)}>Unstage {files(pickedStaged.length)}</SectionBtn>
              ) : (
                <SectionBtn onClick={() => act("Unstage failed", () => api.unstage(status.staged.map((f) => f.path)))}>Unstage all</SectionBtn>
              )
            }
          >
            {status.staged.map((file) =>
              row({ kind: "staged", file }, (rows) => (
                <RowAction label={rows.length > 1 ? `Unstage ${files(rows.length)}` : "Unstage"} onClick={() => unstage(rows)}>
                  <Minus />
                </RowAction>
              )),
            )}
          </Section>
        )}
        {status.unstaged.length > 0 && (
          <Section
            title="Changes"
            count={status.unstaged.length}
            pinned={!!pickedChanges}
            action={
              pickedChanges ? (
                <>
                  <SectionBtn onClick={() => discard(pickedChanges.map((r) => r.file))}>Discard {files(pickedChanges.length)}…</SectionBtn>
                  <SectionBtn onClick={() => stage(pickedChanges)}>Stage {files(pickedChanges.length)}</SectionBtn>
                </>
              ) : (
                <>
                  {/* Leaves untracked files alone; deleting one is a per-file choice. */}
                  <SectionBtn onClick={() => discard(status.unstaged.filter((f) => f.status !== "?"))}>Discard</SectionBtn>
                  {viewedPaths.length > 0 && <SectionBtn onClick={() => act("Stage failed", () => api.stage(viewedPaths))}>Stage {viewedPaths.length} viewed</SectionBtn>}
                  <SectionBtn onClick={stageAll}>Stage all</SectionBtn>
                </>
              )
            }
          >
            {status.unstaged.map((file) =>
              file.nested ? (
                <NestedRow key={file.path} file={file} />
              ) : (
                row({ kind: "unstaged", file }, (rows) => (
                  <>
                    {file.status !== "?" && (
                      <RowAction label={rows.length > 1 ? `Discard ${files(rows.length)}` : "Discard changes"} onClick={() => discard(rows.map((r) => r.file))}>
                        <Undo2 />
                      </RowAction>
                    )}
                    <RowAction label={rows.length > 1 ? `Stage ${files(rows.length)}` : "Stage"} onClick={() => stage(rows)}>
                      <Plus />
                    </RowAction>
                  </>
                ))
              ),
            )}
          </Section>
        )}
      </div>
      {status.operation ? (
        // Committing by hand mid-rebase would splice an extra commit into the history.
        <div className="shrink-0 border-t border-border bg-panel px-3 py-2.5 text-[11.5px] text-muted-foreground">
          A {status.operation.kind} is in progress. Resolve the conflicts, then use <span className="font-medium text-foreground">Continue</span> above.
        </div>
      ) : (
        <CommitBox status={status} refresh={refresh} />
      )}
    </div>
  );
}

const OP_LABEL = { merge: "Merging", rebase: "Rebasing", "cherry-pick": "Cherry-picking", revert: "Reverting" } as const;

/** Shown while a merge/rebase waits for the user: what's happening, what's left, and the way out. */
function OperationBanner({ status, refresh }: Pick<Props, "status" | "refresh">) {
  const op = status.operation!;
  const [busy, setBusy] = useState(false);
  const left = status.conflicted.length;
  const run = async (title: string, fn: () => Promise<boolean | void>) => {
    setBusy(true);
    try {
      const [stopped, entry] = await tracked(fn);
      if (stopped) toast("info", "Stopped on new conflicts", "Resolve them to continue.");
      // Finished: the entry is the whole merge or rebase, from where it started.
      else if (entry !== null) toast("success", `${op.kind[0].toUpperCase()}${op.kind.slice(1)} finished`, undefined, undoAction(entry, refresh));
    } catch (e) {
      toast("error", title, errorMessage(e));
    } finally {
      setBusy(false);
      await refresh();
    }
  };
  const abort = async () => {
    const ok = await ask(`Abort the ${op.kind}? Your branch goes back to how it was before it started.`, { title: `Abort ${op.kind}`, kind: "warning", okLabel: "Abort" });
    if (ok) await run("Abort failed", api.opAbort);
  };
  return (
    <div className="shrink-0 border-b border-conflict/40 bg-conflict/10 px-3 py-2">
      <div className="flex items-center gap-2 text-[12px]">
        <GitMerge className="size-3.5 shrink-0 text-conflict" />
        <span className="font-semibold">{OP_LABEL[op.kind]}</span>
        {op.step != null && op.total != null && (
          <span className="font-mono text-[11px] text-muted-foreground">
            {op.step}/{op.total}
          </span>
        )}
        {op.subject && <span className="min-w-0 truncate text-muted-foreground">{op.subject}</span>}
      </div>
      <div className="mt-1 text-[11.5px] text-muted-foreground">
        {left ? `${left} conflict${left > 1 ? "s" : ""} left. Resolve them, then continue.` : "All conflicts resolved. Continue to finish."}
      </div>
      <div className="mt-2 flex gap-1">
        <Button size="sm" className="flex-1" disabled={busy || left > 0} onClick={() => run("Continue failed", api.opContinue)}>
          Continue
        </Button>
        {op.kind === "rebase" && (
          <Button variant="secondary" size="sm" disabled={busy} onClick={() => run("Skip failed", api.rebaseSkip)}>
            Skip commit
          </Button>
        )}
        <Button variant="destructive" size="sm" disabled={busy} onClick={abort}>
          Abort
        </Button>
      </div>
    </div>
  );
}

/** `pinned`: the actions stay visible instead of showing on hover (they act on a selection the user just made). */
function Section({ title, count, tone, action, pinned, children }: { title: string; count: number; tone?: string; action?: React.ReactNode; pinned?: boolean; children: React.ReactNode }) {
  const [open, setOpen] = useState(true);
  return (
    <div>
      <div className="group sticky top-0 z-10 flex h-7 items-center gap-1 border-b border-border bg-panel pr-1.5 pl-2">
        <button className="flex items-center gap-1 text-[10.5px] font-semibold tracking-[0.08em] text-subtle uppercase hover:text-foreground" onClick={() => setOpen(!open)}>
          <ChevronDown className={cn("size-3 transition-transform", !open && "-rotate-90")} />
          <span className={tone}>{title}</span>
          <span className="ml-1 font-mono tracking-normal text-muted-foreground">{count}</span>
        </button>
        <div className={cn("ml-auto flex gap-0.5", !pinned && "opacity-0 group-focus-within:opacity-100 group-hover:opacity-100")}>{action}</div>
      </div>
      {open && <div className="py-0.5">{children}</div>}
    </div>
  );
}

function SectionBtn({ onClick, children }: { onClick: () => void; children: React.ReactNode }) {
  return (
    <button onClick={onClick} className="h-5 rounded-sm px-1.5 text-[11px] text-muted-foreground outline-none hover:bg-active hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring">
      {children}
    </button>
  );
}

function Row({
  sel,
  active,
  selected,
  dim,
  tabStop,
  viewed,
  checkLabel,
  onClick,
  onOpen,
  onHover,
  onToggleViewed,
  menu,
  children,
}: {
  sel: Change;
  active: boolean;
  selected: boolean;
  /** Selected while focus is elsewhere: shown fainter, like VS Code's inactive selection. The open row keeps its color. */
  dim: boolean;
  tabStop: boolean;
  viewed: boolean;
  checkLabel: string;
  onClick: (e: React.MouseEvent) => void;
  onOpen: (s: Selection, pin?: boolean) => void;
  onHover: (s: Selection) => void;
  onToggleViewed: () => void;
  menu: React.ReactNode;
  children?: React.ReactNode;
}) {
  const file = sel.file;
  const ref = useRef<HTMLDivElement>(null);
  // J/K can move the selection off-screen; follow it. ↑/↓ from a row also moves focus to it.
  useEffect(() => {
    if (!active) return;
    ref.current?.scrollIntoView({ block: "nearest" });
    if (document.activeElement instanceof HTMLElement && document.activeElement.dataset.row !== undefined) ref.current?.focus();
  }, [active]);
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <div
          ref={ref}
          role="button"
          tabIndex={tabStop ? 0 : -1}
          data-row={selectionKey(sel)}
          aria-current={active || undefined}
          onClick={onClick}
          onDoubleClick={() => onOpen(sel, true)}
          onMouseEnter={() => onHover(sel)}
          className={cn(
            "group/row relative flex h-[26px] cursor-pointer items-center gap-2 pr-2 pl-2 text-[12px] outline-none focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset",
            selected ? (dim ? "bg-active" : "bg-primary/15") : "hover:bg-hover data-[state=open]:bg-hover",
          )}
        >
          {active && <span className="absolute inset-y-0 left-0 w-0.5 bg-primary" />}
          {sel.kind === "conflict" ? (
            <GitMerge className="size-3.5 shrink-0 text-conflict" />
          ) : (
          <Tip label={checkLabel}>
            <button
              role="checkbox"
              tabIndex={tabStop ? undefined : -1}
              aria-checked={viewed}
              aria-label={sel.kind === "staged" ? "Staged" : "Viewed"}
              onClick={(e) => {
                e.stopPropagation();
                onToggleViewed();
              }}
              className={cn(
                "flex size-3.5 shrink-0 items-center justify-center rounded-[3px] border outline-none focus-visible:ring-1 focus-visible:ring-ring",
                viewed ? "border-added-fill bg-added-fill text-on-status" : "border-border-strong hover:border-muted-foreground",
              )}
            >
              {viewed && <Check className="size-2.5" strokeWidth={3} />}
            </button>
          </Tip>
          )}
          <FileIcon path={file.path} />
          <PathLabel path={file.path} className={cn("flex-1", viewed && "opacity-45")} />
          {/* Shown on the active row too, so Tab can reach them without a mouse. */}
          <LineCounts file={file} className={active ? "hidden" : "group-focus-within/row:hidden group-hover/row:hidden"} />
          <div className={cn("items-center", active ? "flex" : "hidden group-focus-within/row:flex group-hover/row:flex")} onClick={(e) => e.stopPropagation()}>
            {children}
          </div>
          <StatusLetter status={file.status} />
        </div>
      </ContextMenuTrigger>
      {menu}
    </ContextMenu>
  );
}

/**
 * An untracked folder that is another repository (not one of ours: worktrees stay out of
 * status). Git lists it, so we do too, but it has no diff here and can't be staged.
 */
function NestedRow({ file }: { file: FileChange }) {
  return (
    <div className="group/row relative flex h-[26px] cursor-default items-center gap-2 pr-2 pl-2 text-[12px]">
      <span className="size-3.5 shrink-0" />
      <FolderGit2 className="size-4 shrink-0 text-subtle" />
      <PathLabel path={file.path.replace(/\/$/, "")} className="flex-1" />
      <span className="max-w-32 shrink-0 truncate rounded-sm bg-elevated px-1 font-mono text-[10.5px] leading-4 text-muted-foreground group-hover/row:hidden">
        nested repo
      </span>
      <div className="hidden items-center group-hover/row:flex">
        <RowAction label="Can't stage a separate git repository" onClick={() => toast("info", "Not stageable", NESTED_EXPLAINED)}>
          <Plus className="opacity-40" />
        </RowAction>
      </div>
      <StatusLetter status={file.status} />
    </div>
  );
}

function RowAction({ label, onClick, children }: { label: string; onClick: () => void; children: React.ReactNode }) {
  return (
    <Tip label={label}>
      <button
        onClick={onClick}
        aria-label={label}
        className="flex size-5 items-center justify-center rounded-sm text-muted-foreground outline-none hover:bg-active hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring [&_svg]:size-3.5"
      >
        {children}
      </button>
    </Tip>
  );
}

function AllCaughtUp() {
  return (
    <div className="flex flex-col items-center gap-1.5 px-6 pt-20 text-center">
      <Check className="mb-1 size-6 text-border-strong" />
      <div className="text-[12.5px] text-muted-foreground">Working tree clean</div>
      <div className="text-[11.5px] leading-relaxed text-subtle">Anything your agent writes shows up here instantly.</div>
    </div>
  );
}

function CommitBox({ status, refresh }: Pick<Props, "status" | "refresh">) {
  const [summary, setSummary] = useState("");
  const [body, setBody] = useState("");
  const [amend, setAmend] = useState(false);
  const [busy, setBusy] = useState(false);

  const hasStaged = status.staged.length > 0;
  const all = stageable(status.unstaged);
  const hasAny = hasStaged || all.paths.length > 0;
  const canCommit = !busy && !status.conflicted.length && (amend || (summary.trim() !== "" && hasAny));
  const label = amend ? "Amend" : hasStaged ? "Commit" : "Commit all";
  // The button stays short; the tooltip still says how much goes in.
  const scope = hasStaged && !amend ? `Commit ${status.staged.length} staged` : label;
  const target = status.branch ? `${scope} to ${status.branch}` : scope;
  const skipped = !hasStaged && !amend && all.skipped > 0;

  const commit = async () => {
    if (!canCommit) return;
    setBusy(true);
    const message = body.trim() ? `${summary.trim()}\n\n${body.trim()}` : summary.trim();
    let entry: number | null = null;
    const ok = await attempt("Commit failed", async () => {
      // Nothing staged means "commit everything", the common case after an agent run.
      if (!hasStaged && !amend) await api.stage(all.paths);
      [, entry] = await tracked(() => api.commit(message, amend));
    });
    setBusy(false);
    if (ok) {
      setSummary("");
      setBody("");
      setAmend(false);
      toast("success", amend ? "Commit amended" : "Committed", message.split("\n")[0], undoAction(entry, refresh));
      // They stay in the list after the commit; say why rather than leave it looking missed.
      if (skipped) toast("info", `${leftOut(all.skipped)} of the commit`, NESTED_EXPLAINED);
    }
    await refresh();
  };

  useCommands({ "git.commit": canCommit ? commit : undefined });
  const commitKey = useShortcut("git.commit");
  const onKey = (e: React.KeyboardEvent) => {
    if (matchesCommand("git.commit", e.nativeEvent)) {
      e.preventDefault();
      commit();
    }
  };

  return (
    <div className="shrink-0 border-t border-border bg-panel p-2">
      <Input placeholder="Summary" value={summary} onChange={(e) => setSummary(e.target.value)} onKeyDown={onKey} className="font-medium" />
      <Textarea placeholder="Description" value={body} onChange={(e) => setBody(e.target.value)} onKeyDown={onKey} rows={2} className="mt-1.5 py-1.5 text-[12px]" />
      <div className="mt-1.5 flex items-center gap-2">
        <label className="flex items-center gap-1.5 text-[11.5px] text-muted-foreground">
          <input type="checkbox" checked={amend} onChange={(e) => setAmend(e.target.checked)} className="accent-primary" />
          Amend
        </label>
        <Tip label={skipped ? `${target} (${leftOut(all.skipped).toLowerCase()})` : target} shortcut={commitKey}>
          <Button className="ml-auto flex-1" disabled={!canCommit} onClick={commit}>
            {busy ? "Committing…" : label}
          </Button>
        </Tip>
      </div>
    </div>
  );
}
