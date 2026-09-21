import { ask } from "@tauri-apps/plugin-dialog";
import { ChevronRight, Copy, File, FilePlus, FolderPlus, FolderSearch, Pencil, Trash2, Undo2 } from "lucide-react";
import { Fragment, useCallback, useEffect, useImperativeHandle, useLayoutEffect, useMemo, useRef, useState } from "react";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuShortcut, ContextMenuTrigger } from "@/components/ui/context-menu";
import { api, type ChangeStatus, type Entry, errorMessage, type RepoStatus } from "@/lib/api";
import { type Selection, selectionKey } from "@/lib/selection";
import { toast } from "@/lib/toast";
import { cn } from "@/lib/utils";
import { FileIcon, FolderIcon } from "./FileIcon";
import { statusInfo } from "./StatusBadge";

export interface FileTreeHandle {
  collapseAll: () => void;
}

interface Props {
  status: RepoStatus | null;
  revision: number;
  activeKey: string | null;
  onOpen: (s: Selection, pin?: boolean) => void;
  onHover: (s: Selection) => void;
  /** An entry was renamed (`to`) or trashed (`to` = null), so open tabs can follow it. */
  onPathMoved: (from: string, to: string | null) => void;
  ref?: React.Ref<FileTreeHandle>;
}

type Editing = { mode: "rename"; entry: Entry } | { mode: "new"; parent: string; isDir: boolean };

const INDENT = 12;

const parentOf = (path: string) => path.slice(0, Math.max(0, path.lastIndexOf("/")));
const join = (dir: string, name: string) => (dir ? `${dir}/${name}` : name);
const isInside = (path: string, dir: string) => path === dir || path.startsWith(`${dir}/`);

