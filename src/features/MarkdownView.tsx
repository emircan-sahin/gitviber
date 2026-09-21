import { Children, type ComponentProps, isValidElement, type ReactNode } from "react";
import Markdown, { type Components, type Options } from "react-markdown";
import rehypeRaw from "rehype-raw";
import rehypeSanitize, { defaultSchema } from "rehype-sanitize";
import remarkGfm from "remark-gfm";
import { github } from "@/lib/api";
import { useHighlight } from "@/lib/highlight";
import { languageFor } from "@/lib/language";
import type { Selection } from "@/lib/selection";
import { useSettings } from "@/lib/settings";
import { toast } from "@/lib/toast";
import { type MediaSource, useMediaUrl } from "./MediaView";

export function isMarkdown(path: string) {
  return /\.(md|markdown|mdown|mkd)$/i.test(path);
}

/** Resolves a link written in `from` (a repo-relative file) to a repo-relative path, or null if it leaves the repo. */
function resolve(from: string, href: string) {
  const parts = href.startsWith("/") ? [] : from.split("/").slice(0, -1);
  for (const seg of decodeURIComponent(href.split(/[?#]/)[0]).split("/")) {
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
const slug = (children: ReactNode) =>
  textOf(children)
    .toLowerCase()
    .trim()
    .replace(/[^\p{L}\p{N}\s_-]/gu, "")
    .replace(/\s/g, "-");

const heading = (Tag: "h1" | "h2" | "h3" | "h4" | "h5" | "h6") =>
  function Heading({ children }: ComponentProps<"h1">) {
    return <Tag id={slug(children)}>{children}</Tag>;
  };

const headings = { h1: heading("h1"), h2: heading("h2"), h3: heading("h3"), h4: heading("h4"), h5: heading("h5"), h6: heading("h6") };

const isExternal =(href: string) => /^[a-z][a-z0-9+.-]*:/i.test(href);

/**
 * Follows a link in rendered markdown. Letting the webview follow it would navigate the
 * whole app away: in-page anchors scroll, web links open in the browser, the rest is `local`.
 */
export function followLink(href: string, local: (href: string) => void) {
  if (href.startsWith("#")) {
    const id = decodeURIComponent(href.slice(1));
    // The sanitizer prefixes ids from the text itself (footnotes) the way GitHub does.
    (document.getElementById(id) ?? document.getElementById(`user-content-${id}`))?.scrollIntoView();
  } else if (/^https?:/i.test(href)) {
    github.openUrl(href).catch(() => navigator.clipboard.writeText(href).then(() => toast("info", "Link copied", "It can't be opened from here.")));
  } else if (isExternal(href)) {
    navigator.clipboard.writeText(href).then(() => toast("success", "Link copied"));
  } else {
    local(href);
  }
}

type PluggableList = NonNullable<Options["remarkPlugins"]>;

// Inline HTML is common in markdown (<details>, pasted <img> tags), so it's parsed, then cut
// down to GitHub's own allowlist: no scripts, styles, event handlers, iframes or forms, and
// only http(s)/mailto URLs. Nothing from the text ever runs.
const rehypePlugins: PluggableList = [rehypeRaw, [rehypeSanitize, defaultSchema]];

// Module-level so re-renders keep the same component types and code blocks stay mounted.
const base: Components = { ...headings, pre: ({ children }) => <>{children}</>, code: CodeBlock };

/** GitHub-flavored markdown with code highlighting. Wrap it in `.markdown` for styling. */
export function MarkdownBody({ text, components, remarkPlugins = [] }: { text: string; components: Components; remarkPlugins?: PluggableList }) {
  return (
    <Markdown remarkPlugins={[remarkGfm, ...remarkPlugins]} rehypePlugins={rehypePlugins} components={{ ...base, ...components }}>
      {text}
    </Markdown>
  );
}

/** Rendered markdown. Relative links open in a tab, relative images load from the same revision. */
export function MarkdownView({ text, src, onOpen }: { text: string; src: MediaSource; onOpen: (s: Selection) => void }) {
  const components: Components = {
    a: ({ href = "", children }) => (
      <a
        href={href}
        onClick={(e) => {
          e.preventDefault();
          followLink(href, (href) => {
            const path = resolve(src.path, href);
            if (path) onOpen({ kind: "file", path });
          });
        }}
      >
        {children}
      </a>
    ),
    img: ({ src: href, alt, width, height }) => {
      if (typeof href !== "string" || !href) return null;
      if (isExternal(href)) return <img src={href} alt={alt} width={width} height={height} />;
      const path = resolve(src.path, href);
      return path ? <RepoImage src={{ ...src, path, oldPath: null }} alt={alt} width={width} height={height} /> : null;
    },
  };

  return (
    <div className="h-full overflow-auto">
      <article className="markdown mx-auto max-w-[860px] px-8 py-6 select-text">
        <MarkdownBody text={text} components={components} />
      </article>
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
  // Inline code has no language and no trailing newline; fenced blocks always end in one.
  if (!lang && !code.endsWith("\n")) return <code>{children}</code>;
  return <Fence code={code.replace(/\n$/, "")} lang={lang ? languageFor(`x.${lang}`) : "text"} />;
}

function Fence({ code, lang }: { code: string; lang: string }) {
  const s = useSettings();
  const hl = useHighlight(code, lang, s.codeTheme);
  const lines = hl?.fresh ? hl.data.lines : null;
  return (
    <pre>
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
