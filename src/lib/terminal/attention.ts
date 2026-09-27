// A program asking for the user: the notification escapes agents send (Claude Code's `iterm2`,
// `ghostty` and `kitty` channels, Codex's). Pure, so it runs under `node --test`.

export interface Note {
  title?: string;
  body: string;
}

// A notification is a line or two; a program printing megabytes into one mustn't reach the OS.
const MAX_TEXT = 1000;
const cut = (s: string) => s.slice(0, MAX_TEXT);

/** OSC 9: iTerm2's notification, its text the body. ConEmu's `9;<1-12>` commands aren't one (`9;4` is the progress bar), as in Ghostty. */
export function osc9Note(data: string): Note | null {
  return /^(?:[1-9]|1[0-2])(?:;|$)/.test(data) ? null : { body: cut(data) };
}

/** OSC 777: rxvt's `notify;<title>;<body>`, as Ghostty and foot take it; its other commands aren't one. */
export function osc777Note(data: string): Note | null {
  const m = /^notify;([^;]*)(?:;([\s\S]*))?$/.exec(data);
  return m ? { title: cut(m[1]), body: cut(m[2] ?? "") } : null;
}

/**
 * OSC 99, kitty's: `<key=value:…>;<payload>`, the title (`p=title`, the default) and the body
 * (`p=body`) in chunks of one id until `d=1` (the default), base64 with `e=1`. Queries, icons,
 * buttons and closes aren't a notification. One reader per terminal: it keeps the chunks.
 */
export function kittyNotes() {
  let [id, title, body] = ["", "", ""];
  return (data: string): Note | null => {
    const at = data.indexOf(";");
    if (at < 0) return null;
    const meta = new Map(data.slice(0, at).split(":").map((kv): [string, string] => [kv.split("=")[0], kv.slice(kv.indexOf("=") + 1)]));
    const kind = meta.get("p") || "title";
    if (kind !== "title" && kind !== "body") return null;
    let text = data.slice(at + 1, at + 1 + MAX_TEXT * 2);
    if (meta.get("e") === "1") {
      try {
        const binary = atob(text);
        text = new TextDecoder().decode(Uint8Array.from(binary, (c) => c.charCodeAt(0)));
      } catch {
        return null;
      }
    }
    const of = meta.get("i") ?? "";
    if (of !== id) [id, title, body] = [of, "", ""];
    if (kind === "title") title = cut(title + text);
    else body = cut(body + text);
    if (meta.get("d") === "0") return null;
    const note = { title, body };
    [id, title, body] = ["", "", ""];
    return note;
  };
}
