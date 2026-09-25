// Repo paths are git's: relative to the worktree, "/" between folders, no trailing slash.

/** A repo path's last part: the file or folder name. */
export const basename = (p: string) => p.slice(p.lastIndexOf("/") + 1);

/** The folder a repo path is in ("" for the root). */
export const dirname = (p: string) => p.slice(0, Math.max(0, p.lastIndexOf("/")));

/** `name` in the repo folder `dir` ("" for the root). */
export const childPath = (dir: string, name: string) => (dir ? `${dir}/${name}` : name);

/** A repo path's folder, with its trailing "/" ("" at the root), and its name. */
export function splitPath(path: string) {
  const i = path.lastIndexOf("/");
  return i === -1 ? { dir: "", name: path } : { dir: path.slice(0, i + 1), name: path.slice(i + 1) };
}

/** A path with forward slashes, as git and the index have them (Windows prints backslashes). */
export const slashes = (p: string) => p.replace(/\\/g, "/");

// OS paths: absolute, as the shell or the file dialog gives them.

// A Windows path (C:\… or \\server\…) may mix both separators; elsewhere "\" can be part of a name.
const separator = (path: string) => (/^([a-z]:[\\/]|\\\\)/i.test(path) ? /[\\/]/ : /\//);

/** A folder's name, from its OS path; trailing separators (git prints one) don't count. */
export const folderName = (path: string) => {
  const sep = separator(path);
  return path.replace(new RegExp(`${sep.source}+$`), "").split(sep).pop() ?? path;
};

/** The folder `path` is in, with its trailing separator. */
export const parentFolder = (path: string) => path.slice(0, path.length - folderName(path).length);

/** `name` inside the folder `dir`, joined with the separator `dir` already uses. */
export const joinPath = (dir: string, name: string) => {
  const sep = separator(dir);
  return `${dir.replace(new RegExp(`${sep.source}+$`), "")}${sep.test("\\") && dir.includes("\\") ? "\\" : "/"}${name}`;
};

/** `path` is somewhere inside the folder `dir`. */
export const isInside = (path: string, dir: string) => path.startsWith(dir) && separator(dir).test(path.charAt(dir.length));
