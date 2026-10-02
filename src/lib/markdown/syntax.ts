// Markdown beyond GFM, as mdast transforms with no JSX or app imports, so node tests run them
// through the real pipeline: GitHub's alerts and math, and Obsidian's own syntax (wikilinks,
// embeds, callouts, ==highlights==, #tags, %%comments%%, block ids, inline footnotes, task
// statuses). The parsing follows Obsidian's documented rules and Quartz's ofm transformer.

/** The bits of mdast these plugins touch. */
export interface MNode {
  type: string;
  value?: string;
  children?: MNode[];
  data?: { hName?: string; hProperties?: Record<string, unknown> };
  position?: { start: { offset?: number }; end: { offset?: number } };
  checked?: boolean | null;
  depth?: number;
  url?: string;
  identifier?: string;
  label?: string;
  ordered?: boolean | null;
  spread?: boolean | null;
}

const text = (value: string): MNode => ({ type: "text", value });
const props = (node: MNode) => ((node.data ??= {}).hProperties ??= {});

/** The plain text of a node, as its heading or link reads. */
export const plainText = (node: MNode): string => node.value ?? (node.children ?? []).map(plainText).join("");

// ---------------------------------------------------------------- comments

const FENCE = /^ {0,3}(`{3,}|~{3,})/;

/**
 * Obsidian's %%comments%%, inline or over many lines, cut from the source before it's parsed;
 * not inside code. One left open hides the rest of the note, as in Obsidian.
 */
export function stripComments(src: string): string {
  if (!src.includes("%%")) return src;
  let out = "";
  let fence: string | null = null;
  let comment = false;
  const lines = src.split("\n");
  lines.forEach((line, n) => {
    const eol = n < lines.length - 1 ? "\n" : "";
    if (!comment) {
      const open = FENCE.exec(line)?.[1];
      if (fence) {
        if (open && open[0] === fence[0] && open.length >= fence.length && !line.trim().slice(open.length)) fence = null;
        out += line + eol;
        return;
      }
      if (open) {
        fence = open;
        out += line + eol;
        return;
      }
    }
    for (let i = 0; i < line.length; ) {
      if (comment) {
        const end = line.indexOf("%%", i);
        if (end < 0) {
          i = line.length;
          break;
        }
        comment = false;
        i = end + 2;
        continue;
      }
      if (line[i] === "`") {
        // An inline code span runs to the next run of as many backticks.
        let run = i;
        while (line[run] === "`") run++;
        const ticks = line.slice(i, run);
        const close = line.indexOf(ticks, run);
        const end = close < 0 ? run : close + ticks.length;
        out += line.slice(i, end);
        i = end;
        continue;
      }
      if (line.startsWith("%%", i)) {
        comment = true;
        i += 2;
        continue;
      }
      out += line[i++];
    }
    if (!comment) out += eol;
  });
  return out;
}

// ---------------------------------------------------------------- callouts

// [!type|metadata]+ Title: Obsidian's callout line; GitHub's alert is [!TYPE] alone on it.
const CALLOUT = /^\[!([\w-]+)(?:\|[^\]]*)?\]([+-]?)[ \t]*/;
const GITHUB_ALERTS = ["note", "tip", "important", "warning", "caution"];

/** Obsidian's callout aliases, by the type each one draws as. */
const ALIASES: Record<string, string> = {
  summary: "abstract",
  tldr: "abstract",
  hint: "tip",
  important: "tip",
  check: "success",
  done: "success",
  help: "question",
  faq: "question",
  caution: "warning",
  attention: "warning",
  fail: "failure",
  missing: "failure",
  error: "danger",
  cite: "quote",
};

const titleCase = (s: string) => s.charAt(0).toUpperCase() + s.slice(1).replace(/-/g, " ");

/**
 * Blockquotes that open with a type become callouts: `div[data-callout]` around a title and a
 * body. `obsidian`: any type, a title and folding (+ open, - closed); else GitHub's five alerts,
 * typed "gh-…" since they look like GitHub's, not Obsidian's.
 */
export function remarkCallouts({ obsidian }: { obsidian: boolean }) {
  const visit = (node: MNode) => {
    node.children?.forEach(visit);
    if (node.type === "blockquote") callout(node, obsidian);
  };
  return (tree: MNode) => visit(tree);
}

