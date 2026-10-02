import { ask } from "@/lib/app/ask";
import { ArrowLeftToLine, ArrowRightToLine, Check, Minus, Plus, Undo2 } from "lucide-react";
import { Fragment, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useListFilter } from "@/components/ListFilter";
import { Windowed } from "@/components/Windowed";
import { api, type Commit, type FileChange, type RepoStatus } from "@/lib/api";
import { hasConflictMarkers } from "@/lib/git/conflicts";
import { ignorePattern } from "@/lib/git/gitignore";
import { focusPanel } from "@/lib/ui/panels";
import { rangeOf, toggled } from "@/lib/ui/pick";
import { lastInputWasKey } from "@/lib/ui/pointer";
import { isMenuKey, moveTarget, openRowMenu, pageOf } from "@/lib/ui/useListNav";
import { matchesCommand, useCommands } from "@/lib/commands/keybindings";
import { type Selection, selectionKey } from "@/lib/repo/selection";
import { type Leaving, rowInPlace, settleLeaving } from "@/lib/repo/leaving";
import { toast } from "@/lib/app/toast";
import { tracked, undoAction } from "@/lib/repo/undo";
import { NESTED_EXPLAINED, stageable } from "@/lib/git/worktrees";
import { StashDialog, StashList, useStashes } from "./StashList";
import { BisectBar } from "@/features/history/BisectBar";
import { SubmoduleList, updateSubmodules, useSubmodules } from "./SubmoduleList";
import { attempt, type Change, changeList, files, filtered, keptByRestore, leftOut, paths, sumLines } from "./changeList";
import { ChangeRowMenu, FolderRowMenu } from "./ChangeRowMenu";
import { OperationBanner } from "./OperationBanner";
import { AllCaughtUp, collapsedSections, FolderRow, NestedRow, ReviewSummary, Row, Section, SectionBtn } from "./ChangeRows";
import { changesView, closedFolders, type Mtimes, ordered, rankFor, toggleFolder } from "./changesView";
import { foldersOf, pathTree } from "@/lib/ui/pathTree";
import { CommitBox } from "./CommitBox";
import { RowAction } from "@/components/RowAction";
import { primaryKey } from "@/lib/platform";
import { useNotes } from "@/lib/review/noteStore";
import { ReviewNotes } from "@/features/review/ReviewNotes";

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
  /** False: unstaging the staged ones it unchecked failed. */
  setViewed: (s: Selection[], on: boolean) => Promise<boolean>;
  /** Shows the file in the explorer, opening the panel if it's hidden. */
  onRevealInExplorer: (path: string) => void;
  /** History, filtered to this file's commits. */
  onShowHistory: (path: string) => void;
  /** The files' modified times, while sorting by them. */
  mtimes: Mtimes | null;
}

// Row and NestedRow are h-[26px].
const ROW_HEIGHT = 26;

const SECTION_OF: Record<Change["kind"], string> = { conflict: "Conflicts", staged: "Staged", unstaged: "Changes" };

/** A row of a list: a file, or in the tree view a folder. */
type Item =
  | { type: "file"; kind: Change["kind"]; file: FileChange; depth?: number; name?: string }
  | { type: "folder"; kind: Change["kind"]; key: string; path: string; label: string; depth: number; open: boolean; files: FileChange[] };

const folderKey = (kind: Change["kind"], path: string) => `${kind}:${path}`;

