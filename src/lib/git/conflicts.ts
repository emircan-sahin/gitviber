// Parsing of git conflict markers. Kept free of UI code so it can be tested on its own.

export type Block = { id: number; ours: string[]; base: string[] | null; theirs: string[]; oursLabel: string; theirsLabel: string };
export type Segment = { t: "text"; lines: string[] } | ({ t: "conflict" } & Block);

const marker = (line: string, m: string) => {
  const l = line.endsWith("\r") ? line.slice(0, -1) : line;
  return l === m || l.startsWith(`${m} `) ? l.slice(m.length).trim() : null;
};

/** Splits a file with conflict markers into text runs and conflict blocks (diff3 base optional). */
export function parseConflicts(text: string): { segments: Segment[]; trailingNewline: boolean } | null {
  const lines = text.split("\n");
  const trailingNewline = lines[lines.length - 1] === "";
  if (trailingNewline) lines.pop();
  const segments: Segment[] = [];
  let buf: string[] = [];
  let block: Block | null = null;
  let part: "ours" | "base" | "theirs" = "ours";
  let id = 0;
  for (const line of lines) {
    if (!block) {
      const label = marker(line, "<<<<<<<");
      if (label === null) {
        buf.push(line);
        continue;
      }
      if (buf.length) segments.push({ t: "text", lines: buf });
      buf = [];
      block = { id: id++, ours: [], base: null, theirs: [], oursLabel: label, theirsLabel: "" };
      part = "ours";
      continue;
    }
    if (part === "ours" && marker(line, "|||||||") !== null) {
      block.base = [];
      part = "base";
    } else if (part !== "theirs" && marker(line, "=======") === "") {
      part = "theirs";
    } else if (part === "theirs" && marker(line, ">>>>>>>") !== null) {
      block.theirsLabel = marker(line, ">>>>>>>")!;
      segments.push({ t: "conflict", ...block });
      block = null;
    } else if (part === "base") block.base!.push(line);
    else block[part].push(line);
  }
  if (block) return null; // unterminated markers: don't guess
  if (buf.length) segments.push({ t: "text", lines: buf });
  return { segments, trailingNewline };
}

/** The file as "current" would leave it, for sniffing its language without the marker lines. */
export function oursText(segments: Segment[]): string {
  return segments.flatMap((s) => (s.t === "text" ? s.lines : s.ours)).join("\n");
}