/** Lazy tree of the working directory, like VS Code's explorer, annotated with git status. */
export function FileTree({ status, revision, activeKey, onOpen, onHover, onPathMoved, ref }: Props) {
  const [children, setChildren] = useState<Record<string, Entry[]>>({});
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set([""]));
  // Keyboard cursor, separate from the open tab (activeKey) like VS Code's focused item.
  const [selected, setSelected] = useState<string | null>(null);
  const [editing, setEditing] = useState<Editing | null>(null);
  // Right-clicked entry; null = the empty area below the tree (acts on the repo root).
  const [menuTarget, setMenuTarget] = useState<Entry | null>(null);
  const treeRef = useRef<HTMLDivElement>(null);
  // Set when a menu item starts inline editing, so the closing menu doesn't steal the input's focus.
  const keepFocus = useRef(false);

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

  useImperativeHandle(ref, () => ({
    collapseAll: () => {
      setExpanded(new Set([""]));
      setSelected((s) => s && s.split("/")[0]);
    },
  }));

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
      children[dir]?.forEach((entry) => {
        out.push({ entry, depth });
        if (entry.isDir && expanded.has(entry.path)) walk(entry.path, depth + 1);
      });
    walk("", 0);
    return out;
  }, [children, expanded]);

  // The edited entry (or the folder a new one goes into) vanished on a refresh: drop the input,
  // or keyboard navigation stays disabled with nothing to type into.
  useEffect(() => {
    const path = editing && (editing.mode === "rename" ? editing.entry.path : editing.parent);
    if (!path || rows.some((r) => r.entry.path === path)) return;
    setEditing(null);
    if (document.activeElement === document.body) treeRef.current?.focus();
  }, [rows, editing]);

  useEffect(() => {
    if (selected) treeRef.current?.querySelector(`[data-path="${CSS.escape(selected)}"]`)?.scrollIntoView({ block: "nearest" });
  }, [selected]);

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

  const activate = (e: Entry, pin = false) => (e.isDir ? setOpen(e.path, !expanded.has(e.path)) : onOpen({ kind: "file", path: e.path }, pin));

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
    const dir = ed.mode === "rename" ? parentOf(ed.entry.path) : ed.parent;
    const path = join(dir, name);
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

  const remove = async (e: Entry) => {
    const ok = await ask(`Move "${e.path}" to the Trash?${e.isDir ? " Everything inside it goes too." : ""}`, {
      title: e.isDir ? "Delete folder" : "Delete file",
      kind: "warning",
      okLabel: "Move to Trash",
    });
    if (!ok) return;
    try {
      await api.trashPath(e.path);
      onPathMoved(e.path, null);
      // Land on the next row outside the deleted entry, like VS Code.
      const i = rows.findIndex((r) => r.entry.path === e.path);
      const next = rows.slice(i + 1).find((r) => !isInside(r.entry.path, e.path)) ?? rows[i - 1];
      setSelected(next?.entry.path ?? null);
      loadDir(parentOf(e.path));
    } catch (err) {
      toast("error", "Could not move to Trash", errorMessage(err));
    }
  };

  const discard = async (e: Entry) => {
    const ok = await ask(`Discard changes to ${e.path}? This cannot be undone.`, { title: "Discard changes", kind: "warning", okLabel: "Discard" });
    if (!ok) return;
    try {
      await api.discard([e.path]);
    } catch (err) {
      toast("error", "Discard failed", errorMessage(err));
    }
  };

  const copy = (text: string, what: string) => navigator.clipboard.writeText(text).then(() => toast("success", what));
  const reveal = (path: string) => api.revealPath(path).catch((e) => toast("error", "Could not reveal in Finder", errorMessage(e)));

  const onKeyDown = (ev: React.KeyboardEvent) => {
    if (ev.target !== ev.currentTarget || editing) return;
    // ⌥↑/⌥↓ (next/previous change) and ⌘-arrows belong to the global shortcuts.
    if (ev.key.startsWith("Arrow") && (ev.altKey || ev.metaKey)) return;
    const i = rows.findIndex((r) => r.entry.path === selected);
    const cur = rows[i]?.entry;
    const move = (to: number) => {
      const row = rows[Math.max(0, Math.min(rows.length - 1, to))];
      if (row) setSelected(row.entry.path);
    };
    let handled = true;
    if (ev.key === "ArrowDown") move(i + 1);
    else if (ev.key === "ArrowUp") move(i < 0 ? rows.length - 1 : i - 1);
    else if (!cur) handled = false;
    else if (ev.key === "ArrowRight") {
      if (!cur.isDir) handled = false;
      else if (!expanded.has(cur.path)) setOpen(cur.path, true);
      else if (rows[i + 1] && parentOf(rows[i + 1].entry.path) === cur.path) move(i + 1);
    } else if (ev.key === "ArrowLeft") {
      if (cur.isDir && expanded.has(cur.path)) setOpen(cur.path, false);
      else if (parentOf(cur.path)) setSelected(parentOf(cur.path));
    } else if (ev.key === "Enter") activate(cur, true);
    else if (ev.key === "F2") setEditing({ mode: "rename", entry: cur });
    else if (ev.key === "Backspace" && ev.metaKey) remove(cur);
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

  const t = menuTarget;
  const menuDir = t ? (t.isDir ? t.path : null) : "";

  return (
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
          }}
          className="group/tree h-full overflow-x-hidden overflow-y-auto py-1 outline-none"
        >
          {editing?.mode === "new" && editing.parent === "" && inputRow(0)}
          {rows.map(({ entry: e, depth }) => {
            const isOpen = expanded.has(e.path);
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
                  onClick={() => {
                    setSelected(e.path);
                    activate(e);
                  }}
                  onDoubleClick={() => !e.isDir && onOpen(sel, true)}
                  onMouseEnter={() => !e.isDir && !e.ignored && onHover(sel)}
                  className={cn(
                    active ? "bg-primary/15" : "hover:bg-hover",
                    e.ignored && "opacity-40",
                    selected === e.path && "group-focus/tree:outline group-focus/tree:-outline-offset-1 group-focus/tree:outline-primary/70",
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
        </div>
      </ContextMenuTrigger>
      <ContextMenuContent
        onCloseAutoFocus={(ev) => {
          if (keepFocus.current) ev.preventDefault();
          keepFocus.current = false;
        }}
      >
        {t && !t.isDir && (
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
        <ContextMenuItem onSelect={() => reveal(t?.path ?? "")}>
          <FolderSearch /> Reveal in Finder
        </ContextMenuItem>
        <ContextMenuItem disabled={!status} onSelect={() => status && copy(t ? `${status.root}/${t.path}` : status.root, "Path copied")}>
          <Copy /> Copy Path
        </ContextMenuItem>
        {t && (
          <>
            <ContextMenuItem onSelect={() => copy(t.path, "Relative path copied")}>
              <Copy /> Copy Relative Path
            </ContextMenuItem>
            <ContextMenuSeparator />
            <ContextMenuItem onSelect={() => startEditing({ mode: "rename", entry: t })}>
              <Pencil /> Rename… <ContextMenuShortcut>F2</ContextMenuShortcut>
            </ContextMenuItem>
            {discardable.has(t.path) && (
              <ContextMenuItem onSelect={() => discard(t)}>
                <Undo2 /> Discard Changes
              </ContextMenuItem>
            )}
            <ContextMenuItem onSelect={() => remove(t)}>
              <Trash2 /> Delete <ContextMenuShortcut>⌘⌫</ContextMenuShortcut>
            </ContextMenuItem>
          </>
        )}
      </ContextMenuContent>
    </ContextMenu>
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
  useLayoutEffect(
    () => () => {
      done.current = true;
    },
    [],
  );
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
