// OSC 52, how a program in the terminal copies (neovim's "+y, tmux, anything over SSH). Pure, so
// it runs under `node --test`.

// About 1 MB decoded: past that a `cat` of a hostile file would decode megabytes on the UI thread.
const MAX_BASE64 = 1_400_000;

/**
 * The text an OSC 52 payload (`<targets>;<base64>`) copies, else null. A read (`?`) is never
 * answered: what the user copied elsewhere isn't the program's. Every target lands on the one clipboard.
 */
export function osc52Text(data: string): string | null {
  const at = data.indexOf(";");
  const payload = at < 0 ? "" : data.slice(at + 1);
  if (!payload || payload === "?" || payload.length > MAX_BASE64) return null;
  try {
    // A plain loop: Uint8Array.from with a map function took 50 ms for 1 MB, this 3 ms.
    const binary = atob(payload);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    return null;
  }
}
