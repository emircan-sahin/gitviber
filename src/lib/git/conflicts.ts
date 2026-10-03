// Parsing of git conflict markers. Kept free of UI code so it can be tested on its own.

import type { Operation } from "../api";

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

/**
 * Whether the file the choices make ends with a newline. Git ends a closing marker line with one
 * even where neither side's file did, so for a conflict at the very end it comes from the sides
 * (`sides`, from the index; null while unknown): the one chosen, the incoming one for both (it
 * comes last), either for an edit by hand.
 */
export function endsWithNewline(parsed: { segments: Segment[]; trailingNewline: boolean }, lastChoice: "ours" | "theirs" | "both" | "custom" | undefined, sides: { oursNewline: boolean; theirsNewline: boolean } | null): boolean {
  if (parsed.segments.at(-1)?.t !== "conflict" || !lastChoice || !sides) return parsed.trailingNewline;
  if (lastChoice === "ours") return sides.oursNewline;
  if (lastChoice === "custom") return sides.oursNewline || sides.theirsNewline;
  return sides.theirsNewline;
}

/** Whether a file still has conflict blocks, or a start marker left open. */
export function hasConflictMarkers(text: string): boolean {
  const parsed = parseConflicts(text);
  return !parsed || parsed.segments.some((s) => s.t === "conflict");
}

/** The file as "current" would leave it, for sniffing its language without the marker lines. */
export function oursText(segments: Segment[]): string {
  return segments.flatMap((s) => (s.t === "text" ? s.lines : s.ours)).join("\n");
}

const sameLine = (a: string, b: string) => a.replace(/\r$/, "") === b.replace(/\r$/, "");

/** Where `inner` first runs as lines within `outer` at or after `from`, or -1. */
function runAt(outer: string[], inner: string[], from: number) {
  for (let i = from; i + inner.length <= outer.length; i++) if (inner.every((l, j) => sameLine(outer[i + j], l))) return i;
  return -1;
}

/**
 * Each block's base: its own (diff3 style wrote one), else that of the block in `rebuilt` (the file
 * merged again in diff3 style, from its index stages) that holds both its sides. Git's default
 * style trims and splits the conflicts diff3 style keeps whole, so a block can be a part of one.
 * Null for a block edited since, which none holds. One side may be empty (it deleted the lines),
 * which fits anywhere: the other side, never empty too, places it.
 */
export function basesFor(blocks: Block[], rebuilt: Block[]): (string[] | null)[] {
  // Walked in file order: a rebuilt block, and within it each side, only forward from where the
  // last block was found. So twins (the same edit in two places) each find their own, and two
  // parts split from one find it in turn.
  let at = 0;
  let ours = 0;
  let theirs = 0;
  return blocks.map((b) => {
    if (b.base) return b.base;
    for (let i = at; i < rebuilt.length; i++) {
      const from = i === at ? [ours, theirs] : [0, 0];
      const o = runAt(rebuilt[i].ours, b.ours, from[0]);
      const t = o < 0 ? -1 : runAt(rebuilt[i].theirs, b.theirs, from[1]);
      if (t < 0) continue;
      [at, ours, theirs] = [i, o + Math.max(1, b.ours.length), t + Math.max(1, b.theirs.length)];
      return rebuilt[i].base;
    }
    // Git's default style also joins conflicts a line or two apart that diff3 style keeps as two:
    // such a block is those rebuilt ones with the lines between them.
    for (let i = at; i < rebuilt.length - 1; i++) {
      const joined = joinedFrom(b, rebuilt, i);
      if (!joined) continue;
      [at, ours, theirs] = [joined.last, rebuilt[joined.last].ours.length, rebuilt[joined.last].theirs.length];
      return joined.base;
    }
    return null;
  });
}

const sameRun = (a: string[], b: string[]) => a.length === b.length && a.every((l, i) => sameLine(l, b[i]));

/** The base of `b` if it is `rebuilt[from]` and the blocks after it, with the lines both sides share between them. */
function joinedFrom(b: Block, rebuilt: Block[], from: number): { base: string[]; last: number } | null {
  const base: string[] = [];
  let [o, t] = [0, 0];
  for (let k = from; k < rebuilt.length; k++) {
    const r = rebuilt[k];
    if (!r.base || !sameRun(b.ours.slice(o, o + r.ours.length), r.ours) || !sameRun(b.theirs.slice(t, t + r.theirs.length), r.theirs)) return null;
    base.push(...r.base);
    o += r.ours.length;
    t += r.theirs.length;
    if (o === b.ours.length && t === b.theirs.length) return k > from ? { base, last: k } : null;
    // What lies between is the same in both sides; how much, the next block's start tells.
    const between = [...Array(Math.min(b.ours.length - o, b.theirs.length - t) + 1).keys()].find((n) => {
      const next = rebuilt[k + 1];
      return (
        next?.base &&
        sameRun(b.ours.slice(o, o + n), b.theirs.slice(t, t + n)) &&
        sameRun(b.ours.slice(o + n, o + n + next.ours.length), next.ours) &&
        sameRun(b.theirs.slice(t + n, t + n + next.theirs.length), next.theirs)
      );
    });
    if (between === undefined) return null;
    base.push(...b.ours.slice(o, o + between));
    o += between;
    t += between;
  }
  return null;
}

/** What's being merged, in an agent's words; `theirs`: the incoming side's marker label. */
export function mergingWhat(kind: Operation["kind"] | undefined, subject: string | null, branch: string | null, theirs: string | null): string | null {
  if (kind === "merge") return `merging ${theirs || "the incoming branch"} into ${branch || "HEAD"}`;
  if (kind === "rebase") return `rebasing ${subject || "the branch"}${theirs ? `, replaying ${theirs}` : ""}`;
  if (kind === "cherry-pick") return `cherry-picking ${theirs || "a commit"}`;
  if (kind === "revert") return `reverting ${theirs || "a commit"}`;
  return null;
}

/** The prompt "Ask agent to resolve" pastes for the agent. */
export function resolvePrompt(files: string[], merging: string | null): string {
  return `Resolve the merge conflicts in ${files.join(", ")}${merging ? ` (${merging})` : ""}; keep both sides' intent, then stage the files.`;
}
