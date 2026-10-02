import { createElement, type MouseEvent, type ReactNode } from "react";
import type { Components, Options } from "react-markdown";
import rehypeRaw from "rehype-raw";
import rehypeSanitize, { defaultSchema } from "rehype-sanitize";
import remarkFrontmatter from "remark-frontmatter";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import { remarkCallouts, remarkFrontmatterTable, remarkMathGuard, remarkObsidian, remarkSection } from "../markdown/syntax.ts";

// The markdown pipeline without JSX or app imports, so node tests render through the real thing.

/** The bits of hast the refs plugin touches. */
interface HNode {
  type: string;
  tagName?: string;
  value?: string;
  properties?: Record<string, unknown>;
  children?: HNode[];
}

// Inline HTML is common in markdown (<details>, pasted <img> tags), so it's parsed, then cut
// down to GitHub's own allowlist: no scripts, styles, event handlers, iframes or forms, and
// only http(s)/mailto URLs. <picture>/<source> go too: srcset would load images around the
// img component, which decides what may load.
const base = {
  ...defaultSchema,
  tagNames: defaultSchema.tagNames?.filter((t) => t !== "picture" && t !== "source"),
  attributes: Object.fromEntries(Object.entries(defaultSchema.attributes ?? {}).filter(([tag]) => tag !== "source")),
};
type Schema = typeof base;

const extend = (schema: Schema, attributes: Record<string, string[]>, tagNames: string[] = []): Schema => ({
  ...schema,
  tagNames: [...(schema.tagNames ?? []), ...tagNames],
  attributes: { ...schema.attributes, ...Object.fromEntries(Object.entries(attributes).map(([tag, more]) => [tag, [...(schema.attributes[tag] ?? []), ...more]])) },
});

// What the syntax plugins (lib/markdown/syntax) add, let through on purpose: data attributes
// the components read to draw a callout, math, a table or an embed, and <mark>. Nothing that
// loads or runs; raw HTML carrying them gets a styled box at most.
const githubSchema = extend({ ...base, attributes: { ...base.attributes, code: [["className", /^language-./, "math-inline", "math-display"]] } }, { div: ["dataCallout", "dataCalloutFold", "dataCalloutTitle", "dataCalloutBody"] });
const repoSchema = extend(githubSchema, { div: ["dataFrontmatter"] });
const obsidianSchema = extend(repoSchema, { a: ["dataWikilink"], div: ["dataEmbed", "dataEmbedAlias"], span: ["dataEmbed", "dataEmbedAlias", "dataTag"], li: ["dataTask"] }, ["mark"]);
const SCHEMAS = { github: githubSchema, repo: repoSchema, obsidian: obsidianSchema };

/**
 * Which markdown a block is: GitHub's (PR and issue text: alerts, math), a repo file's (also
 * YAML frontmatter, as a table) or an Obsidian note's (also wikilinks, embeds, callouts and the rest).
 */
export type Flavor = "github" | "repo" | "obsidian";

/**
 * Plugins for one rendered block. `idPrefix` namespaces every id and name in it (headings,
 * footnotes, raw HTML), so blocks on one page don't collide with each other or the app.
 * `repo` (https://github.com/owner/name) turns @mentions and #123 into links. `section` cuts a
 * note down to what an embed of it shows (see remarkSection).
 */
export function markdownOptions({ idPrefix, repo, flavor = "github", section }: { idPrefix: string; repo?: string; flavor?: Flavor; section?: string }): Pick<Options, "remarkPlugins" | "rehypePlugins" | "remarkRehypeOptions"> {
  const obsidian = flavor === "obsidian";
  return {
    remarkPlugins: [
      remarkGfm,
      remarkMath,
      remarkMathGuard,
      ...(flavor === "github" ? [] : [remarkFrontmatter, remarkFrontmatterTable]),
      [remarkCallouts, { obsidian }],
      ...(obsidian ? [remarkObsidian] : []),
      ...(section !== undefined ? [[remarkSection, { anchor: section }] as [typeof remarkSection, { anchor: string }]] : []),
    ],
    // Footnote ids come out bare and the sanitizer prefixes them with everything else.
    remarkRehypeOptions: { clobberPrefix: "" },
    rehypePlugins: [
      rehypeRaw,
      ...(repo ? [[rehypeGithubRefs, { repo }] as [typeof rehypeGithubRefs, { repo: string }]] : []),
      [rehypeSanitize, { ...SCHEMAS[flavor], clobberPrefix: idPrefix }],
    ],
  };
}