function callout(quote: MNode, obsidian: boolean) {
  const first = quote.children?.[0];
  const lead = first?.type === "paragraph" ? first.children?.[0] : undefined;
  if (!first || lead?.type !== "text") return;
  const m = CALLOUT.exec(lead.value!);
  if (!m) return;
  const written = m[1].toLowerCase();
  const rest = lead.value!.slice(m[0].length);
  if (!obsidian && (!GITHUB_ALERTS.includes(written) || m[2] || !/^\s*(\n|$)/.test(rest) || (!rest.includes("\n") && first.children!.length > 1 && first.children![1].type !== "break")))
    return;
  // The title is the rest of the first line: up to the first line break, across inline nodes.
  const inline = [text(rest), ...first.children!.slice(1)];
  const title: MNode[] = [];
  let body: MNode[] = [];
  for (let i = 0; i < inline.length; i++) {
    const n = inline[i];
    const nl = n.type === "text" ? n.value!.indexOf("\n") : -1;
    if (n.type === "break" || nl >= 0) {
      if (nl > 0) title.push(text(n.value!.slice(0, nl)));
      body = [...(nl >= 0 && n.value!.length > nl + 1 ? [text(n.value!.slice(nl + 1))] : []), ...inline.slice(i + 1)];
      break;
    }
    title.push(n);
  }
  const titled = obsidian && title.some((n) => plainText(n).trim() || n.type !== "text");
  const type = obsidian ? (ALIASES[written] ?? written) : `gh-${written}`;
  quote.data = { hName: "div", hProperties: { dataCallout: type, dataCalloutFold: obsidian ? m[2] : "" } };
  quote.children = [
    { type: "calloutTitle", data: { hName: "div", hProperties: { dataCalloutTitle: true } }, children: titled ? trimEnd(title) : [text(titleCase(m[1].toLowerCase()))] },
    {
      type: "calloutBody",
      data: { hName: "div", hProperties: { dataCalloutBody: true } },
      children: [...(body.length ? [{ ...first, children: body }] : []), ...quote.children!.slice(1)],
    },
  ];
}

function trimEnd(nodes: MNode[]) {
  const last = nodes.at(-1);
  if (last?.type === "text") return [...nodes.slice(0, -1), text(last.value!.replace(/\s+$/, ""))];
  return nodes;
}

// ---------------------------------------------------------------- math

/**
 * Inline math as GitHub and Obsidian read it: `$` with no space inside either end and no digit
 * right after the closing one, so "$5 and $10" stays text. Others go back to the text they were.
 */
export function remarkMathGuard() {
  return (tree: MNode, file: { value?: unknown }) => {
    const src = String(file.value ?? "");
    const visit = (node: MNode) => {
      node.children?.forEach((child, i) => {
        if (child.type !== "inlineMath") return visit(child);
        const [start, end] = [child.position?.start.offset, child.position?.end.offset];
        if (start === undefined || end === undefined || src[start + 1] === "$") return;
        if (/^\s|\s$/.test(child.value ?? "") || /\d/.test(src[end] ?? "")) node.children![i] = text(src.slice(start, end));
      });
    };
    visit(tree);
  };
}

// ---------------------------------------------------------------- frontmatter

/** YAML frontmatter (remark-frontmatter's node) as a `div[data-frontmatter]` holding it, which renders as a table. */
export function remarkFrontmatterTable() {
  return (tree: MNode) => {
    const first = tree.children?.[0];
    if (first?.type !== "yaml") return;
    if (!first.value?.trim()) tree.children!.shift();
    else tree.children![0] = { type: "frontmatter", data: { hName: "div", hProperties: { dataFrontmatter: first.value } } };
  };
}

// ---------------------------------------------------------------- obsidian inline syntax

/** A wikilink's parts: `[[path#anchor|alias]]`. */
export interface WikiTarget {
  path: string;
  /** A heading (subheadings joined by #), or ^block-id. */
  anchor: string;
  alias: string | null;
}

export function parseWikilink(inner: string): WikiTarget {
  const bar = inner.indexOf("|");
  const target = bar < 0 ? inner : inner.slice(0, bar);
  const hash = target.indexOf("#");
  return {
    path: (hash < 0 ? target : target.slice(0, hash)).trim(),
    anchor: hash < 0 ? "" : target.slice(hash + 1).trim(),
    alias: bar < 0 ? null : inner.slice(bar + 1).trim(),
  };
}

/** How Obsidian shows a wikilink without an alias: "note > Heading". */
export function wikilinkLabel({ path, anchor, alias }: WikiTarget) {
  if (alias) return alias;
  const parts = anchor ? anchor.split("#").filter(Boolean) : [];
  return [...(path ? [path] : []), ...parts].join(" > ") || path;
}

const WIKILINK = /(!?)\[\[([^[\]\n]+?)\]\]/g;
// Letters, digits, _ - / and emoji, with at least one that isn't a digit; after a space or at the start.
const TAG = /(^|\s)#((?:[\p{L}\p{N}\p{M}_/-]|\p{Extended_Pictographic})+)/gu;
const BLOCK_ID = /(?:^|\s)\^([A-Za-z0-9-]+)\s*$/;
const TASK = /^\[([^\]\n])\][ \t]+/;

