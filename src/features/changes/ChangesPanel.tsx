import { ask } from "@tauri-apps/plugin-dialog";
import { ArrowLeftToLine, ArrowRightToLine, Check, Minus, Plus, Undo2 } from "lucide-react";
import { useEffect, useState } from "react";
import { useListFilter } from "@/components/ListFilter";
import { Windowed } from "@/components/Windowed";
import { api, type Commit, type FileChange, type RepoStatus } from "@/lib/api";
import { ignorePattern } from "@/lib/git/gitignore";
import { focusPanel } from "@/lib/ui/panels";
import { isMenuKey, moveTarget, openRowMenu, pageOf } from "@/lib/ui/useListNav";
import { matchesCommand, useCommands } from "@/lib/commands/keybindings";
import { type Selection, selectionKey } from "@/lib/repo/selection";
import { toast } from "@/lib/app/toast";
import { tracked, undoAction } from "@/lib/repo/undo";
import { NESTED_EXPLAINED, stageable } from "@/lib/git/worktrees";
import { StashDialog, StashList, useStashes } from "./StashList";
import { BisectBar } from "@/features/history/BisectBar";
import { SubmoduleList, updateSubmodules, useSubmodules } from "./SubmoduleList";
import { attempt, type Change, changeList, files, filtered, leftOut, paths, sumLines } from "./changeList";
import { ChangeRowMenu } from "./ChangeRowMenu";
import { OperationBanner } from "./OperationBanner";
import { AllCaughtUp, NestedRow, ReviewSummary, Row, Section, SectionBtn } from "./ChangeRows";
import { CommitBox } from "./CommitBox";
import { RowAction } from "@/components/RowAction";
import { primaryKey } from "@/lib/platform";

