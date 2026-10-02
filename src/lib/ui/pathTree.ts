/** Flat lists of paths shown as a folder tree (Changes' tree view), as VS Code's Source Control tree does it. */

export interface FolderRow<T> {
  kind: "folder";
  /** The deepest folder of a compacted row ("src/lib" is src/lib's). */
  path: string;
  /** "src/lib" for a folder whose only entry is another folder. */
  label: string;
  depth: number;
  /** Everything under it, in the tree's order. */
  items: T[];
  open: boolean;
}

export interface LeafRow<T> {
  kind: "leaf";
  item: T;
  name: string;
  depth: number;
}

export type TreeRow<T> = FolderRow<T> | LeafRow<T>;

interface Node<T> {
  name: string;
  path: string;
  folders: Map<string, Node<T>>;
  leaves: { name: string; item: T }[];
  /** The highest rank under it. */
  rank: number;
}

const byName = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" }).compare;

/**
 * The rows of `items` as a tree: folders first, then files, each by name, or newest first by
 * `rank` (a modified time) when given. A folder holding just one folder shares its row ("src/lib");
 * a top-level folder always starts its own. `closed` folders' contents are left out.
 */
export function pathTree<T>(items: T[], pathOf: (t: T) => string, opts: { closed?: (path: string) => boolean; rank?: (t: T) => number } = {}): TreeRow<T>[] {
  const { closed = () => false, rank } = opts;
  const root: Node<T> = { name: "", path: "", folders: new Map(), leaves: [], rank: -Infinity };
  for (const item of items) {
    const parts = pathOf(item).replace(/\/+$/, "").split("/");
    const r = rank ? rank(item) : 0;
    let node = root;
    node.rank = Math.max(node.rank, r);
    for (const part of parts.slice(0, -1)) {
      let next = node.folders.get(part);
      if (!next) {
        next = { name: part, path: node.path ? `${node.path}/${part}` : part, folders: new Map(), leaves: [], rank: -Infinity };
        node.folders.set(part, next);
      }
      next.rank = Math.max(next.rank, r);
      node = next;
    }
    node.leaves.push({ name: parts[parts.length - 1], item });
  }

  const order = <N extends { name: string }>(list: N[], rankOf: (n: N) => number) =>
    list.sort((a, b) => (rank ? rankOf(b) - rankOf(a) : 0) || byName(a.name, b.name));
  const rows: TreeRow<T>[] = [];
  const all = (node: Node<T>): T[] => [...order([...node.folders.values()], (n) => n.rank).flatMap(all), ...order([...node.leaves], (l) => rank!(l.item)).map((l) => l.item)];
  const walk = (node: Node<T>, depth: number) => {
    for (let folder of order([...node.folders.values()], (n) => n.rank)) {
      let label = folder.name;
      while (folder.folders.size === 1 && !folder.leaves.length) {
        folder = [...folder.folders.values()][0];
        label += `/${folder.name}`;
      }
      const open = !closed(folder.path);
      rows.push({ kind: "folder", path: folder.path, label, depth, items: all(folder), open });
      if (open) walk(folder, depth + 1);
    }
    for (const leaf of order([...node.leaves], (l) => rank!(l.item))) rows.push({ kind: "leaf", item: leaf.item, name: leaf.name, depth });
  };
  walk(root, 0);
  return rows;
}

/** `items` in the order the tree shows them, closed folders included. */
export const treeOrder = <T>(items: T[], pathOf: (t: T) => string, rank?: (t: T) => number): T[] =>
  pathTree(items, pathOf, { rank }).flatMap((r) => (r.kind === "leaf" ? [r.item] : []));

/** The folders `path` sits in, outermost first: "a/b/c.ts" → "a", "a/b". */
export function foldersOf(path: string): string[] {
  const parts = path.replace(/\/+$/, "").split("/").slice(0, -1);
  return parts.map((_, i) => parts.slice(0, i + 1).join("/"));
}
