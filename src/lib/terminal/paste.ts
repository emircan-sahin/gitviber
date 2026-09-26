// Paths the terminal pastes for a dropped or copied file, or a pasted image (clipboard.rs). Pure,
// so it runs under `node --test`.

/** As Ghostty escapes them: a shell reads it as one word, and an AI CLI still finds the file. */
const SPECIAL = /[\\ ()[\]{}<>"'`!#$&;|*?\t]/g;

/**
 * `path` escaped for a shell, or null when it holds a control character: a name with a newline
 * or ESC in it could run a command or end the bracketed paste early.
 */
export function shellPath(path: string): string | null {
  if (/[\x00-\x08\x0a-\x1f\x7f]/.test(path)) return null;
  return path.replace(SPECIAL, (c) => `\\${c}`);
}

/**
 * The pastes for `paths`, one each: Claude Code attaches only the last path of a paste that holds
 * several. From the second on they start with a space, to stay separate words.
 */
export function pathPastes(paths: string[]): string[] {
  return paths
    .map(shellPath)
    .filter((p) => p !== null)
    .map((p, i) => (i ? ` ${p}` : p));
}
