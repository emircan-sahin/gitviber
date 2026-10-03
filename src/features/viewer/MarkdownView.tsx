import { Children, type ComponentProps, isValidElement, type ReactNode, use, useMemo } from "react";
import Markdown, { type Components } from "react-markdown";
import { PageFind } from "@/components/FindBox";
import { github } from "@/lib/api";
import { useHighlight } from "@/lib/editor/highlight";
import { copyNarrowed, indentUnit, TAB, widen } from "@/lib/editor/indent";
import { languageFor } from "@/lib/editor/language";
import { type Flavor, markdownLink, markdownOptions, markdownSource, safeDecode } from "@/lib/github/markdown";
import type { Selection } from "@/lib/repo/selection";
import { useSettings } from "@/lib/settings";
import { failed, toast } from "@/lib/app/toast";
import { copyText } from "@/lib/app/clipboard";
import { type MediaSource, useMediaUrl } from "./MediaView";
import { Callout } from "./markdown/Callout";
import { MarkdownHostContext } from "./markdown/host";
import { MathTex } from "./markdown/MathTex";
import { Mermaid } from "./markdown/Mermaid";
import { Properties } from "./markdown/Properties";

export function isMarkdown(path: string) {
  return /\.(md|markdown|mdown|mkd)$/i.test(path);
}

