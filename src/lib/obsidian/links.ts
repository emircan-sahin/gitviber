// How Obsidian finds the file a link names, without JSX or app imports so node tests run it.
// Paths are vault-relative with "/" between folders.

const norm = (s: string) => s.normalize("NFC").toLowerCase();
const dir = (p: string) => p.slice(0, Math.max(0, p.lastIndexOf("/")));
const base = (p: string) => p.slice(p.lastIndexOf("/") + 1);

/** A vault's files by lowercased name, with and without ".md": a link's last part looks its file up there. */
export interface LinkIndex {
  files: string[];
  byName: Map<string, string[]>;
}

const indexes = new WeakMap<string[], LinkIndex>();

export function linkIndex(files: string[]): LinkIndex {
  let index = indexes.get(files);
  if (index) return index;
  const byName = new Map<string, string[]>();
  const add = (key: string, file: string) => {
    const list = byName.get(key);
    if (list) list.push(file);
    else byName.set(key, [file]);
  };
  for (const file of files) {
    const name = norm(base(file));
    add(name, file);
    if (name.endsWith(".md")) add(name.slice(0, -3), file);
  }
  index = { files, byName };
  indexes.set(files, index);
  return index;
}

/** `link` resolved against the note at `from`: a relative path (./, ../) as written, then the rest. */
function relative(link: string, from: string) {
  const parts = dir(from) ? dir(from).split("/") : [];
  for (const seg of link.split("/")) {
    if (seg === "..") parts.pop();
    else if (seg && seg !== ".") parts.push(seg);
  }
  return parts.join("/");
}

/**
 * The file a [[link]] or a markdown link's path names, written in the note at `from`, or null
 * when the vault has none. As Obsidian: names match without regard to case (or Unicode form),
 * ".md" may be left off, and a name alone finds the file anywhere in the vault. Of several that
 * match, the one at that very path from the vault's top wins, then one in `from`'s folder, then
 * the shortest path.
 */
export function resolveLink(index: LinkIndex, link: string, from: string): string | null {
  let want = link.trim().replace(/\\/g, "/");
  if (!want) return from;
  if (want.startsWith("./") || want.startsWith("../")) want = relative(want, from);
  want = norm(want.replace(/^\/+/, ""));
  const candidates = (index.byName.get(base(want)) ?? []).filter((f) => {
    const n = norm(f);
    return [n, n.replace(/\.md$/, "")].some((p) => p === want || p.endsWith(`/${want}`));
  });
  if (!candidates.length) return null;
  // The note before a file without an extension of the same name: [[Plan]] is Plan.md.
  const exact = candidates.find((f) => norm(f) === `${want}.md`) ?? candidates.find((f) => norm(f) === want);
  if (exact) return exact;
  const here = candidates.find((f) => dir(f) === dir(from));
  if (here) return here;
  return [...candidates].sort((a, b) => a.split("/").length - b.split("/").length || a.length - b.length || (a < b ? -1 : 1))[0];
}

/** A file's name as Obsidian shows it: a note without its ".md". */
export const noteName = (path: string) => base(path).replace(/\.md$/i, "");

/** The target of a [[link]] or a markdown link's href, as path and anchor (heading or ^block). */
export function splitTarget(target: string): { path: string; anchor: string } {
  const hash = target.indexOf("#");
  return hash < 0 ? { path: target, anchor: "" } : { path: target.slice(0, hash), anchor: target.slice(hash + 1) };
}