/**
 * Obsidian's inline syntax: [[wikilinks]] become links marked `data-wikilink` (the target),
 * ![[embeds]] `span`/`div[data-embed]`, ==highlights== `mark`, #tags `span[data-tag]`, ^[inline
 * footnotes] footnotes, a trailing ^block-id the block's id, and `- [?]` task statuses `data-task`.
 */
export function remarkObsidian() {
  return (tree: MNode) => {
    const notes: MNode[] = [];
    const walk = (node: MNode, inLink: boolean) => {
      if (!node.children) return;
      // A link can't hold another, and a tag's text is already one.
      const link = inLink || node.type === "link" || node.type === "linkReference" || node.type === "tag";
      let children = wrapPairs(node.children, "^[", "]", true, (inner) => {
        const id = `inline-${notes.length + 1}`;
        notes.push({ type: "footnoteDefinition", identifier: id, label: id, children: [{ type: "paragraph", children: inner }] });
        return { type: "footnoteReference", identifier: id, label: id };
      });
      children = wrapPairs(children, "==", "==", false, (inner) => ({ type: "highlight", data: { hName: "mark" }, children: inner }));
      if (!link) children = children.flatMap((c) => (c.type === "text" ? splitText(c.value!) : [c]));
      node.children = children;
      for (const child of children) walk(child, link);
      if (node.type === "listItem") taskStatus(node);
    };
    walk(tree, false);
    // Notes written inline hold more inline syntax, and land after the written ones.
    for (const note of notes) walk(note, false);
    tree.children!.push(...notes);
    blockIds(tree);
    blockEmbeds(tree);
  };
}

/** Text with [[links]], ![[embeds]] and #tags split out. */
function splitText(value: string): MNode[] {
  if (!value.includes("[[") && !value.includes("#")) return [text(value)];
  const out: MNode[] = [];
  let last = 0;
  const push = (to: number) => to > last && out.push(text(value.slice(last, to)));
  const marks = [...value.matchAll(WIKILINK)].map((m) => ({ at: m.index, end: m.index + m[0].length, node: wikiNode(m[1] === "!", m[2]) }));
  for (const m of value.matchAll(TAG)) {
    const tag = m[2].replace(/\/+$/, "");
    const at = m.index + m[1].length;
    // Only digits is a number (#123), not a tag; one inside a wikilink belongs to the link.
    if (!/[^\d/]/.test(tag) || marks.some((w) => at >= w.at && at < w.end)) continue;
    marks.push({ at, end: at + tag.length + 1, node: { type: "tag", data: { hName: "span", hProperties: { dataTag: tag } }, children: [text(`#${tag}`)] } });
  }
  for (const m of marks.sort((a, b) => a.at - b.at)) {
    push(m.at);
    out.push(m.node);
    last = m.end;
  }
  push(value.length);
  return out;
}

function wikiNode(embed: boolean, inner: string): MNode {
  const target = parseWikilink(inner);
  const ref = target.anchor ? `${target.path}#${target.anchor}` : target.path;
  if (embed) return { type: "embed", data: { hName: "span", hProperties: { dataEmbed: ref, dataEmbedAlias: target.alias ?? "" } }, children: [] };
  return { type: "link", url: "#", data: { hProperties: { dataWikilink: ref } }, children: [text(wikilinkLabel(target))] };
}

/**
 * `open`…`close` around inline content, across sibling nodes: text before and after stays,
 * what's between goes to `make`. `nested`: brackets inside must balance, as in ^[a [b] c].
 * Without `nested`, neither end may touch a space (Obsidian's == rule).
 */