interface Props {
  status: RepoStatus;
  /** HEAD's commit, for Amend; null before the first commit or while history loads. */
  head: Commit | null;
  /** The main worktree: sign-off is kept per repository, across its worktrees. */
  main: string;
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

// Row and NestedRow are h-[26px].
const ROW_HEIGHT = 26;

export function ChangesPanel({ status: full, head, main, activeKey, onOpen, onHover, refresh, viewed, setViewed, onRevealInExplorer, onShowHistory }: Props) {
  // The list and its section actions (Stage all, Discard) cover the files the filter leaves, and
  // say so ("Stage 3 shown"); the commit takes hidden ones too and says how many.
  const filter = useListFilter("git", "Filter changed files");
  const filtering = !!filter.needle;
  const status = filtering ? filtered(full, (f) => filter.matches(f.path, f.oldPath)) : full;
  const allOrShown = (verb: string, n: number) => (filtering ? `${verb} ${n} shown` : `${verb} all`);
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
  // Tracked ones keep a copy of what they were in the Trash, which Undo (and ⌘Z) writes back.
  const discard = async (list: FileChange[]) => {
    const restorable = list.filter((f) => f.status !== "?");
    const untracked = list.filter((f) => f.status === "?");
    if (!list.length) return;
    const one = list.length === 1 ? list[0].path : null;
    const trashOnly = !restorable.length;
    const message = trashOnly
      ? one
        ? `Move ${one} to the Trash? It is untracked, so git has no copy of it.`
        : `Move ${untracked.length} untracked files to the Trash? Git has no copy of them.`
      : `Discard changes to ${one ?? files(restorable.length)}? ${restorable.length === 1 ? "Its current version is" : "Their current versions are"} moved to the Trash.${untracked.length ? ` ${files(untracked.length)} git doesn't track will be moved to the Trash too.` : ""}`;
    const ok = await ask(message, trashOnly ? { title: one ? "Delete file" : "Delete files", kind: "warning", okLabel: "Move to Trash" } : { title: "Discard changes", kind: "warning", okLabel: "Discard" });
    if (!ok) return;
    let entry: number | null = null;
    const done = await attempt(trashOnly ? "Could not move to Trash" : "Discard failed", async () => {
      if (restorable.length) [, entry] = await tracked(() => api.discard(restorable.map((f) => f.path)));
      for (const f of untracked) await api.trashPath(f.path);
    });
    if (done && restorable.length) {
      const single = restorable.length === 1;
      toast("success", `Discarded ${single ? restorable[0].path : files(restorable.length)}`, `The old ${single ? "version is" : "versions are"} in the Trash.`, undoAction(entry, refresh));
    }
    await refresh();
  };

  const ignore = (list: FileChange[]) =>
    act("Could not update .gitignore", async () => {
      const cur = await api.readFile(".gitignore");
      if (cur.exists && (cur.binary || cur.lossy || cur.tooLarge)) throw new Error(".gitignore is not a plain text file");
      const sep = cur.text && !cur.text.endsWith("\n") ? "\n" : "";
      const lines = [...new Set(list.map((f) => ignorePattern(f.path)))].join("\n");
      await api.writeFile(".gitignore", `${cur.text}${sep}${lines}\n`);
    });

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

  // ⌘-click (Ctrl off macOS) toggles a row, ⇧-click picks the range from the anchor. The open tab follows the clicked row either way.
  const pick = (c: Change, e: { metaKey: boolean; ctrlKey: boolean; shiftKey: boolean }) => {
    const key = selectionKey(c);
    if (primaryKey(e)) setPicked({ rows: selected.has(key) ? [...selected.values()].filter((s) => selectionKey(s) !== key) : [...selected.values(), c], anchor: c, focus: c.file.path });
    else if (e.shiftKey && anchor) setPicked({ rows: range(anchor, c), anchor, focus: c.file.path });
    else setPicked(null);
    onOpen(c);
  };

  const stashes = useStashes(full);
  const submodules = useSubmodules(full);
  // The files to stash, or all of them ([]); null: not stashing.
  const [stashing, setStashing] = useState<string[] | null>(null);

  // The review's progress, whatever the filter shows.
  const total = changeList(full);
  const reviewed = total.filter(viewed).length;
  const { add, del } = sumLines(total.map((c) => c.file));

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

  // ↑/↓ (Home/End, PageUp/PageDown) from a focused row (clicking one focuses it), ⇧ to extend the
  // selection, ⌘A (git.selectAllChanges) for all of it, Esc to let it go; ↵ keeps the preview tab, Space opens it like a
  // click, → goes to its code, ⇧F10 opens its menu. The other lists share the moves through useListNav.
  const onListKey = (e: React.KeyboardEvent) => {
    const key = e.target instanceof HTMLElement ? e.target.dataset.row : undefined;
    const i = key === undefined ? -1 : (index.get(key) ?? -1);
    if (i < 0) return;
    const cur = all[i];
    if (matchesCommand("git.selectAllChanges", e.nativeEvent)) {
      e.preventDefault();
      setPicked({ rows: all, anchor: anchor ?? cur, focus: (active ?? cur).file.path });
      if (!active) onOpen(cur);
      return;
    }
    if (e.altKey || e.ctrlKey) return;
    const move = e.metaKey ? null : moveTarget(e.key, i, all.length, pageOf(e.target as HTMLElement));
    if (isMenuKey(e)) openRowMenu(e.target as HTMLElement);
    else if (e.key === "Escape") {
      // Only when there's a selection to drop; otherwise Esc isn't ours to take.
      if (!picked || stale) return;
      setPicked(null);
    } else if (e.metaKey) return;
    else if (move !== null) {
      // A focused row that isn't open yet (Tab into a list with nothing open) opens first.
      const to = all[key !== activeKey ? i : move];
      if (e.shiftKey) setPicked({ rows: range(anchor ?? cur, to), anchor: anchor ?? cur, focus: to.file.path });
      else setPicked(null);
      onOpen(to);
    } else if (e.shiftKey) return;
    else if (e.key === "ArrowRight") {
      if (key !== activeKey) onOpen(cur);
      focusPanel("code");
    } else if (e.key === "Enter") onOpen(cur, true);
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
        menu={() => (
          <ChangeRowMenu
            sel={sel}
            rows={rows}
            root={status.root}
            canStash={!status.operation}
            viewed={viewed}
            setViewed={setViewed}
            onOpen={onOpen}
            onShowHistory={onShowHistory}
            onRevealInExplorer={onRevealInExplorer}
            stage={stage}
            unstage={unstage}
            discard={discard}
            ignore={ignore}
            resolve={resolve}
            stash={setStashing}
          />
        )}
      >
        {actions(rows)}
      </Row>
    );
  };

