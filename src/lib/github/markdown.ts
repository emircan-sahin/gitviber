import { createElement, type MouseEvent, type ReactNode } from "react";
import type { Components, Options } from "react-markdown";
import rehypeRaw from "rehype-raw";
import rehypeSanitize, { defaultSchema } from "rehype-sanitize";
import remarkGfm from "remark-gfm";

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
const schema = {
  ...defaultSchema,
  tagNames: defaultSchema.tagNames?.filter((t) => t !== "picture" && t !== "source"),
  attributes: Object.fromEntries(Object.entries(defaultSchema.attributes ?? {}).filter(([tag]) => tag !== "source")),
};

/**
 * Plugins for one rendered block. `idPrefix` namespaces every id and name in it (headings,
 * footnotes, raw HTML), so blocks on one page don't collide with each other or the app.
 * `repo` (https://github.com/owner/name) turns @mentions and #123 into links.
 */
export function markdownOptions({ idPrefix, repo }: { idPrefix: string; repo?: string }): Pick<Options, "remarkPlugins" | "rehypePlugins" | "remarkRehypeOptions"> {
  return {
    remarkPlugins: [remarkGfm],
    // Footnote ids come out bare and the sanitizer prefixes them with everything else.
    remarkRehypeOptions: { clobberPrefix: "" },
    rehypePlugins: [rehypeRaw, ...(repo ? [[rehypeGithubRefs, { repo }] as [typeof rehypeGithubRefs, { repo: string }]] : []), [rehypeSanitize, { ...schema, clobberPrefix: idPrefix }]],
  };
}

// @login (GitHub's username rules) or #123, not inside a word, path or email address.
const REF = /(^|[^\w@/.-])(?:@([a-z\d](?:[a-z\d]|-(?=[a-z\d])){0,38})(?![\w/-])|#(\d+)\b)/gi;

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
  const out: HNode[] = [];
  let last = 0;
  for (const m of value.matchAll(REF)) {
    const start = m.index + m[1].length;
    if (start > last) out.push({ type: "text", value: value.slice(last, start) });
    const href = m[2] ? `https://github.com/${m[2]}` : `${repo}/issues/${m[3]}`;
    out.push({ type: "element", tagName: "a", properties: { href }, children: [{ type: "text", value: m[0].slice(m[1].length) }] });
    last = m.index + m[0].length;
  }
  if (last < value.length) out.push({ type: "text", value: value.slice(last) });
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