function wrapPairs(nodes: MNode[], open: string, close: string, nested: boolean, make: (inner: MNode[]) => MNode): MNode[] {
  if (!nodes.some((n) => n.type === "text" && n.value!.includes(open))) return nodes;
  const out: MNode[] = [];
  const queue = [...nodes];
  while (queue.length) {
    const node = queue.shift()!;
    const v = node.type === "text" ? node.value! : "";
    let at = v.indexOf(open);
    // "===" opens nothing, and neither does "== " (a space after it).
    while (!nested && at >= 0 && (v[at + open.length] === "=" || /\s/.test(v[at + open.length] ?? " "))) at = v.indexOf(open, at + open.length + 1);
    if (at < 0) {
      out.push(node);
      continue;
    }
    const inner: MNode[] = [];
    let depth = 0;
    let found: { taken: number; after: string } | null = null;
    // The rest of this text, then the siblings after it, until the closing mark.
    const stream = [text(v.slice(at + open.length)), ...queue];
    for (let s = 0; s < stream.length && !found; s++) {
      const part = stream[s];
      if (part.type !== "text") {
        inner.push(part);
        continue;
      }
      const t = part.value!;
      for (let i = 0; i < t.length; i++) {
        if (nested && t[i] === "[") depth++;
        else if (t.startsWith(close, i) && (!nested || depth-- === 0)) {
          const before = t.slice(0, i);
          if (!nested && (/\s$/.test(before) || (!before && !inner.length))) continue;
          if (before) inner.push(text(before));
          found = { taken: s, after: t.slice(i + close.length) };
          break;
        }
      }
      if (!found) inner.push(part);
    }
    if (!found) {
      out.push(text(v.slice(0, at + open.length)));
      queue.unshift(text(v.slice(at + open.length)));
      continue;
    }
    if (at > 0) out.push(text(v.slice(0, at)));
    out.push(make(inner));
    queue.splice(0, found.taken);
    if (found.after) queue.unshift(text(found.after));
  }
  // Neighboring text pieces join again, so later patterns see them whole.
  return out.reduce<MNode[]>((acc, n) => {
    const prev = acc.at(-1);
    if (prev?.type === "text" && n.type === "text") acc[acc.length - 1] = text(prev.value! + n.value!);
    else acc.push(n);
    return acc;
  }, []);
}

/** `- [/] item`: a task with a status other than done, which GFM leaves as text. */
function taskStatus(item: MNode) {
  const para = item.children?.[0];
  const lead = para?.type === "paragraph" ? para.children?.[0] : undefined;
  if (item.checked !== null && item.checked !== undefined) {
    props(item).dataTask = item.checked ? "x" : " ";
    return;
  }
  const m = lead?.type === "text" ? TASK.exec(lead.value!) : null;
  if (!m || !lead) return;
  lead.value = lead.value!.slice(m[0].length);
  props(item).dataTask = m[1];
}

/** `text ^id` ends a block with its id (a list item's, for its first line); `^id` alone names the block before it. */
function blockIds(node: MNode) {
  const kids = node.children;
  if (!kids) return;
  for (let i = 0; i < kids.length; i++) {
    const child = kids[i];
    blockIds(child);
    if (child.type !== "paragraph") continue;
    const last = child.children?.at(-1);
    const m = last?.type === "text" ? BLOCK_ID.exec(last.value!) : null;
    if (!m || !last) continue;
    last.value = last.value!.slice(0, m.index);
    const id = `^${m[1]}`;
    const empty = !child.children!.some((c) => c.type !== "text" || c.value!.trim());
    if (empty && i > 0) {
      props(kids[i - 1]).id = id;
      kids.splice(i--, 1);
    } else if (node.type === "listItem" && i === 0) props(node).id = id;
    else props(child).id = id;
  }
}

/** A paragraph that is just one embed is the embed, as a block. */
function blockEmbeds(node: MNode) {
  node.children?.forEach((child, i) => {
    if (child.type !== "paragraph") return blockEmbeds(child);
    const real = child.children!.filter((c) => c.type !== "text" || c.value!.trim());
    if (real.length === 1 && real[0].type === "embed") {
      real[0].data!.hName = "div";
      if (child.data?.hProperties?.id) props(real[0]).id = child.data.hProperties.id;
      node.children![i] = real[0];
    }
  });
}

// ---------------------------------------------------------------- sections

/**
 * Cuts a note down to one part, for an embed of `note#Heading` (the heading and what's under
 * it, to the next heading as high) or `note#^id` (the block with that id).
 */
export function remarkSection({ anchor }: { anchor: string }) {
  return (tree: MNode) => {
    const kids = tree.children ?? [];
    if (anchor.startsWith("^")) {
      const block = findId(tree, anchor);
      tree.children = !block ? [] : block.type === "listItem" ? [{ type: "list", ordered: false, spread: false, children: [block] }] : [block];
      return;
    }
    const want = anchor.split("#").filter(Boolean).at(-1)?.trim().toLowerCase() ?? "";
    const start = kids.findIndex((n) => n.type === "heading" && plainText(n).trim().toLowerCase() === want);
    if (start < 0) {
      tree.children = [];
      return;
    }
    const depth = kids[start].depth ?? 1;
    const end = kids.findIndex((n, i) => i > start && n.type === "heading" && (n.depth ?? 1) <= depth);
    tree.children = kids.slice(start, end < 0 ? undefined : end);
  };
}

function findId(node: MNode, id: string): MNode | null {
  if (node.data?.hProperties?.id === id) return node;
  for (const child of node.children ?? []) {
    const hit = findId(child, id);
    if (hit) return hit;
  }
  return null;
}
