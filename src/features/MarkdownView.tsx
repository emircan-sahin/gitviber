import { Children, type ComponentProps, isValidElement, type ReactNode, useMemo } from "react";
import Markdown, { type Components } from "react-markdown";
import { github } from "@/lib/api";
import { useHighlight } from "@/lib/highlight";
import { languageFor } from "@/lib/language";
import { markdownLink, markdownOptions, safeDecode } from "@/lib/markdown";
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
const slug = (children: ReactNode) =>
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
    base = { h1: h("h1"), h2: h("h2"), h3: h("h3"), h4: h("h4"), h5: h("h5"), h6: h("h6"), pre: ({ children }) => <>{children}</>, code: CodeBlock };
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
    github.openUrl(url).catch(() => navigator.clipboard.writeText(href).then(() => toast("info", "Link copied", "It can't be opened from here.")));
  } else if (isExternal(href)) {
    navigator.clipboard.writeText(href).then(() => toast("success", "Link copied"));
  } else {
    local(href);
  }
}

/**
 * GitHub-flavored markdown with code highlighting; wrap it in `.markdown` for styling. Every
 * id in it gets `idPrefix`, so give each block on a page its own. `repo` links @mentions and #123.
 */
export function MarkdownBody({ text, components, idPrefix = "user-content-", repo }: { text: string; components: Components; idPrefix?: string; repo?: string }) {
  const options = useMemo(() => markdownOptions({ idPrefix, repo }), [idPrefix, repo]);
  return (
    <Markdown {...options} components={{ ...baseComponents(idPrefix), ...components }}>
      {text}
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
