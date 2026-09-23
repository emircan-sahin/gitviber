/** The clone dialog's URL field as git gets it: `owner/name` is GitHub shorthand. */
export function cloneUrl(input: string): string {
  const s = input.trim();
  return /^[\w-][\w.-]*\/[\w.-]+$/.test(s) ? `https://github.com/${s.replace(/\.git$/, "")}.git` : s;
}

/** The folder a clone of `url` goes in, as `git clone` would name it: …/owner/name.git → name. */
export function cloneFolderName(url: string): string {
  const path = url
    .trim()
    .replace(/[?#].*$/, "")
    .replace(/\/+$/, "")
    .replace(/\/\.git$/, "")
    .replace(/\.git$/, "");
  // After the last "/" (or the ":" of git@host:owner/name).
  const name = path.split(/[/:]/).pop() ?? "";
  return name === "." || name === ".." ? "" : name;
}
