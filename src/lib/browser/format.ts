import type { BrowserPick, ConsoleEntry } from "../api/browser.ts";

// What goes to an agent from a browser tab: a picked element with the user's note, or the
// page's errors. Plain text, short, in the order an agent would look.

/** The device a page was shown as, for an agent reading a pick. */
export interface ShownAs {
  name: string;
  viewport: { w: number; h: number };
}

const px = (n: number) => Math.round(n);
/** What the page wrote, on one line: a newline in it can't start a line an agent reads as ours. */
const flat = (s: string) => s.replace(/\s+/g, " ").trim();
/** 143.765625px reads as 143.77px. */
const rounded = (v: string) => v.replace(/-?\d*\.\d+(?=px)/g, (n) => String(Math.round(Number(n) * 100) / 100));

/** The element, where it is and what it looks like, the screenshot's path, then the note. */
export function formatPick(pick: BrowserPick, note: string, device: ShownAs | null = null): string {
  const url = flat(pick.url);
  const on = device ? `${url} as ${device.name} (${device.viewport.w}×${device.viewport.h})` : url;
  const styles = Object.entries(pick.styles)
    .filter(([, v]) => v && v !== "none" && v !== "normal" && v !== "auto" && v !== "0px")
    .map(([k, v]) => `${flat(k)}: ${rounded(flat(v))}`)
    .join("; ");
  const lines = [
    `Picked an element on ${on}`,
    `- selector: ${flat(pick.selector)}`,
    pick.components.length > 0 && `- React: ${pick.components.map(flat).join(" < ")}`,
    pick.text && `- text: ${JSON.stringify(pick.text)}`,
    `- box: ${px(pick.box.w)}×${px(pick.box.h)} at ${px(pick.box.x)},${px(pick.box.y)}`,
    styles && `- styles: ${styles}`,
    `- html: ${flat(pick.html)}`,
    pick.screenshot && `- screenshot: ${pick.screenshot}`,
    note.trim() && `Note: ${note.trim()}`,
  ];
  return lines.filter(Boolean).join("\n");
}

/** Stack lines kept under each error: where it was thrown, and a little of how it got there. */
const STACK_LINES = 3;
/** Errors sent at most, the latest. */
const ERRORS = 20;

/** The page's latest errors, each once with how often, oldest first; empty when there are none. */
export function formatErrors(entries: ConsoleEntry[]): string {
  const errors = entries.filter((e) => e.level === "error");
  const seen = new Map<string, { entry: ConsoleEntry; times: number }>();
  for (const e of errors) {
    const was = seen.get(e.msg);
    // Moved to the end: the order is of each message's latest time.
    seen.delete(e.msg);
    seen.set(e.msg, { entry: e, times: (was?.times ?? 0) + 1 });
  }
  const latest = [...seen.values()].slice(-ERRORS);
  if (!latest.length) return "";
  const pages = new Set(latest.map((l) => flat(l.entry.url)));
  const head = pages.size === 1 ? `Errors on ${[...pages][0]}:` : "Errors from the page:";
  const body = latest.map(({ entry, times }) => {
    const stack = entry.stack
      .split("\n")
      .map(flat)
      .filter((l) => l && !entry.msg.includes(l))
      .slice(0, STACK_LINES)
      .map((l) => `    ${l}`);
    const where = pages.size > 1 ? ` (${flat(entry.url)})` : "";
    return [`- ${times > 1 ? `(×${times}) ` : ""}${flat(entry.msg)}${where}`, ...stack].join("\n");
  });
  return [head, ...body].join("\n");
}