// @login (GitHub's username rules) or #123, not inside a word (in any script), path or email
// address; #0 and #007 are no issue.
const REF = /(^|[^\p{L}\p{N}_@/.-])(?:@([a-z\d](?:[a-z\d]|-(?=[a-z\d])){0,38})(?![\p{L}\p{N}_/-])|#([1-9]\d*)(?![\p{L}\p{N}_]))/giu;

/** Links @mentions to profiles and #123 to the repo's issues (GitHub redirects PRs), as GitHub does. */
function rehypeGithubRefs({ repo }: { repo: string }) {
  return (tree: HNode) => walk(tree, repo);
}

function walk(node: HNode, repo: string) {
  // Runs after raw HTML is parsed, so links and code written as HTML are left alone too.
  if (!node.children || (node.type === "element" && ["a", "code", "pre"].includes(node.tagName ?? ""))) return;
  node.children = node.children.flatMap((child) => {
    if (child.type !== "text") {
      walk(child, repo);
      return [child];
    }
    return split(child.value ?? "", repo);
  });
}

function split(value: string, repo: string): HNode[] {
  return githubRefs(value, repo).map(({ text, href }) =>
    href ? { type: "element", tagName: "a", properties: { href }, children: [{ type: "text", value: text }] } : { type: "text", value: text },
  );
}

/** Plain text in pieces, @mentions and #123 with the `href` GitHub gives them (see REF); `repo` is https://github.com/owner/name. */
export function githubRefs(value: string, repo: string): { text: string; href?: string }[] {
  const out: { text: string; href?: string }[] = [];
  let last = 0;
  for (const m of value.matchAll(REF)) {
    const start = m.index + m[1].length;
    if (start > last) out.push({ text: value.slice(last, start) });
    out.push({ text: m[0].slice(m[1].length), href: m[2] ? `https://github.com/${m[2]}` : `${repo}/issues/${m[3]}` });
    last = m.index + m[0].length;
  }
  if (last < value.length) out.push({ text: value.slice(last) });
  return out;
}

const stop = (e: MouseEvent) => e.preventDefault();

/**
 * Links that never navigate the webview: a click goes to `follow`, and the native menu and
 * middle click (open in the app window, open in a new window) are off. A link the sanitizer
 * emptied (javascript: and the like) is plain text; ids and names pass through for anchors.
 */
export function markdownLink(follow: (href: string) => void): NonNullable<Components["a"]> {
  return function Link({ node: _node, href, children, ...rest }) {
    if (!href) return createElement("span", { id: rest.id ?? (rest as { name?: string }).name }, children as ReactNode);
    return createElement("a", { ...rest, href, onClick: (e: MouseEvent) => (stop(e), follow(href)), onContextMenu: stop, onAuxClick: stop }, children as ReactNode);
  };
}

/** Hosts GitHub serves images from; anything else only loads on request (see ExternalImage). */
export function isGitHubHosted(src: string) {
  try {
    const { protocol, hostname } = new URL(src);
    return protocol === "https:" && (hostname === "github.com" || hostname.endsWith(".githubusercontent.com"));
  } catch {
    return false;
  }
}

/** decodeURIComponent that leaves malformed escapes ("%zz") as written instead of throwing. */
export function safeDecode(s: string) {
  try {
    return decodeURIComponent(s);
  } catch {
    return s;
  }
}