/** Resolves a link written in `from` (a repo-relative file) to a repo-relative path, or null if it leaves the repo. */
function resolve(from: string, href: string) {
  const parts = href.startsWith("/") ? [] : from.split("/").slice(0, -1);
  for (const seg of safeDecode(href.split(/[?#]/)[0]).split("/")) {
    if (seg === "..") {
      if (!parts.pop()) return null;
    } else if (seg && seg !== ".") parts.push(seg);
  }
  return parts.join("/");
}

function textOf(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (isValidElement<{ children?: ReactNode }>(node)) return textOf(node.props.children);
  return Children.toArray(node).map(textOf).join("");
}

/** GitHub's heading anchors, so `#section` links inside READMEs work. */
export const slug = (children: ReactNode) =>
  textOf(children)
    .toLowerCase()
    .trim()
    .replace(/[^\p{L}\p{N}\s_-]/gu, "")
    .replace(/\s/g, "-");

const heading = (Tag: "h1" | "h2" | "h3" | "h4" | "h5" | "h6", prefix: string) =>
  function Heading({ children }: ComponentProps<"h1">) {
    return <Tag id={prefix + slug(children)}>{children}</Tag>;
  };

// One set per id prefix, so re-renders keep the same component types and code blocks stay mounted.
const bases = new Map<string, Components>();
function baseComponents(prefix: string) {
  let base = bases.get(prefix);
  if (!base) {
    const h = (tag: "h1" | "h2" | "h3" | "h4" | "h5" | "h6") => heading(tag, prefix);
    base = { h1: h("h1"), h2: h("h2"), h3: h("h3"), h4: h("h4"), h5: h("h5"), h6: h("h6"), pre: ({ children }) => <>{children}</>, code: CodeBlock, div: Div, span: Span };
    bases.set(prefix, base);
  }
  return base;
}

const isExternal = (href: string) => /^[a-z][a-z0-9+.-]*:/i.test(href);

/**
 * Follows a link in rendered markdown. Letting the webview follow it would navigate the
 * whole app away: in-page anchors scroll, web links open in the browser, the rest is `local`.
 * `idPrefix` is the rendered block's (see MarkdownBody): anchors point at bare ids.
 */
export function followLink(href: string, local: (href: string) => void, idPrefix = "") {
  if (href.startsWith("#")) {
    const id = safeDecode(href.slice(1));
    (document.getElementById(idPrefix + id) ?? document.getElementById(id))?.scrollIntoView();
  } else if (/^https?:/i.test(href)) {
    let url = href;
    try {
      // Canonical form: lowercase scheme and host, non-ASCII and spaces percent-encoded.
      url = new URL(href).href;
    } catch {
      // Left as written; the backend refuses it and it's copied instead.
    }
    github.openUrl(url).catch(() => navigator.clipboard.writeText(href).then(() => toast("info", "Link copied", "It can't be opened from here."), failed("Could not copy")));
  } else if (isExternal(href)) {
    void copyText(href, "Link copied");
  } else {
    local(href);
  }
}

/**
 * GitHub-flavored markdown with code highlighting, diagrams and math; wrap it in `.markdown` for
 * styling. Every id in it gets `idPrefix`, so give each block on a page its own. `repo` links
 * @mentions and #123; `flavor` and `section` as markdownOptions has them.
 */
export function MarkdownBody({
  text,
  components,
  idPrefix = "user-content-",
  repo,
  flavor,
  section,
}: {
  text: string;
  components: Components;
  idPrefix?: string;
  repo?: string;
  flavor?: Flavor;
  section?: string;
}) {
  const options = useMemo(() => markdownOptions({ idPrefix, repo, flavor, section }), [idPrefix, repo, flavor, section]);
  const source = useMemo(() => markdownSource(text, flavor), [flavor, text]);
  return (
    <Markdown {...options} components={{ ...baseComponents(idPrefix), ...components }}>
      {source}
    </Markdown>
  );
}

/** Rendered markdown. Relative links open in a tab, relative images load from the same revision. */
export function MarkdownView({ text, src, onOpen }: { text: string; src: MediaSource; onOpen: (s: Selection) => void }) {
  const components: Components = {
    a: markdownLink((href) =>
      followLink(
        href,
        (href) => {
          const path = resolve(src.path, href);
          if (path) onOpen({ kind: "file", path });
        },
        "user-content-",
      ),
    ),
    img: ({ src: href, alt, width, height }) => {
      if (typeof href !== "string" || !href) return null;
      if (isExternal(href)) return <img src={href} alt={alt} width={width} height={height} />;
      const path = resolve(src.path, href);
      return path ? <RepoImage src={{ ...src, path, oldPath: null }} alt={alt} width={width} height={height} /> : null;
    },
  };

  return (
    <MarkdownPage>
      <MarkdownBody text={text} components={components} flavor="repo" />
    </MarkdownPage>
  );
}

/** A rendered markdown file filling the view: it scrolls, Find searches it. */
export function MarkdownPage({ children }: { children: ReactNode }) {
  return (
    // Focusable so the keyboard can scroll it (focusPanel("code") lands here).
    <div data-code-scroll tabIndex={0} className="h-full overflow-auto outline-none">
      <PageFind />
      <article className="markdown mx-auto max-w-[860px] px-8 py-6 select-text">{children}</article>
    </div>
  );
}

function RepoImage({ src, ...props }: { src: MediaSource } & Omit<ComponentProps<"img">, "src">) {
  const { url } = useMediaUrl(src, false);
  return url ? <img src={url} {...props} /> : null;
}

function CodeBlock({ className, children }: ComponentProps<"code">) {
  const lang = /language-(\S+)/.exec(className ?? "")?.[1];
  const code = String(children ?? "");
  // remark-math's: $inline$ and $$display$$.
  if (lang === "math" && className?.includes("math-")) return <MathTex tex={code} display={className.includes("math-display")} />;
  // Inline code has no language and no trailing newline; fenced blocks always end in one.
  if (!lang && !code.endsWith("\n")) return <code>{children}</code>;
  const source = code.replace(/\n$/, "");
  if (lang === "math") return <MathTex tex={source} display />;
  const fence = <Fence code={source} lang={lang ? languageFor(`x.${lang}`) : "text"} />;
  if (lang === "mermaid") return <Mermaid code={source} fallback={fence} />;
  return fence;
}

type Data = { node?: unknown; "data-callout"?: string; "data-callout-fold"?: string; "data-frontmatter"?: string; "data-embed"?: string; "data-embed-alias"?: string };

/** The divs the syntax plugins mark: callouts, frontmatter, and embeds the host resolves. */
function Div({ node: _node, ...props }: ComponentProps<"div"> & Data) {
  const host = use(MarkdownHostContext);
  if (props["data-callout"] !== undefined) return <Callout type={props["data-callout"]} fold={props["data-callout-fold"] ?? ""}>{props.children}</Callout>;
  if (props["data-frontmatter"] !== undefined) return <Properties source={props["data-frontmatter"]} />;
  if (props["data-embed"] !== undefined) return host.embed?.(props["data-embed"], props["data-embed-alias"] ?? "", true) ?? null;
  return <div {...props} />;
}

function Span({ node: _node, ...props }: ComponentProps<"span"> & Data) {
  const host = use(MarkdownHostContext);
  if (props["data-embed"] !== undefined) return host.embed?.(props["data-embed"], props["data-embed-alias"] ?? "", false) ?? null;
  return <span {...props} />;
}

function Fence({ code: raw, lang }: { code: string; lang: string }) {
  const s = useSettings();
  const unit = useMemo(() => indentUnit(raw), [raw]);
  const code = useMemo(() => widen(raw, unit), [raw, unit]);
  const hl = useHighlight(code, lang, s.codeTheme);
  const lines = hl?.fresh ? hl.data.lines : null;
  return (
    <pre style={{ tabSize: TAB }} onCopy={copyNarrowed(unit)}>
      <code>
        {lines
          ? lines.map((line, i) => (
              <div key={i}>
                {line.map(([t, color, style], j) => (
                  <span key={j} style={{ color, fontStyle: style & 1 ? "italic" : undefined, fontWeight: style & 2 ? "bold" : undefined }}>
                    {t}
                  </span>
                ))}
                {line.length === 0 && "\n"}
              </div>
            ))
          : code}
      </code>
    </pre>
  );
}
