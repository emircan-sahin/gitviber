import { ask } from "@tauri-apps/plugin-dialog";
import { ChevronRight, Copy, File, FilePlus, FolderPlus, FolderSearch, History, Pencil, Trash2, Undo2 } from "lucide-react";
import { Fragment, useCallback, useEffect, useImperativeHandle, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useListFilter } from "@/components/ListFilter";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuShortcut, ContextMenuTrigger } from "@/components/ui/context-menu";
import { api, type ChangeStatus, type Entry, errorMessage, type RepoStatus } from "@/lib/api";
import { IS_MAC, primaryKey, REVEAL_LABEL } from "@/lib/platform";
import { matchesCommand, useShortcut } from "@/lib/commands/keybindings";
import { focusPanel } from "@/lib/ui/panels";
import { isMenuKey, moveTarget, openRowMenu, pageOf } from "@/lib/ui/useListNav";
import { type Selection, selectionKey } from "@/lib/repo/selection";
import { toast } from "@/lib/app/toast";
import { tracked, undoAction } from "@/lib/repo/undo";
import { cn } from "@/lib/utils";
import { copyFiles, copyLabel, copyText } from "@/lib/app/clipboard";
import { revealPath } from "@/lib/app/openIn";
import { basename, childPath, dirname } from "@/lib/path";
import { FileIcon, FolderIcon } from "@/components/FileIcon";
import { OpenInMenuItem } from "@/features/workspace/OpenIn";
import { statusInfo } from "@/components/StatusBadge";

export interface FileTreeHandle {
  collapseAll: () => void;
  /** Opens the folders down to `path`, selects it and focuses the tree. */
  reveal: (path: string) => void;
  /** Opens the filter, as Find does with focus in the tree. */
  filter: () => void;
}

interface Props {
  status: RepoStatus | null;
  revision: number;
  activeKey: string | null;
  onOpen: (s: Selection, pin?: boolean) => void;
  onHover: (s: Selection) => void;
  /** An entry was renamed (`to`) or trashed (`to` = null), so open tabs can follow it. */
  onPathMoved: (from: string, to: string | null) => void;
  /** History, filtered to a file's commits (followed through renames) or a folder's. */
  onShowHistory: (path: string, file: boolean) => void;
  ref?: React.Ref<FileTreeHandle>;
}

type Editing = { mode: "rename"; entry: Entry } | { mode: "new"; parent: string; isDir: boolean };

const INDENT = 12;

const isInside = (path: string, dir: string) => path === dir || path.startsWith(`${dir}/`);

/** `list` without the entries inside a folder also in it: trashing the folder takes them along. */
const topmost = (list: Entry[]) => list.filter((e) => !list.some((d) => d !== e && d.isDir && isInside(e.path, d.path)));

/** The filter lists this many files at most: the tree renders every row it has. */
const MAX_MATCHES = 1000;

/** The files that match and the folders down to them, all open, in the order list_dir sorts a folder. */
function matchingTree(files: string[], matches: (path: string) => boolean) {
  const children: Record<string, Entry[]> = {};
  const add = (path: string, isDir: boolean) => (children[dirname(path)] ??= []).push({ name: basename(path), path, isDir, ignored: false });
  const expanded = new Set([""]);
  let found = 0;
  for (const path of files) {
    if (!matches(path)) continue;
    if (++found > MAX_MATCHES) break;
    add(path, false);
    for (let dir = dirname(path); dir && !expanded.has(dir); dir = dirname(dir)) {
      expanded.add(dir);
      add(dir, true);
    }
  }
  const key = (e: Entry) => e.name.toLowerCase();
  for (const list of Object.values(children)) list.sort((a, b) => Number(b.isDir) - Number(a.isDir) || (key(a) < key(b) ? -1 : key(a) > key(b) ? 1 : 0));
  return { children, expanded, found: Math.min(found, MAX_MATCHES), capped: found > MAX_MATCHES };
}