  /** A section's rows; with thousands, only those near the screen (and the open and tab-stop rows). */
  const rowsOf = (kind: Change["kind"], list: FileChange[], render: (file: FileChange) => React.ReactNode) => {
    const keep = [activeKey, tabStop].map((k) => list.findIndex((file) => selectionKey({ kind, file }) === k));
    return <Windowed count={list.length} height={ROW_HEIGHT} keep={keep} render={(i) => render(list[i])} />;
  };

  return (
    <div className="flex h-full flex-col">
      {filter.bar}
      {status.operation?.kind === "bisect" ? <BisectBar refresh={refresh} /> : status.operation && <OperationBanner status={full} refresh={refresh} />}
      {total.length > 0 && <ReviewSummary files={total.length} add={add} del={del} reviewed={reviewed} />}
      <div
        onKeyDown={onListKey}
        // React focus events bubble out of portals too, so a row's open context menu still counts as the list.
        onFocus={() => setListFocused(true)}
        onBlur={(e) => setListFocused(e.currentTarget.contains(e.relatedTarget))}
        // The empty space below the rows lets go of the selection, like Finder.
        onClick={(e) => e.target === e.currentTarget && setPicked(null)}
        className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto pb-2 outline-none">
        {!all.length && !status.unstaged.length && (filter.needle ? <div className="px-4 py-6 text-center text-[12px] text-subtle">No changed files match.</div> : <AllCaughtUp />)}
        {status.conflicted.length > 0 && (
          <Section
            title="Conflicts"
            count={status.conflicted.length}
            tone="text-conflict"
            pinned={!!pickedConflicts}
            action={pickedConflicts && <SectionBtn onClick={() => stage(pickedConflicts)}>Mark {files(pickedConflicts.length)} resolved</SectionBtn>}
          >
            {rowsOf("conflict", status.conflicted, (file) =>
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
                <SectionBtn onClick={() => act("Unstage failed", () => api.unstage(status.staged.map((f) => f.path)))}>{allOrShown("Unstage", status.staged.length)}</SectionBtn>
              )
            }
          >
            {rowsOf("staged", status.staged, (file) =>
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
                  <SectionBtn onClick={() => discard(status.unstaged.filter((f) => f.status !== "?"))}>
                    {filtering ? `Discard ${status.unstaged.filter((f) => f.status !== "?").length} shown…` : "Discard"}
                  </SectionBtn>
                  {viewedPaths.length > 0 && (
                    <SectionBtn onClick={() => act("Stage failed", () => api.stage(viewedPaths))}>
                      Stage {viewedPaths.length} viewed{filtering && " shown"}
                    </SectionBtn>
                  )}
                  <SectionBtn onClick={stageAll}>{allOrShown("Stage", stageable(status.unstaged).paths.length)}</SectionBtn>
                </>
              )
            }
          >
            {rowsOf("unstaged", status.unstaged, (file) =>
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
        {(stashes.length > 0 || total.length > 0) && (
          <Section title="Stashes" count={stashes.length} action={total.length > 0 && !status.operation && <SectionBtn onClick={() => setStashing([])}>Stash…</SectionBtn>}>
            <StashList stashes={stashes} activeKey={activeKey} onOpen={onOpen} onHover={onHover} refresh={refresh} />
          </Section>
        )}
        {submodules.length > 0 && (
          <Section title="Submodules" count={submodules.length} action={<SectionBtn onClick={() => void updateSubmodules(refresh)}>Update</SectionBtn>}>
            <SubmoduleList submodules={submodules} />
          </Section>
        )}
      </div>
      {stashing && <StashDialog status={full} paths={stashing} onClose={() => setStashing(null)} refresh={refresh} />}
      {status.operation ? (
        // Committing by hand mid-rebase would splice an extra commit into the history.
        <div className="shrink-0 border-t border-border bg-panel px-3 py-2.5 text-[11.5px] text-muted-foreground">
          A {status.operation.kind} is in progress. Resolve the conflicts, then use <span className="font-medium text-foreground">Continue</span> above.
        </div>
      ) : (
        <CommitBox status={full} shown={filtering ? status : null} head={head} main={main} refresh={refresh} />
      )}
    </div>
  );
}
