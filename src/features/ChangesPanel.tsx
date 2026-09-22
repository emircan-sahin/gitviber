import { ask } from "@tauri-apps/plugin-dialog";
import { ArrowLeftToLine, ArrowRightToLine, Check, ChevronDown, Copy, Diff, EyeOff, File, FolderGit2, FolderSearch, GitMerge, ListTree, Minus, Plus, SquareCheck, Undo2 } from "lucide-react";
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
  toggleViewed: (s: Selection) => void;
  /** Shows the file in the explorer, opening the panel if it's hidden. */
  onRevealInExplorer: (path: string) => void;
}

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
export function changeList(status: RepoStatus): (Selection & { kind: "conflict" | "staged" | "unstaged" })[] {
  return [
    ...status.conflicted.map((file) => ({ kind: "conflict" as const, file })),
    ...status.staged.map((file) => ({ kind: "staged" as const, file })),
    ...status.unstaged.filter((f) => !f.nested).map((file) => ({ kind: "unstaged" as const, file })),
  ];
}

const leftOut = (n: number) => `Left out ${n} nested ${n === 1 ? "repository" : "repositories"}`;

export function ChangesPanel({ status, activeKey, onOpen, onHover, refresh, viewed, toggleViewed, onRevealInExplorer }: Props) {
  const act = async (title: string, fn: () => Promise<unknown>) => {
    await attempt(title, fn);
    await refresh();
  };

  const stageAll = () => {
    const { paths, skipped } = stageable(status.unstaged);
    if (skipped) toast("info", leftOut(skipped), NESTED_EXPLAINED);
    if (paths.length) act("Stage failed", () => api.stage(paths));
  };

  // Nested repos can't be marked viewed, so every path here is stageable.
  const viewedPaths = status.unstaged.filter((file) => !file.nested && viewed({ kind: "unstaged", file })).map((f) => f.path);

  const discard = async (files: FileChange[]) => {
    const tracked = files.filter((f) => f.status !== "?");
    if (!tracked.length) return;
    const what = tracked.length === 1 ? tracked[0].path : `${tracked.length} files`;
    const ok = await ask(`Discard changes to ${what}? This cannot be undone.`, { title: "Discard changes", kind: "warning", okLabel: "Discard" });
    if (ok) await act("Discard failed", () => api.discard(tracked.map((f) => f.path)));
  };

  // Untracked files have nothing to restore; like VS Code, discarding one deletes it (to the Trash here).
  const trash = async (file: FileChange) => {
    const ok = await ask(`Move ${file.path} to the Trash? It is untracked, so git has no copy of it.`, { title: "Delete file", kind: "warning", okLabel: "Move to Trash" });
    if (ok) await act("Could not move to Trash", () => api.trashPath(file.path));
  };

  const discardOne = (file: FileChange) => (file.status === "?" ? trash(file) : discard([file]));

  const ignore = (file: FileChange) =>
    act("Could not update .gitignore", async () => {
      const cur = await api.readFile(".gitignore");
      if (cur.exists && (cur.binary || cur.lossy || cur.tooLarge)) throw new Error(".gitignore is not a plain text file");
      const sep = cur.text && !cur.text.endsWith("\n") ? "\n" : "";
      await api.writeFile(".gitignore", `${cur.text}${sep}${ignorePattern(file.path)}\n`);
    });

  const copy = (text: string, what: string) =>
    navigator.clipboard.writeText(text).then(
      () => toast("success", what),
      (e) => toast("error", "Could not copy", errorMessage(e)),
    );

  // Set by "Reveal in Explorer", so the closing menu doesn't pull focus back from the tree.
  const keepFocus = useRef(false);

  /** The row's right-click menu, modeled on VS Code's Source Control view. */
  const menu = (sel: Selection & { kind: "staged" | "unstaged" | "conflict" }) => {
    const { file } = sel;
    const onDisk = file.status !== "D";
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
        <ContextMenuSeparator />
        {sel.kind === "unstaged" && (
          <>
            <ContextMenuItem onSelect={() => act("Stage failed", () => api.stage([file.path]))}>
              <Plus /> Stage Changes
            </ContextMenuItem>
            <ContextMenuItem onSelect={() => discardOne(file)}>
              <Undo2 /> Discard Changes
            </ContextMenuItem>
            {file.status === "?" && (
              <ContextMenuItem onSelect={() => ignore(file)}>
                <EyeOff /> Add to .gitignore
              </ContextMenuItem>
            )}
          </>
        )}
        {sel.kind === "staged" && (
          <ContextMenuItem onSelect={() => act("Unstage failed", () => api.unstage([file.path]))}>
            <Minus /> Unstage Changes
          </ContextMenuItem>
        )}
        {sel.kind === "conflict" && (
          <>
            <ContextMenuItem onSelect={() => act("Stage failed", () => api.stage([file.path]))}>
              <Check /> Mark as Resolved
            </ContextMenuItem>
            <ContextMenuItem onSelect={() => act("Resolve failed", () => api.resolveSide(file.path, "ours"))}>
              <ArrowLeftToLine /> Take Current Version
            </ContextMenuItem>
            <ContextMenuItem onSelect={() => act("Resolve failed", () => api.resolveSide(file.path, "theirs"))}>
              <ArrowRightToLine /> Take Incoming Version
            </ContextMenuItem>
          </>
        )}
        {sel.kind === "unstaged" && (
          <ContextMenuItem onSelect={() => toggleViewed(sel)}>
            <SquareCheck /> {viewed(sel) ? "Mark as Not Viewed" : "Mark as Viewed"}
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
        <ContextMenuItem onSelect={() => copy(`${status.root}/${file.path}`, "Path copied")}>
          <Copy /> Copy Path
        </ContextMenuItem>
        <ContextMenuItem onSelect={() => copy(file.path, "Relative path copied")}>
          <Copy /> Copy Relative Path
        </ContextMenuItem>
      </ContextMenuContent>
    );
  };

  const all = changeList(status);
  const reviewed = all.filter(viewed).length;
  const add = all.reduce((n, s) => n + (s.file.additions ?? 0), 0);
  const del = all.reduce((n, s) => n + (s.file.deletions ?? 0), 0);

  const active = all.find((c) => selectionKey(c) === activeKey);
  useCommands({
    // The tab follows the file into the other list, so pressing it again undoes it. Conflicts are left to their own actions.
    "git.toggleStage":
      active?.kind === "unstaged"
        ? () => act("Stage failed", () => api.stage([active.file.path]))
        : active?.kind === "staged"
          ? () => act("Unstage failed", () => api.unstage([active.file.path]))
          : undefined,
    "git.discard": active?.kind === "unstaged" ? () => discardOne(active.file) : undefined,
  });

  // One tab stop for the whole list (the active row), so Tab reaches its actions, not every row.
  const tabStop = active ? activeKey : all[0] && selectionKey(all[0]);

  // ↑/↓ from a focused row (clicking one focuses it); ↵ keeps the preview tab, Space opens it like a click.
  const onListKey = (e: React.KeyboardEvent) => {
    const key = e.target instanceof HTMLElement ? e.target.dataset.row : undefined;
    if (key === undefined || e.altKey || e.metaKey || e.ctrlKey || e.shiftKey) return;
    const i = all.findIndex((c) => selectionKey(c) === key);
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      // A focused row that isn't open yet (Tab into a list with nothing open) opens first.
      const to = key !== activeKey ? i : e.key === "ArrowDown" ? i + 1 : i - 1;
      onOpen(all[Math.max(0, Math.min(all.length - 1, to))]);
    } else if (e.key === "Enter") onOpen(all[i], true);
    else if (e.key === " ") onOpen(all[i]);
    else return;
    e.preventDefault();
  };

  const row = (sel: Selection & { kind: "staged" | "unstaged" | "conflict" }, actions: React.ReactNode) => (
    <Row
      key={selectionKey(sel)}
      sel={sel}
      active={activeKey === selectionKey(sel)}
      tabStop={tabStop === selectionKey(sel)}
      viewed={viewed(sel)}
      onOpen={onOpen}
      onHover={onHover}
      onToggleViewed={() => toggleViewed(sel)}
      menu={menu(sel)}
    >
      {actions}
    </Row>
  );

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
      <div onKeyDown={onListKey} className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto pb-2 outline-none">
        {!all.length && !status.unstaged.length && <AllCaughtUp />}
        {status.conflicted.length > 0 && (
          <Section title="Conflicts" count={status.conflicted.length} tone="text-conflict">
            {status.conflicted.map((file) =>
              row(
                { kind: "conflict", file },
                <>
                  <RowAction label="Mark resolved as it is" onClick={() => act("Stage failed", () => api.stage([file.path]))}>
                    <Check />
                  </RowAction>
                  <RowAction label="Take current version" onClick={() => act("Resolve failed", () => api.resolveSide(file.path, "ours"))}>
                    <ArrowLeftToLine />
                  </RowAction>
                  <RowAction label="Take incoming version" onClick={() => act("Resolve failed", () => api.resolveSide(file.path, "theirs"))}>
                    <ArrowRightToLine />
                  </RowAction>
                </>,
              ),
            )}
          </Section>
        )}
        {status.staged.length > 0 && (
          <Section
            title="Staged"
            count={status.staged.length}
            action={<SectionBtn onClick={() => act("Unstage failed", () => api.unstage(status.staged.map((f) => f.path)))}>Unstage all</SectionBtn>}
          >
            {status.staged.map((file) =>
              row(
                { kind: "staged", file },
                <RowAction label="Unstage" onClick={() => act("Unstage failed", () => api.unstage([file.path]))}>
                  <Minus />
                </RowAction>,
              ),
            )}
          </Section>
        )}
        {status.unstaged.length > 0 && (
          <Section
            title="Changes"
            count={status.unstaged.length}
            action={
              <>
                <SectionBtn onClick={() => discard(status.unstaged)}>Discard</SectionBtn>
                {viewedPaths.length > 0 && <SectionBtn onClick={() => act("Stage failed", () => api.stage(viewedPaths))}>Stage {viewedPaths.length} viewed</SectionBtn>}
                <SectionBtn onClick={stageAll}>Stage all</SectionBtn>
              </>
            }
          >
            {status.unstaged.map((file) =>
              file.nested ? (
                <NestedRow key={file.path} file={file} />
              ) : (
              row(
                { kind: "unstaged", file },
                <>
                  {file.status !== "?" && (
                    <RowAction label="Discard changes" onClick={() => discard([file])}>
                      <Undo2 />
                    </RowAction>
                  )}
                  <RowAction label="Stage" onClick={() => act("Stage failed", () => api.stage([file.path]))}>
                    <Plus />
                  </RowAction>
                </>,
              )
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

function Section({ title, count, tone, action, children }: { title: string; count: number; tone?: string; action?: React.ReactNode; children: React.ReactNode }) {
  const [open, setOpen] = useState(true);
  return (
    <div>
      <div className="group sticky top-0 z-10 flex h-7 items-center gap-1 border-b border-border bg-panel pr-1.5 pl-2">
        <button className="flex items-center gap-1 text-[10.5px] font-semibold tracking-[0.08em] text-subtle uppercase hover:text-foreground" onClick={() => setOpen(!open)}>
          <ChevronDown className={cn("size-3 transition-transform", !open && "-rotate-90")} />
          <span className={tone}>{title}</span>
          <span className="ml-1 font-mono tracking-normal text-muted-foreground">{count}</span>
        </button>
        <div className="ml-auto flex gap-0.5 opacity-0 group-focus-within:opacity-100 group-hover:opacity-100">{action}</div>
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
  tabStop,
  viewed,
  onOpen,
  onHover,
  onToggleViewed,
  menu,
  children,
}: {
  sel: Selection & { kind: "staged" | "unstaged" | "conflict" };
  active: boolean;
  tabStop: boolean;
  viewed: boolean;
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
          onClick={() => onOpen(sel)}
          onDoubleClick={() => onOpen(sel, true)}
          onMouseEnter={() => onHover(sel)}
          className={cn(
            "group/row relative flex h-[26px] cursor-pointer items-center gap-2 pr-2 pl-2 text-[12px] outline-none focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset",
            active ? "bg-primary/15" : "hover:bg-hover data-[state=open]:bg-hover",
          )}
        >
          {active && <span className="absolute inset-y-0 left-0 w-0.5 bg-primary" />}
          {sel.kind === "conflict" ? (
            <GitMerge className="size-3.5 shrink-0 text-conflict" />
          ) : (
          <Tip label={sel.kind === "staged" ? "Unstage" : viewed ? "Mark as not viewed" : "Mark as viewed"}>
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