/** Lazy tree of the working directory, like VS Code's explorer, annotated with git status. */
export function FileTree({ status, revision, activeKey, onOpen, onHover, onPathMoved, onShowHistory, ref }: Props) {
  const [children, setChildren] = useState<Record<string, Entry[]>>({});
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set([""]));
  // Keyboard cursor, separate from the open tab (activeKey) like VS Code's focused item.
  const [selected, setSelected] = useState<string | null>(null);
  // Rows picked with ⌘/⇧ (clicks or ⇧-arrows) from `anchor`, as in the Changes panel; null: just `selected`.
  const [picked, setPicked] = useState<{ paths: Set<string>; anchor: string } | null>(null);
  const [editing, setEditing] = useState<Editing | null>(null);
  const renameKey = useShortcut("explorer.rename");
  const deleteKey = useShortcut("explorer.delete");
  // Right-clicked entry; null = the empty area below the tree (acts on the repo root).
  const [menuTarget, setMenuTarget] = useState<Entry | null>(null);
  const treeRef = useRef<HTMLDivElement>(null);
  // Set when a menu item starts inline editing, so the closing menu doesn't steal the input's focus.
  const keepFocus = useRef(false);
  // A revealed path's folders may still be loading; scroll to it once its row exists.
  const revealing = useRef<string | null>(null);

  // Filtering lists every file git does (not the ignored ones), open folders or not.
  const filter = useListFilter("explorer", "Filter files");
  const filtering = !!filter.needle;
  const [files, setFiles] = useState<string[] | null>(null);
  useEffect(() => {
    if (!filtering) return;
    let live = true;
    api.listFiles().then(
      (list) => live && setFiles(list),
      (e) => live && toast("error", "Could not list files", errorMessage(e)),
    );
    return () => {
      live = false;
    };
  }, [filtering, revision]);
  const matching = useMemo(() => (filtering && files ? matchingTree(files, (path) => filter.matches(path)) : null), [filtering, files, filter.needle]);
  // What the rows show: the matches while filtering (the last tree until they're listed), else the folders opened.
  const shown = matching ?? { children, expanded };

  // Per-path request counter: a slow, older listing must not overwrite a newer one.
  const requests = useRef(new Map<string, number>());
  const loadDir = useCallback(async (path: string) => {
    const id = (requests.current.get(path) ?? 0) + 1;
    requests.current.set(path, id);
    try {
      const entries = await api.listDir(path);
      if (requests.current.get(path) === id) setChildren((c) => ({ ...c, [path]: entries }));
    } catch (e) {
      if (requests.current.get(path) !== id) return;
      // The root must stay expanded, or the tree would stay empty after the error clears.
      if (path === "") return toast("error", "Could not list repository", errorMessage(e));
      // Folder deleted (agents do that): close it quietly instead of erroring on every refresh.
      setExpanded((x) => {
        const next = new Set(x);
        next.delete(path);
        return next;
      });
      setChildren(({ [path]: _gone, ...rest }) => rest);
    }
  }, []);

  // Re-list open folders whenever the disk changes so new/deleted files show up.
  useEffect(() => {
    expanded.forEach((p) => loadDir(p));
  }, [revision, loadDir]);

  /** Opens the folders down to `path` and scrolls to it once its row is there. */
  const openTo = (path: string) => {
    const parts = path.split("/");
    const dirs = parts.slice(0, -1).map((_, i) => parts.slice(0, i + 1).join("/"));
    setExpanded((x) => new Set([...x, ...dirs]));
    dirs.forEach((d) => loadDir(d));
    revealing.current = path;
  };

  useImperativeHandle(ref, () => ({
    collapseAll: () => {
      filter.close();
      setExpanded(new Set([""]));
      setPicked(null);
      setSelected((s) => s && s.split("/")[0]);
    },
    reveal: (path) => {
      filter.close();
      openTo(path);
      setSelected(path);
      setPicked(null);
      treeRef.current?.focus();
    },
    filter: filter.open,
  }));

  // Leaving the filter, the file picked in it stays picked, shown in its folders.
  const wasFiltering = useRef(false);
  useEffect(() => {
    // Rows picked on one side of the filter aren't what the other side shows.
    setPicked(null);
    if (wasFiltering.current && !filtering && selected) openTo(selected);
    wasFiltering.current = filtering;
    // Only on the way out.
  }, [filtering]);

  const { fileStatus, dirtyDirs, discardable } = useMemo(() => {
    const fileStatus = new Map<string, ChangeStatus>();
    for (const f of [...(status?.staged ?? []), ...(status?.unstaged ?? []), ...(status?.conflicted ?? [])]) {
      fileStatus.set(f.path, f.status);
    }
    const dirtyDirs = new Set<string>();
    for (const path of fileStatus.keys()) {
      const parts = path.split("/");
      for (let i = 1; i < parts.length; i++) dirtyDirs.add(parts.slice(0, i).join("/"));
    }
    // Same rule as the Changes panel: only tracked worktree edits can be restored.
    const discardable = new Set((status?.unstaged ?? []).filter((f) => f.status !== "?").map((f) => f.path));
    return { fileStatus, dirtyDirs, discardable };
  }, [status]);

  // What's on screen, top to bottom: arrow keys walk this list.
  const rows = useMemo(() => {
    const out: { entry: Entry; depth: number }[] = [];
    const walk = (dir: string, depth: number) =>
      shown.children[dir]?.forEach((entry) => {
        out.push({ entry, depth });
        if (entry.isDir && shown.expanded.has(entry.path)) walk(entry.path, depth + 1);
      });
    walk("", 0);
    return out;
  }, [shown.children, shown.expanded]);

  /** The paths from `from` to `to` in the order they show; just `to` if `from` is gone. */
  const range = (from: string, to: string) => {
    const [i, j] = [rows.findIndex((r) => r.entry.path === from), rows.findIndex((r) => r.entry.path === to)];
    return new Set(i < 0 || j < 0 ? [to] : rows.slice(Math.min(i, j), Math.max(i, j) + 1).map((r) => r.entry.path));
  };
  const shows = (path: string | null | undefined): path is string => !!path && rows.some((r) => r.entry.path === path);
  /** Where a ⇧-range starts: the last anchor while its row still shows (a collapsed folder or the filter can hide it). */
  const anchorFor = (fallback: string) => (shows(picked?.anchor) ? picked.anchor : fallback);

  // ⌘-click (Ctrl off macOS) toggles a row, ⇧-click picks the range from the anchor; neither opens it, as in VS Code.
  // The ⌘ check comes first, as in the Changes panel.
  const click = (e: Entry, ev: React.MouseEvent) => {
    setSelected(e.path);
    if (primaryKey(ev)) {
      const paths = new Set(picked?.paths ?? (selected ? [selected] : []));
      if (!paths.delete(e.path)) paths.add(e.path);
      setPicked({ paths, anchor: e.path });
    } else if (ev.shiftKey) {
      const anchor = anchorFor(shows(selected) ? selected : e.path);
      setPicked({ paths: range(anchor, e.path), anchor });
    } else {
      setPicked(null);
      activate(e);
    }
  };

  // The edited entry (or the folder a new one goes into) vanished on a refresh: drop the input,
  // or keyboard navigation stays disabled with nothing to type into.
  useEffect(() => {
    const path = editing && (editing.mode === "rename" ? editing.entry.path : editing.parent);
    if (!path || rows.some((r) => r.entry.path === path)) return;
    setEditing(null);
    if (document.activeElement === document.body) treeRef.current?.focus();
  }, [rows, editing]);

  const rowOf = (path: string) => treeRef.current?.querySelector(`[data-path="${CSS.escape(path)}"]`);

  useEffect(() => {
    if (selected) rowOf(selected)?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  useEffect(() => {
    const row = revealing.current && rowOf(revealing.current);
    if (!row) return;
    row.scrollIntoView({ block: "center" });
    revealing.current = null;
  }, [rows]);

  const setOpen = (path: string, open: boolean) => {
    setExpanded((x) => {
      const next = new Set(x);
      if (open) next.add(path);
      else next.delete(path);
      return next;
    });
    // Always re-list: the cached listing may be from before the agent changed it.
    if (open) loadDir(path);
  };

  // While filtering, folders stay open around their matches.
  const activate = (e: Entry, pin = false) => (!e.isDir ? onOpen({ kind: "file", path: e.path }, pin) : !matching && setOpen(e.path, !expanded.has(e.path)));

  const startEditing = (next: Editing) => {
    keepFocus.current = true;
    if (next.mode === "new" && !expanded.has(next.parent)) setOpen(next.parent, true);
    setEditing(next);
  };

  const finishEditing = async (name: string | null, refocus: boolean) => {
    const ed = editing;
    setEditing(null);
    if (refocus) treeRef.current?.focus();
    name = name?.trim() ?? "";
    if (!ed || !name || (ed.mode === "rename" && name === ed.entry.name)) return;
    if (name.includes("/") || name === "." || name === "..") return toast("error", "Invalid name", `"${name}" is not a valid file or folder name.`);
    const dir = ed.mode === "rename" ? dirname(ed.entry.path) : ed.parent;
    const path = childPath(dir, name);
    try {
      if (ed.mode === "rename") {
        await api.renamePath(ed.entry.path, path);
        // Keep the renamed folder (and anything open inside it) expanded under its new name.
        const from = ed.entry.path;
        const moved = [...expanded].filter((p) => isInside(p, from));
        setExpanded((x) => new Set([...x].map((p) => (isInside(p, from) ? path + p.slice(from.length) : p))));
        // Their listings are cached under the old paths; the watcher skips node_modules, so list now.
        moved.forEach((p) => loadDir(path + p.slice(from.length)));
        onPathMoved(from, path);
      } else {
        await (ed.isDir ? api.createDir(path) : api.createFile(path));
        if (!ed.isDir) onOpen({ kind: "file", path }, true);
      }
      setSelected(path);
      loadDir(dir);
    } catch (e) {
      toast("error", ed.mode === "rename" ? "Rename failed" : `Could not create ${ed.isDir ? "folder" : "file"}`, errorMessage(e));
    }
  };

  const remove = async (list: Entry[]) => {
    const top = topmost(list);
    const [one] = top;
    if (!one) return;
    const many = top.length > 1;
    const ok = await ask(
      many
        ? `Move ${top.length} items to the Trash?${top.some((e) => e.isDir) ? " Everything inside the folders goes too." : ""}`
        : `Move "${one.path}" to the Trash?${one.isDir ? " Everything inside it goes too." : ""}`,
      { title: many ? `Delete ${top.length} items` : one.isDir ? "Delete folder" : "Delete file", kind: "warning", okLabel: "Move to Trash" },
    );
    if (!ok) return;
    const gone: Entry[] = [];
    try {
      for (const e of top) {
        await api.trashPath(e.path);
        gone.push(e);
        onPathMoved(e.path, null);
      }
    } catch (err) {
      toast("error", "Could not move to Trash", errorMessage(err));
    }
    if (!gone.length) return;
    // Land on the next row outside the deleted entries, like VS Code.
    const kept = (r: { entry: Entry }) => !gone.some((e) => isInside(r.entry.path, e.path));
    const i = rows.findIndex((r) => !kept(r));
    const next = rows.slice(i + 1).find(kept) ?? rows.slice(0, i).reverse().find(kept);
    setSelected(next?.entry.path ?? null);
    setPicked(null);
    new Set(gone.map((e) => dirname(e.path))).forEach((d) => loadDir(d));
  };

  const discard = async (list: Entry[]) => {
    const what = list.length > 1 ? `${list.length} files` : list[0].path;
    const ok = await ask(`Discard changes to ${what}? ${list.length > 1 ? "Their current versions are" : "Its current version is"} moved to the Trash.`, {
      title: "Discard changes",
      kind: "warning",
      okLabel: "Discard",
    });
    if (!ok) return;
    try {
      const [, entry] = await tracked(() => api.discard(list.map((e) => e.path)));
      // The file watcher refreshes after an undo writes the file back.
      toast("success", `Discarded ${what}`, list.length > 1 ? "The old versions are in the Trash." : "The old version is in the Trash.", undoAction(entry, () => {}));
    } catch (err) {
      toast("error", "Discard failed", errorMessage(err));
    }
  };

  const onKeyDown = (ev: React.KeyboardEvent) => {
    if (ev.target !== ev.currentTarget || editing) return;
    if (isMenuKey(ev)) {
      // With nothing selected, the tree's own menu: New File / New Folder at the root.
      const row = selected ? rowOf(selected) : null;
      openRowMenu(row instanceof HTMLElement ? row : (ev.currentTarget as HTMLElement));
      ev.preventDefault();
      return;
    }
    // ⌥↑/⌥↓ (next/previous change) and ⌘-arrows belong to the global shortcuts.
    if (ev.key.startsWith("Arrow") && (ev.altKey || ev.metaKey)) return;
    const i = rows.findIndex((r) => r.entry.path === selected);
    const cur = rows[i]?.entry;
    // ⇧ extends the picked range from its anchor, as in the Changes panel.
    const move = (to: number) => {
      const row = rows[Math.max(0, Math.min(rows.length - 1, to))];
      if (!row) return;
      const anchor = anchorFor(cur?.path ?? row.entry.path);
      setPicked(ev.shiftKey ? { paths: range(anchor, row.entry.path), anchor } : null);
      setSelected(row.entry.path);
    };
    let handled = true;
    if (cur && matchesCommand("explorer.rename", ev.nativeEvent)) setEditing({ mode: "rename", entry: cur });
    else if (cur && matchesCommand("explorer.delete", ev.nativeEvent)) remove(picked?.paths.has(cur.path) ? pickedEntries : [cur]);
    else if (ev.key === "Escape" && picked) setPicked(null);
    else if (ev.key === "ArrowDown") move(i + 1);
    else if (ev.key === "ArrowUp") move(i < 0 ? rows.length - 1 : i - 1);
    else if (["Home", "End", "PageUp", "PageDown"].includes(ev.key) && rows.length) {
      const row = rowOf(rows[Math.max(i, 0)].entry.path);
      move(moveTarget(ev.key, Math.max(i, 0), rows.length, row instanceof HTMLElement ? pageOf(row) : 1)!);
    }
    else if (!cur) handled = false;
    else if (ev.key === "ArrowRight") {
      // A file: open it and read it.
      if (!cur.isDir) {
        activate(cur);
        focusPanel("code");
      } else if (!shown.expanded.has(cur.path)) setOpen(cur.path, true);
      else if (rows[i + 1] && dirname(rows[i + 1].entry.path) === cur.path) move(i + 1);
    } else if (ev.key === "ArrowLeft") {
      if (cur.isDir && shown.expanded.has(cur.path) && !matching) {
        setOpen(cur.path, false);
        setPicked(null);
      }
      else if (dirname(cur.path)) setSelected(dirname(cur.path));
    } else if (ev.key === "Enter") activate(cur, true);
    else handled = false;
    if (handled) ev.preventDefault();
  };

  const inputRow = (depth: number) =>
    editing?.mode === "new" && (
      <Row depth={depth}>
        <span className="w-3 shrink-0" />
        {editing.isDir ? <FolderIcon name="" open={false} /> : <FileIcon path="" />}
        <NameInput initial="" onDone={finishEditing} />
      </Row>
    );

  const pickedEntries = picked ? rows.filter((r) => picked.paths.has(r.entry.path)).map((r) => r.entry) : [];
  const t = menuTarget;
  // What the menu acts on: the picked rows when it opened on one of them.
  const targets = t && picked?.paths.has(t.path) && pickedEntries.length > 1 ? pickedEntries : t ? [t] : [];
  const multi = targets.length > 1;
  const targetFiles = targets.filter((e) => !e.isDir);
  const targetDiscardable = targets.filter((e) => discardable.has(e.path));
  const menuDir = t ? (t.isDir && !multi ? t.path : null) : "";

  return (
    <div className="flex h-full flex-col">
    {filter.bar}
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <div
          ref={treeRef}
          tabIndex={0}
          onKeyDown={onKeyDown}
          onContextMenu={(ev) => {
            const path = (ev.target as HTMLElement).closest<HTMLElement>("[data-path]")?.dataset.path;
            const entry = rows.find((r) => r.entry.path === path)?.entry ?? null;
            setMenuTarget(entry);
            if (entry) setSelected(entry.path);
            if (!entry || !picked?.paths.has(entry.path)) setPicked(null);
          }}
          className="group/tree min-h-0 flex-1 overflow-x-hidden overflow-y-auto py-1 outline-none"
        >
          {matching && !matching.found && <div className="px-4 py-6 text-center text-[12px] text-subtle">No files match.</div>}
          {editing?.mode === "new" && editing.parent === "" && inputRow(0)}
          {rows.map(({ entry: e, depth }) => {
            const isOpen = shown.expanded.has(e.path);
            const st = fileStatus.get(e.path);
            const tone = st ? statusInfo(st).text : undefined;
            const sel: Selection = { kind: "file", path: e.path };
            const active = !e.isDir && activeKey === selectionKey(sel);
            const renaming = editing?.mode === "rename" && editing.entry.path === e.path;
            return (
              <Fragment key={e.path}>
                <Row
                  depth={depth}
                  path={e.path}
                  onClick={(ev) => click(e, ev)}
                  onDoubleClick={() => !e.isDir && onOpen(sel, true)}
                  onMouseEnter={() => !e.isDir && !e.ignored && onHover(sel)}
                  className={cn(
                    active || picked?.paths.has(e.path) ? "bg-primary/15" : "hover:bg-hover",
                    e.ignored && "opacity-40",
                    // The tree holds the focus, not the row: the keyboard's row looks hovered too.
                    selected === e.path && "group-focus/tree:bg-hover group-focus/tree:outline group-focus/tree:-outline-offset-1 group-focus/tree:outline-primary/70",
                  )}
                >
                  {active && <span className="absolute inset-y-0 left-0 w-0.5 bg-primary" />}
                  {e.isDir ? (
                    <>
                      <ChevronRight className={cn("size-3 shrink-0 text-subtle transition-transform duration-100", isOpen && "rotate-90")} />
                      <FolderIcon name={e.name} open={isOpen} />
                    </>
                  ) : (
                    <>
                      <span className="w-3 shrink-0" />
                      <FileIcon path={e.path} />
                    </>
                  )}
                  {renaming ? (
                    <NameInput initial={e.name} selectStem={!e.isDir} onDone={finishEditing} />
                  ) : (
                    <>
                      <span className={cn("truncate", tone ?? "text-foreground/85")}>{e.name}</span>
                      {st && <span className={cn("ml-auto font-mono text-[10.5px] font-bold", tone)}>{statusInfo(st).letter}</span>}
                      {!st && e.isDir && dirtyDirs.has(e.path) && <span className="ml-auto size-1.5 rounded-full bg-modified" />}
                    </>
                  )}
                </Row>
                {editing?.mode === "new" && editing.parent === e.path && isOpen && inputRow(depth + 1)}
              </Fragment>
            );
          })}
          {matching?.capped && <div className="px-4 py-2 text-center text-[11px] text-subtle">Showing the first {MAX_MATCHES} matches</div>}
        </div>
      </ContextMenuTrigger>
      <ContextMenuContent
        onCloseAutoFocus={(ev) => {
          if (keepFocus.current) ev.preventDefault();
          keepFocus.current = false;
        }}
      >
        {t && !t.isDir && !multi && (
          <>
            <ContextMenuItem onSelect={() => activate(t, true)}>
              <File /> Open <ContextMenuShortcut>↵</ContextMenuShortcut>
            </ContextMenuItem>
            <ContextMenuSeparator />
          </>
        )}
        {menuDir !== null && (
          <>
            <ContextMenuItem onSelect={() => startEditing({ mode: "new", parent: menuDir, isDir: false })}>
              <FilePlus /> New File…
            </ContextMenuItem>
            <ContextMenuItem onSelect={() => startEditing({ mode: "new", parent: menuDir, isDir: true })}>
              <FolderPlus /> New Folder…
            </ContextMenuItem>
            <ContextMenuSeparator />
          </>
        )}
        {t && !multi && (
          <>
            <ContextMenuItem disabled={t.ignored || fileStatus.get(t.path) === "?"} onSelect={() => onShowHistory(t.path, !t.isDir)}>
              <History /> Show History
            </ContextMenuItem>
            <ContextMenuSeparator />
          </>
        )}
        {!multi && (
          <>
            <ContextMenuItem onSelect={() => revealPath(t?.path ?? "")}>
              <FolderSearch /> {REVEAL_LABEL}
            </ContextMenuItem>
            <OpenInMenuItem path={t?.path ?? ""} />
          </>
        )}
        {IS_MAC && targetFiles.length > 0 && status && (
          <ContextMenuItem onSelect={() => copyFiles(targetFiles.map((e) => `${status.root}/${e.path}`))}>
            <Copy /> {copyLabel(targetFiles.map((e) => e.path))}
          </ContextMenuItem>
        )}
        <ContextMenuItem
          disabled={!status}
          onSelect={() => status && copyText(t ? targets.map((e) => `${status.root}/${e.path}`).join("\n") : status.root, multi ? `${targets.length} paths copied` : "Path copied")}
        >
          <Copy /> {multi ? "Copy Paths" : "Copy Path"}
        </ContextMenuItem>
        {t && (
          <>
            <ContextMenuItem onSelect={() => copyText(targets.map((e) => e.path).join("\n"), multi ? `${targets.length} relative paths copied` : "Relative path copied")}>
              <Copy /> {multi ? "Copy Relative Paths" : "Copy Relative Path"}
            </ContextMenuItem>
            <ContextMenuSeparator />
            {!multi && (
              <ContextMenuItem onSelect={() => startEditing({ mode: "rename", entry: t })}>
                <Pencil /> Rename…{renameKey && <ContextMenuShortcut>{renameKey}</ContextMenuShortcut>}
              </ContextMenuItem>
            )}
            {targetDiscardable.length > 0 && (
              <ContextMenuItem onSelect={() => discard(targetDiscardable)}>
                <Undo2 /> {targetDiscardable.length > 1 ? `Discard Changes to ${targetDiscardable.length} Files` : "Discard Changes"}
              </ContextMenuItem>
            )}
            <ContextMenuItem onSelect={() => remove(targets)}>
              <Trash2 /> {topmost(targets).length > 1 ? `Delete ${topmost(targets).length} Items` : "Delete"}
              {deleteKey && <ContextMenuShortcut>{deleteKey}</ContextMenuShortcut>}
            </ContextMenuItem>
          </>
        )}
      </ContextMenuContent>
    </ContextMenu>
    </div>
  );
}

function Row({ depth, path, className, children, ...props }: { depth: number; path?: string } & React.ComponentProps<"div">) {
  return (
    <div
      role="button"
      data-path={path}
      style={{ paddingLeft: 8 + depth * INDENT }}
      className={cn("relative flex h-6 cursor-pointer items-center gap-1.5 pr-2 text-[12px]", className)}
      {...props}
    >
      {/* indent guides */}
      {Array.from({ length: depth }, (_, i) => (
        <span key={i} className="absolute inset-y-0 w-px bg-border" style={{ left: 14 + i * INDENT }} />
      ))}
      {children}
    </div>
  );
}

/** Inline name editor: Enter or blur commits, Escape cancels (VS Code behavior). */
function NameInput({ initial, selectStem, onDone }: { initial: string; selectStem?: boolean; onDone: (name: string | null, refocus: boolean) => void }) {
  const done = useRef(false);
  // Unmounted by the tree (entry vanished): a blur fired during removal must not commit.
  // Reset on mount too: StrictMode's mount/unmount/mount would otherwise leave it true, and the
  // input then ignored Enter, Escape and blur.
  useLayoutEffect(() => {
    done.current = false;
    return () => {
      done.current = true;
    };
  }, []);
  const finish = (name: string | null, refocus: boolean) => {
    if (done.current) return;
    done.current = true;
    onDone(name, refocus);
  };
  return (
    <input
      autoFocus
      defaultValue={initial}
      spellCheck={false}
      autoCapitalize="off"
      autoCorrect="off"
      onFocus={(ev) => {
        // Select "name" of "name.ext" so typing keeps the extension.
        const dot = initial.lastIndexOf(".");
        ev.currentTarget.setSelectionRange(0, selectStem && dot > 0 ? dot : initial.length);
      }}
      onClick={(ev) => ev.stopPropagation()}
      onKeyDown={(ev) => {
        ev.stopPropagation();
        // Enter that confirms an IME composition (e.g. Japanese input) isn't a commit.
        if (ev.key === "Enter" && !ev.nativeEvent.isComposing && ev.keyCode !== 229) finish(ev.currentTarget.value, true);
        else if (ev.key === "Escape") finish(null, true);
      }}
      onBlur={(ev) => finish(ev.currentTarget.value, false)}
      className="h-5 min-w-0 flex-1 rounded-sm border border-primary bg-background px-1 text-[12px] text-foreground outline-none select-text"
    />
  );
}
