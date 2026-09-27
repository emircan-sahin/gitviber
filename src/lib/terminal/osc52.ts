// OSC 52, how a program in the terminal copies (neovim's "+y, tmux, anything over SSH). Pure, so
// it runs under `node --test`.

// About 1 MB decoded: past that a `cat` of a hostile file would decode megabytes on the UI thread.
const MAX_BASE64 = 1_400_000;

/**
 * The text an OSC 52 payload (`<targets>;<base64>`) copies, or null: a read (`?`) is never
 * answered, since what the user copied elsewhere isn't the program's; nor is bad base64 or a copy
 * over MAX_BASE64.
 * Every target (clipboard, primary, cut buffers) lands on the one clipboard.
 */
export function osc52Text(data: string): string | null {
  const at = data.indexOf(";");
  const payload = at < 0 ? "" : data.slice(at + 1);
  if (!payload || payload === "?" || payload.length > MAX_BASE64) return null;
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(Uint8Array.from(atob(payload), (c) => c.charCodeAt(0)));
  } catch {
    return null;
  }
}
