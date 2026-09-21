import { Children, type ComponentProps, isValidElement, type ReactNode } from "react";
import Markdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { github } from "@/lib/api";
import { languageFor, useHighlight } from "@/lib/highlight";
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

const isExternal = (href: string) => /^[a-z][a-z0-9+.-]*:/i.test(href);

/** Rendered markdown. Relative links open in a tab, relative images load from the same revision. */
export function MarkdownView({ text, src, onOpen }: { text: string; src: MediaSource; onOpen: (s: Selection) => void }) {
  const components: Components = {
    ...headings,
    a: ({ href = "", children }) => (
      <a
        href={href}
        onClick={(e) => {
          // Letting the webview follow a link would navigate the whole app away.
          e.preventDefault();
          if (href.startsWith("#")) {
            document.getElementById(href.slice(1))?.scrollIntoView();
          } else if (isExternal(href)) {
            if (href.startsWith("https://github.com/")) github.openUrl(href).catch(() => {});
            else navigator.clipboard.writeText(href).then(() => toast("success", "Link copied"));
          } else {
            const path = resolve(src.path, href);
            if (path) onOpen({ kind: "file", path });
          }
        }}
      >
        {children}
      </a>
    ),
    img: ({ src: href, alt }) => {
      if (typeof href !== "string" || !href) return null;
      if (isExternal(href)) return <img src={href} alt={alt} />;
      const path = resolve(src.path, href);
      return path ? <RepoImage src={{ ...src, path, oldPath: null }} alt={alt} /> : null;
    },
    pre: ({ children }) => <>{children}</>,
    code: CodeBlock,
  };

  return (
    <div className="h-full overflow-auto">
      <article className="markdown mx-auto max-w-[860px] px-8 py-6 select-text">
        <Markdown remarkPlugins={[remarkGfm]} components={components}>
          {text}
        </Markdown>
      </article>
    </div>
  );
}

function RepoImage({ src, alt }: { src: MediaSource; alt?: string }) {
  const { url } = useMediaUrl(src, false);
  return url ? <img src={url} alt={alt} /> : null;
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
