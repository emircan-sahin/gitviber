import { useCallback, useEffect, useRef, useState } from "react";
import { compareEntries, dirname } from "@/lib/path";
import { moveTarget, pageOf } from "@/lib/ui/useListNav";

/** A file or folder a tree shows. */
export interface TreeEntry {
  name: string;
  path: string;
  isDir: boolean;
}

/** A fresh, unsorted listing that says what `shown` already does (`same`: an entry still looks the same). */
function unchanged<E extends TreeEntry>(shown: E[] | undefined, listed: E[], same: (was: E, now: E) => boolean) {
  if (shown?.length !== listed.length) return false;
  const byName = new Map(shown.map((e) => [e.name, e]));
  return listed.every((e) => {
    const was = byName.get(e.name);
    return !!was && was.isDir === e.isDir && same(was, e);
  });
}

/**
 * A tree listed folder by folder as they open, like VS Code's explorer: the explorer's files
 * and an Obsidian vault's. Every open folder is listed again when `revision` moves; a folder
 * gone since (agents and sync tools delete them) closes quietly. `onRootError`: the top
 * couldn't be listed, which stays open so the tree comes back once it can.
 */
export function useLazyTree<E extends TreeEntry>({
  list,
  revision,
  same = () => true,
  onRootError,
  initial = new Set([""]),
}: {
  list: (path: string) => Promise<E[]>;
  revision: unknown;
  same?: (was: E, now: E) => boolean;
  onRootError: (e: unknown) => void;
  initial?: Set<string>;
}) {
  const [children, setChildren] = useState<Record<string, E[]>>({});
  const [expanded, setExpanded] = useState<Set<string>>(initial);
  const args = useRef({ list, same, onRootError });
  args.current = { list, same, onRootError };

  // Per-path request counter: a slow, older listing must not overwrite a newer one.
  const requests = useRef(new Map<string, number>());
  const loadDir = useCallback(async (path: string) => {
    const id = (requests.current.get(path) ?? 0) + 1;
    requests.current.set(path, id);
    try {
      const listed = await args.current.list(path);
      // Every refresh re-lists each open folder: an unchanged one skips the sort (13 ms at 10k
      // entries) and the tree's re-render.
      if (requests.current.get(path) === id) setChildren((c) => (unchanged(c[path], listed, args.current.same) ? c : { ...c, [path]: listed.sort(compareEntries) }));
    } catch (e) {
      if (requests.current.get(path) !== id) return;
      if (path === "") return args.current.onRootError(e);
      setExpanded((x) => {
        const next = new Set(x);
        next.delete(path);
        return next;
      });
      setChildren(({ [path]: _gone, ...rest }) => rest);
    }
  }, []);

  useEffect(() => {
    expanded.forEach((p) => loadDir(p));
  }, [revision, loadDir]);

  const setOpen = useCallback(
    (path: string, open: boolean) => {
      setExpanded((x) => {
        const next = new Set(x);
        if (open) next.add(path);
        else next.delete(path);
        return next;
      });
      // Always re-list: the cached listing may be from before something changed it.
      if (open) loadDir(path);
    },
    [loadDir],
  );

  return { children, expanded, setExpanded, loadDir, setOpen };
}

/** What's on screen, top to bottom: the entries of the open folders, each with its depth. */
export function treeRows<E extends TreeEntry>(children: Record<string, E[]>, expanded: Set<string>) {
  const out: { entry: E; depth: number }[] = [];
  const walk = (dir: string, depth: number) =>
    children[dir]?.forEach((entry) => {
      out.push({ entry, depth });
      if (entry.isDir && expanded.has(entry.path)) walk(entry.path, depth + 1);
    });
  walk("", 0);
  return out;
}

/** What a key does in a tree, for the tree to carry out (picking with ⇧, say, is the tree's own). */
export type TreeKey<E> = { move: number } | { open: string } | { close: string } | { select: string } | { activate: E; pin: boolean; focusCode: boolean };

/**
 * The tree keys, as in VS Code's explorer: ↑ ↓ Home End PgUp PgDn move, → opens a folder (then
 * steps into it) or a file (and moves to its code), ← closes a folder or goes to its parent, ↵
 * opens for good. `cur`: the keyboard's row (-1: none); `row`: its element, for a page's size.
 * `isOpen`: the folder shows its entries; `closable`: it may be closed (a filter holds some open).
 */
export function treeKey<E extends TreeEntry>(
  key: string,
  rows: { entry: E }[],
  cur: number,
  { isOpen, closable = () => true, row }: { isOpen: (path: string) => boolean; closable?: (path: string) => boolean; row?: Element | null },
): TreeKey<E> | null {
  if (key === "ArrowDown") return { move: cur + 1 };
  if (key === "ArrowUp") return { move: cur < 0 ? rows.length - 1 : cur - 1 };
  if (["Home", "End", "PageUp", "PageDown"].includes(key) && rows.length)
    return { move: moveTarget(key, Math.max(cur, 0), rows.length, row instanceof HTMLElement ? pageOf(row) : 1)! };
  const e = rows[cur]?.entry;
  if (!e) return null;
  if (key === "ArrowRight") {
    if (!e.isDir) return { activate: e, pin: false, focusCode: true };
    if (!isOpen(e.path)) return { open: e.path };
    return rows[cur + 1] && dirname(rows[cur + 1].entry.path) === e.path ? { move: cur + 1 } : { select: e.path };
  }
  if (key === "ArrowLeft") {
    if (e.isDir && isOpen(e.path) && closable(e.path)) return { close: e.path };
    return { select: dirname(e.path) || e.path };
  }
  if (key === "Enter") return { activate: e, pin: true, focusCode: false };
  return null;
}