export function ChangesPanel({ status: full, head, main, activeKey, onOpen, onHover, refresh, viewed, setViewed, onRevealInExplorer, onShowHistory, mtimes }: Props) {
  // The list and its section actions (Stage all, Discard) cover the files the filter leaves, and
  // say so ("Stage 3 shown"); the commit takes hidden ones too and says how many.
  const filter = useListFilter("git", "Filter changed files");
  const filtering = !!filter.needle;
  const view = changesView.use();
  const closed = closedFolders.use();
  // Each list in the order shown (by name or newest first, as a tree or not), which ranges and ↑/↓ follow.
  const status = useMemo(
    () => ordered(filtering ? filtered(full, (f) => filter.matches(f.path, f.oldPath)) : full, view, mtimes),
    // filter.matches follows the needle.
    [full, filter.needle, view, mtimes],
  );
  const allOrShown = (verb: string, n: number) => (filtering ? `${verb} ${n} shown` : `${verb} all`);
  const act = async (title: string, fn: () => Promise<unknown>) => {
    const done = await attempt(title, fn);
    await refresh();
    return done;
  };

  // The open row staged, unstaged or discarded here hands the tab and the selection to the row now in
  // its place at once, without waiting for git (as in Fork), so S, S, S works down the list. Not
  // conflicts: a resolved one's tab follows it into Staged.
  const leaving = useRef(new Set<Leaving<RepoStatus>>());
  const leavingKeys = () => new Set([...leaving.current].flatMap((l) => [...l.keys]));
  // What the last render showed, for an action that starts after a dialog or settles after it.
  const shownNow = useRef({ activeKey, status: full, rows: [] as Change[] });
  const leave = async (rows: Change[], run: () => unknown) => {
    const keys = new Set(rows.filter((r) => r.kind !== "conflict").map(selectionKey));
    if (!keys.size) return run();
    const record: Leaving<RepoStatus> = { keys, follow: false, seen: null };
    const { activeKey: open, rows: now } = shownNow.current;
    const from = open !== null && keys.has(open) ? now.find((c) => selectionKey(c) === open) : undefined;
    let to: Change | undefined;
    if (from) {
      const out = new Set([...leavingKeys(), ...keys]);
      const key = rowInPlace(now.filter((c) => c.kind === from.kind).map(selectionKey), { has: (k) => !out.has(k) }, selectionKey(from));
      to = now.find((c) => selectionKey(c) === key);
      if (to) {
        setPicked(null);
        onOpen(to);
      } else record.follow = true;
    }
    leaving.current.add(record);
    if ((await run()) !== false) {
      record.seen = shownNow.current.status;
      return;
    }
    leaving.current.delete(record);
    // The file is still there: back to it, unless another row was opened meanwhile.
    if (from && to && shownNow.current.activeKey === selectionKey(to)) onOpen(from);
  };
  // Not rows whose own action is still running: S again before git was done ran it twice.
  const idle = (rows: Change[]) => {
    const out = leavingKeys();
    return rows.filter((r) => !out.has(selectionKey(r)));
  };

  const stageAll = () => {
    const { paths, skipped } = stageable(status.unstaged);
    if (skipped) toast("info", leftOut(skipped), NESTED_EXPLAINED);
    if (paths.length) act("Stage failed", () => api.stage(paths));
  };

  const stage = async (rows: Change[]) => {
    const go = idle(rows);
    if (go.length) await leave(go, () => act("Stage failed", () => api.stage(paths(go))));
  };
  const unstage = async (rows: Change[]) => {
    const go = idle(rows);
    if (go.length) await leave(go, () => act("Unstage failed", () => api.unstage(go.map((r) => r.file))));
  };
  // Unchecking a staged row unstages it (useViewed), so the tab moves on as Unstage's does.
  const markViewed = (rows: Selection[], on: boolean) => {
    const staged = rows.filter((r): r is Change => r.kind === "staged");
    if (on || !staged.length) return setViewed(rows, on);
    const go = idle(staged);
    return go.length ? leave(go, () => setViewed(go, false)) : undefined;
  };
  // Both sides edited (UU) or added (AA) the file, so git wrote markers into it; staging them
  // as they are would commit them.
  const markResolved = async (rows: Change[]) => {
    const check = async ({ file }: Change): Promise<"markers" | "unknown" | null> => {
      if (file.conflict !== "UU" && file.conflict !== "AA") return null;
      const now = await api.readFile(file.path).catch(() => null);
      if (!now) return "unknown";
      // Deleted, or binary: git writes no markers into those.
      if (!now.exists || now.binary) return null;
      if (now.tooLarge || now.lfsMissing) return "unknown";
      return hasConflictMarkers(now.text) ? "markers" : null;
    };
    // One at a time: each read can be megabytes.
    const found = { markers: [] as string[], unknown: [] as string[] };
    for (const r of rows) {
      const kind = await check(r);
      if (kind) found[kind].push(r.file.path);
    }
    const { markers, unknown } = found;
    const flagged = markers.length + unknown.length;
    if (flagged) {
      const name = (list: string[]) => (list.length === 1 ? list[0] : files(list.length));
      const said = [
        markers.length && `${name(markers)} still ${markers.length === 1 ? "has" : "have"} conflict markers.`,
        unknown.length && `${name(unknown)} couldn't be read to check for ${markers.length ? "them" : "conflict markers"}.`,
      ];
      const ok = await ask(`${said.filter(Boolean).join(" ")} Mark ${flagged === 1 ? "it" : "them"} resolved anyway?`, { title: "Mark resolved", kind: "warning", okLabel: "Mark Resolved" });
      if (!ok) return;
    }
    await stage(rows);
  };
  const resolve = (rows: Change[], side: "ours" | "theirs") =>
    act("Resolve failed", async () => {
      for (const r of rows) await api.resolveSide(r.file.path, side);
    });

  const discardable = status.unstaged.filter((f) => f.status !== "?" && !keptByRestore(f));

  // Nested repos can't be marked viewed, so every path here is stageable.
  const viewedPaths = status.unstaged.filter((file) => !file.nested && viewed({ kind: "unstaged", file })).map((f) => f.path);

  // Untracked files have nothing to restore; like VS Code, discarding one deletes it (to the Trash here).
  // Tracked ones keep a copy of what they were in the Trash, which Undo (and ⌘Z) writes back.
  const discard = async (picked: FileChange[]) => {
    // Submodules: nothing to discard there, or to keep in the Trash.
    const submodules = picked.filter(keptByRestore);
    if (submodules.length) toast("info", `Left out ${submodules.length === 1 ? submodules[0].path : `${submodules.length} submodules`}`, "Discard in the submodule itself: git restore leaves its commit and files as they are.");
    const list = picked.filter((f) => !keptByRestore(f));
    const restorable = list.filter((f) => f.status !== "?");
    const untracked = list.filter((f) => f.status === "?");
    if (!list.length) return;
    const one = list.length === 1 ? list[0].path : null;
    const trashOnly = !restorable.length;
    // A deleted file or submodule comes back, with no current version to keep.
    const copied = restorable.filter((f) => f.status !== "D").length;
    const message = trashOnly
      ? one
        ? `Move ${one} to the Trash? It is untracked, so git has no copy of it.`
        : `Move ${untracked.length} untracked files to the Trash? Git has no copy of them.`
      : `Discard changes to ${one ?? files(restorable.length)}?${copied ? ` ${restorable.length === 1 ? "Its current version is" : "Their current versions are"} moved to the Trash.` : ""}${untracked.length ? ` ${files(untracked.length)} git doesn't track will be moved to the Trash too.` : ""}`;
    const ok = await ask(message, trashOnly ? { title: one ? "Delete file" : "Delete files", kind: "warning", okLabel: "Move to Trash" } : { title: "Discard changes", kind: "warning", okLabel: "Discard" });
    if (!ok) return;
    let entry: number | null = null;
    await leave(
      list.map((file): Change => ({ kind: "unstaged", file })),
      async () => {
        const done = await attempt(trashOnly ? "Could not move to Trash" : "Discard failed", async () => {
          if (restorable.length) [, entry] = await tracked(() => api.discard(restorable.map((f) => f.path)));
          for (const f of untracked) await api.trashPath(f.path);
        });
        if (done && restorable.length) {
          const single = restorable.length === 1;
          toast("success", `Discarded ${single ? restorable[0].path : files(restorable.length)}`, copied ? `The old ${copied === 1 ? "version is" : "versions are"} in the Trash.` : undefined, undoAction(entry, refresh));
        }
        await refresh();
        return done;
      },
    );
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

  // The rows each list shows: its files, or in the tree view its folders and the files of open ones.
  const itemsOf = useMemo(() => {
    const rank = rankFor(view, mtimes);
    const of = (kind: Change["kind"], list: FileChange[]): Item[] =>
      !view.tree
        ? list.map((file) => ({ type: "file", kind, file }))
        : pathTree(list, (f) => f.path, { rank, closed: (p) => closed.has(folderKey(kind, p)) }).map((r) =>
            r.kind === "leaf"
              ? { type: "file", kind, file: r.item, depth: r.depth, name: r.name }
              : { type: "folder", kind, key: `folder:${folderKey(kind, r.path)}`, path: r.path, label: r.label, depth: r.depth, open: r.open, files: r.items },
          );
    return { conflict: of("conflict", status.conflicted), staged: of("staged", status.staged), unstaged: of("unstaged", status.unstaged) };
  }, [status, view, mtimes, closed]);
  const keyOf = (it: Item) => (it.type === "folder" ? it.key : selectionKey(it));
  // What ↑/↓ go along: the rows shown, but not nested repositories, which can't be picked.
  const nav = (["conflict", "staged", "unstaged"] as const).flatMap((kind) => itemsOf[kind].filter((it) => it.type === "folder" || !it.file.nested));
  const navKeys = nav.map(keyOf);
  const navIndex = new Map(navKeys.map((k, i) => [k, i]));
  const shownFiles = navKeys.flatMap((k) => (index.has(k) ? [all[index.get(k)!]] : []));
  const folderRows = (it: Item & { type: "folder" }): Change[] => it.files.filter((f) => !f.nested).map((file) => ({ kind: it.kind, file }));

  // Opening a file in a closed folder (J/K, a tab) opens the folders around it.
  useEffect(() => {
    if (!view.tree || !active) return;
    for (const p of foldersOf(active.file.path)) toggleFolder(folderKey(active.kind, p), true);
    // Not on `active`, a new object each status: a folder closed around the open file stays closed.
  }, [activeKey, view.tree]);

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

  const range = (from: Change, to: Change) => rangeOf(shownFiles, from, to, selectionKey);

  // ⌘-click (Ctrl off macOS) toggles a row, ⇧-click picks the range from the anchor. The open tab follows the clicked row either way.
  const pick = (c: Change, e: { metaKey: boolean; ctrlKey: boolean; shiftKey: boolean }) => {
    if (primaryKey(e)) setPicked({ rows: toggled([...selected.values()], c, selectionKey), anchor: c, focus: c.file.path });
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
    // The tab moves on to the next file (see `leave`). Conflicts are left to their own actions.
    "git.toggleStage": active?.kind === "unstaged" ? () => stage(targets(active)) : active?.kind === "staged" ? () => unstage(targets(active)) : undefined,
    "git.discard": active?.kind === "unstaged" && !targets(active).every((r) => keptByRestore(r.file)) ? () => discard(targets(active).map((r) => r.file)) : undefined,
    // Only while several rows are selected: registering then puts it over Workspace's V, which marks
    // just the open file. Staged rows stay out, as there: unmarking one unstages it.
    "review.toggleViewed": active && active.kind !== "staged" && targets(active).length > 1 ? () => setViewed(targets(active), !viewed(active)) : undefined,
  });

  // Once the rows have left (by git's lists, not what the filter shows), an open one that had no row
  // of its list to move to goes to the other list's first. A record that settled without them leaving
  // (the agent rewrote the file) ends with the next status. A row that left holding focus passes it to
  // the row now in its place: always along with the tab, so ↑/↓ go on from there; otherwise only the
  // keyboard's, as a click on Stage focuses its row (WebKit) and the neighbour kept the highlight. Rows
  // git moved elsewhere (the terminal, ⌘Z) keep their tab, which follows the file.
  const list = useRef<HTMLDivElement>(null);
  const lostFocus = useRef<string | null>(null);
  const shown = useRef(all);
  // A row to focus once it renders: a long list renders only what's near the screen.
  const [reach, setReach] = useState<string | null>(null);
  const rowEl = (key: string) => list.current?.querySelector<HTMLElement>(`[data-row="${CSS.escape(key)}"]`);
  const focusKey = (key: string) => {
    const el = rowEl(key);
    if (!el) return setReach(key);
    el.focus();
    el.scrollIntoView({ block: "nearest" });
  };
  useLayoutEffect(() => {
    // In a closed section: the list's first row, so ↑/↓ still work. Nothing at all: the panel, not the page.
    const focusRow = (to: Change | undefined) => {
      const el = to && rowEl(selectionKey(to));
      if (el) return el.focus();
      if (to && !collapsedSections.get().includes(SECTION_OF[to.kind])) return setReach(selectionKey(to));
      const first = [...(list.current?.querySelectorAll<HTMLElement>("[data-row]") ?? [])].find((r) => index.has(r.dataset.row ?? ""));
      if (first) first.focus();
      else focusPanel("git");
    };
    if (reach !== null) {
      setReach(null);
      const el = rowEl(reach);
      if (el) {
        el.focus();
        el.scrollIntoView({ block: "nearest" });
      } else focusRow(undefined);
    }

    const before = shown.current;
    const lost = lostFocus.current;
    shown.current = all;
    lostFocus.current = null;
    shownNow.current = { activeKey, status: full, rows: all };
    const follow = settleLeaving(leaving.current, new Set(total.map(selectionKey)), full, activeKey);
    const from = follow ? activeKey : lost !== null && !index.has(lost) ? lost : null;
    const was = from === null ? undefined : before.find((c) => selectionKey(c) === from);
    if (!was || from === null) return;
    const key = rowInPlace(before.filter((c) => c.kind === was.kind).map(selectionKey), index, from);
    const other = was.kind === "staged" ? "unstaged" : "staged";
    const to = key === undefined ? (all.find((c) => c.kind === other) ?? all[0]) : all[index.get(key) ?? -1];
    if (follow) {
      setPicked(null);
      if (to) onOpen(to);
    }
    if (lost !== null && (follow || lastInputWasKey())) focusRow(to);
  });

  // One tab stop for the whole list (the active row), so Tab reaches its actions, not every row.
  const tabStop = activeKey !== null && navIndex.has(activeKey) ? activeKey : (navKeys[0] ?? null);

  // ↑/↓ (Home/End, PageUp/PageDown) from a focused row (clicking one focuses it), ⇧ to extend the
  // selection, ⌘A (git.selectAllChanges) for all of it, Esc to let it go; ↵ keeps the preview tab, Space opens it like a
  // click, → goes to its code, ⇧F10 opens its menu. The other lists share the moves through useListNav.
  const onListKey = (e: React.KeyboardEvent) => {
    const key = e.target instanceof HTMLElement ? e.target.dataset.row : undefined;
    const i = key === undefined ? -1 : (navIndex.get(key) ?? -1);
    if (key === undefined || i < 0) return;
    const item = nav[i];
    // Undefined on a folder.
    const cur: Change | undefined = all[index.get(key) ?? -1];
    if (matchesCommand("git.selectAllChanges", e.nativeEvent)) {
      e.preventDefault();
      const first = cur ?? active ?? all[0];
      if (!first) return;
      setPicked({ rows: all, anchor: anchor ?? first, focus: (active ?? first).file.path });
      if (!active) onOpen(first);
      return;
    }
    if (e.altKey || e.ctrlKey) return;
    const move = e.metaKey ? null : moveTarget(e.key, i, nav.length, pageOf(e.target as HTMLElement));
    // A folder's row: the nearest one above that is less deep.
    const parent = (): Item | undefined =>
      item.depth
        ? nav
            .slice(0, i)
            .reverse()
            .find((it) => it.type === "folder" && it.depth < item.depth!)
        : undefined;
    if (isMenuKey(e)) openRowMenu(e.target as HTMLElement);
    else if (e.key === "Escape") {
      // Only when there's a selection to drop; otherwise Esc isn't ours to take.
      if (!picked || stale) return;
      setPicked(null);
    } else if (e.metaKey) return;
    else if (move !== null) {
      // A focused row that isn't open yet (Tab into a list with nothing open) opens first.
      const toKey = cur && key !== activeKey ? key : navKeys[move];
      const to: Change | undefined = all[index.get(toKey) ?? -1];
      // A folder takes focus only; ⇧ keeps the selection as it is.
      if (!to) focusKey(toKey);
      else {
        const from = anchor ?? cur ?? to;
        if (e.shiftKey) setPicked({ rows: range(from, to), anchor: from, focus: to.file.path });
        else setPicked(null);
        onOpen(to);
      }
    } else if (e.shiftKey) return;
    else if (item.type === "folder") {
      const closedKey = folderKey(item.kind, item.path);
      if (e.key === "Enter" || e.key === " ") toggleFolder(closedKey);
      else if (e.key === "ArrowRight") {
        if (!item.open) toggleFolder(closedKey, true);
        else if (navKeys[i + 1]) focusKey(navKeys[i + 1]);
      } else if (e.key === "ArrowLeft") {
        const up = parent();
        if (item.open) toggleFolder(closedKey, false);
        else if (up) focusKey(keyOf(up));
      } else return;
    } else if (!cur) return;
    else if (e.key === "ArrowRight") {
      if (key !== activeKey) onOpen(cur);
      focusPanel("code");
    } else if (e.key === "ArrowLeft") {
      const up = parent();
      if (!up) return;
      focusKey(keyOf(up));
    } else if (e.key === "Enter") onOpen(cur, true);
    else if (e.key === " ") pick(cur, e);
    else return;
    e.preventDefault();
  };

  // A row's buttons, for its own file or a folder's (`rows`, every file under it).
  const actionsFor = (kind: Change["kind"], rows: Change[], file?: FileChange, stageRows = stage) => {
    const many = rows.length > 1 || !file;
    if (kind === "conflict")
      return (
        <>
          <RowAction label={many ? `Mark ${files(rows.length)} resolved as they are` : "Mark resolved as it is"} onClick={() => markResolved(rows)}>
            <Check />
          </RowAction>
          <RowAction label={many ? `Take current version of ${files(rows.length)}` : "Take current version"} onClick={() => resolve(rows, "ours")}>
            <ArrowLeftToLine />
          </RowAction>
          <RowAction label={many ? `Take incoming version of ${files(rows.length)}` : "Take incoming version"} onClick={() => resolve(rows, "theirs")}>
            <ArrowRightToLine />
          </RowAction>
        </>
      );
    if (kind === "staged")
      return (
        <RowAction label={many ? `Unstage ${files(rows.length)}` : "Unstage"} onClick={() => unstage(rows)}>
          <Minus />
        </RowAction>
      );
    // A file's own button leaves untracked files out (deleting is in its menu); a folder's covers them.
    const discardable = file ? file.status !== "?" && !keptByRestore(file) : rows.some((r) => !keptByRestore(r.file));
    return (
      <>
        {discardable && (
          <RowAction label={many ? `Discard ${files(rows.length)}…` : "Discard changes"} onClick={() => discard(rows.map((r) => r.file))}>
            <Undo2 />
          </RowAction>
        )}
        <RowAction label={many ? `Stage ${files(rows.length)}` : "Stage"} onClick={() => stageRows(rows)}>
          <Plus />
        </RowAction>
      </>
    );
  };

  const row = (it: Item & { type: "file" }) => {
    const sel: Change = { kind: it.kind, file: it.file };
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
        onToggleViewed={() => markViewed(rows, !isViewed)}
        lostFocus={lostFocus}
        depth={it.depth}
        label={it.name}
        menu={() => (
          <ChangeRowMenu
            sel={sel}
            rows={rows}
            root={status.root}
            canStash={!status.operation}
            viewed={viewed}
            setViewed={markViewed}
            onOpen={onOpen}
            onShowHistory={onShowHistory}
            onRevealInExplorer={onRevealInExplorer}
            stage={stage}
            unstage={unstage}
            markResolved={markResolved}
            discard={discard}
            ignore={ignore}
            resolve={resolve}
            stash={setStashing}
          />
        )}
      >
        {actionsFor(sel.kind, rows, sel.file)}
      </Row>
    );
  };

  const folderRow = (it: Item & { type: "folder" }) => {
    const rows = folderRows(it);
    // Nested repositories in it are left out of its actions, which say so where it matters.
    const nested = it.files.length - rows.length;
    const stageFolder = (r: Change[]) => {
      if (nested) toast("info", leftOut(nested), NESTED_EXPLAINED);
      return stage(r);
    };
    return (
      <FolderRow
        key={it.key}
        rowKey={it.key}
        path={it.path}
        label={it.label}
        depth={it.depth}
        open={it.open}
        count={it.files.length}
        tabStop={tabStop === it.key}
        onToggle={() => toggleFolder(folderKey(it.kind, it.path))}
        // Its files left (staged, say): focus goes on as from the first of them.
        lostFocus={{ ref: lostFocus, as: rows[0] ? selectionKey(rows[0]) : it.key }}
        menu={() => (
          <FolderRowMenu
            kind={it.kind}
            path={it.path}
            rows={rows}
            root={status.root}
            canStash={!status.operation}
            viewed={viewed}
            setViewed={markViewed}
            onRevealInExplorer={onRevealInExplorer}
            stage={stageFolder}
            unstage={unstage}
            markResolved={markResolved}
            discard={discard}
            resolve={resolve}
            stash={setStashing}
          />
        )}
      >
        {rows.length > 0 && actionsFor(it.kind, rows, undefined, stageFolder)}
      </FolderRow>
    );
  };

  /** A section's rows; with thousands, only those near the screen (and the open, tab-stop and `reach` rows). */
  const rowsOf = (kind: Change["kind"], title: string) => {
    const items = itemsOf[kind];
    const keep = [activeKey, tabStop, reach].map((k) => items.findIndex((it) => keyOf(it) === k));
    const render = (it: Item) => (it.type === "folder" ? folderRow(it) : it.file.nested ? <NestedRow key={it.file.path} file={it.file} depth={it.depth} label={it.name} /> : row(it));
    return (
      <div role="tree" aria-label={title} aria-multiselectable>
        <Windowed count={items.length} height={ROW_HEIGHT} keep={keep} render={(i) => render(items[i])} />
      </div>
    );
  };

  const conflicts = status.conflicted.length > 0 && (
    <Section
      title="Conflicts"
      count={status.conflicted.length}
      tone="text-conflict"
      pinned={!!pickedConflicts}
      action={pickedConflicts && <SectionBtn onClick={() => markResolved(pickedConflicts)}>Mark {files(pickedConflicts.length)} resolved</SectionBtn>}
    >
      {rowsOf("conflict", "Conflicts")}
    </Section>
  );
  const staged = status.staged.length > 0 && (
    <Section
      title="Staged"
      count={status.staged.length}
      pinned={!!pickedStaged}
      action={
        pickedStaged ? (
          <SectionBtn onClick={() => unstage(pickedStaged)}>Unstage {files(pickedStaged.length)}</SectionBtn>
        ) : (
          <>
            <SectionBtn onClick={() => onOpen({ kind: "changes", list: "staged" }, true)}>Open All</SectionBtn>
            <SectionBtn onClick={() => act("Unstage failed", () => api.unstage(status.staged))}>{allOrShown("Unstage", status.staged.length)}</SectionBtn>
          </>
        )
      }
    >
      {rowsOf("staged", "Staged")}
    </Section>
  );
  const changes = status.unstaged.length > 0 && (
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
            <SectionBtn onClick={() => onOpen({ kind: "changes", list: "unstaged" }, true)}>Open All</SectionBtn>
            {/* Leaves untracked files alone; deleting one is a per-file choice. */}
            <SectionBtn onClick={() => discard(discardable)}>{filtering ? `Discard ${discardable.length} shown…` : "Discard"}</SectionBtn>
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
      {rowsOf("unstaged", "Changes")}
    </Section>
  );
  const reviewNotes = useNotes().length > 0 && <ReviewNotes changes={all} onOpen={onOpen} />;
  const stashList = (stashes.length > 0 || total.length > 0) && (
    <Section title="Stashes" count={stashes.length} action={total.length > 0 && !status.operation && <SectionBtn onClick={() => setStashing([])}>Stash…</SectionBtn>}>
      <StashList stashes={stashes} activeKey={activeKey} onOpen={onOpen} onHover={onHover} refresh={refresh} />
    </Section>
  );
  const submoduleList = submodules.length > 0 && (
    <Section title="Submodules" count={submodules.length} action={<SectionBtn onClick={() => void updateSubmodules(refresh)}>Update</SectionBtn>}>
      <SubmoduleList submodules={submodules} />
    </Section>
  );
  // In the list or, closed, below it: RepoPanes' way, so a closed section stays out of the way.
  const sections = ([
    ["Conflicts", conflicts],
    ["Staged", staged],
    ["Changes", changes],
    ["Review Notes", reviewNotes],
    ["Stashes", stashList],
    ["Submodules", submoduleList],
  ] as const).filter(([, s]) => s);
  const collapsed = collapsedSections.use();

  return (
    <div className="flex h-full flex-col">
      {filter.bar}
      {status.operation?.kind === "bisect" ? <BisectBar refresh={refresh} /> : status.operation && <OperationBanner status={full} refresh={refresh} />}
      {total.length > 0 && <ReviewSummary files={total.length} add={add} del={del} reviewed={reviewed} />}
      <div
        ref={list}
        onKeyDown={onListKey}
        // React focus events bubble out of portals too, so a row's open context menu still counts as the list.
        onFocus={() => setListFocused(true)}
        onBlur={(e) => setListFocused(e.currentTarget.contains(e.relatedTarget))}
        // The empty space below the rows lets go of the selection, like Finder.
        onClick={(e) => e.target === e.currentTarget && setPicked(null)}
        className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto pb-2 outline-none">
        {!all.length && !status.unstaged.length && (filter.needle ? <div className="px-4 py-6 text-center text-[12px] text-subtle">No changed files match.</div> : <AllCaughtUp />)}
        {sections.filter(([t]) => !collapsed.includes(t)).map(([t, s]) => <Fragment key={t}>{s}</Fragment>)}
      </div>
      {sections.some(([t]) => collapsed.includes(t)) && (
        // The commit box's top border is the last header's bottom one.
        <div className="shrink-0 border-t border-border [&>:last-child>:first-child]:border-b-0">
          {sections.filter(([t]) => collapsed.includes(t)).map(([t, s]) => <Fragment key={t}>{s}</Fragment>)}
        </div>
      )}
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
