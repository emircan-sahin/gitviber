import { Image as ImageIcon } from "lucide-react";
import { type ComponentProps, useMemo, useState } from "react";
import type { Components } from "react-markdown";
import { github, repoOf } from "@/lib/api";
import { revalidate } from "@/lib/github/githubCache";
import { isGitHubHosted, markdownLink } from "@/lib/github/markdown";
import { cn } from "@/lib/utils";
import { followLink, MarkdownBody } from "@/features/viewer/MarkdownView";

/** Where markdown lives: a PR or issue, or for a draft of a new one (`number` null) its repo's url. */
export type MarkdownHome = { url: string; number: number | null };

/** A PR or issue description or comment, rendered like GitHub does (see MarkdownBody for what's allowed). */
export function PullMarkdown({ pull, idPrefix, text, empty, className }: { pull: MarkdownHome; idPrefix: string; text: string; empty?: string; className?: string }) {
  // Keyed on the PR's values, not the object (refreshed detail, a draft's inline one): new
  // components would remount every image.
  const components = useMemo<Components>(() => {
    // Relative links in PR or issue text are relative to its page, as on github.com.
    const absolute = (href: string) => {
      try {
        return new URL(href, pull.url).href;
      } catch {
        return "";
      }
    };
    return {
      a: markdownLink((href) => followLink(href, (href) => followLink(absolute(href), () => {}), idPrefix)),
      img: ({ src, alt, width, height, title }) => (typeof src === "string" && absolute(src) ? <GitHubImage src={absolute(src)} pull={pull} alt={alt} width={width} height={height} title={title} /> : null),
    };
  }, [pull.url, pull.number, idPrefix]);
  if (!text.trim()) return empty ? <div className={cn("px-3 py-2.5 text-[12px] text-subtle italic", className)}>{empty}</div> : null;
  return (
    <div className={cn("markdown px-3 py-2.5 select-text", className)}>
      <MarkdownBody text={text} components={components} idPrefix={idPrefix} repo={pull.url.split(/\/(?:pull|issues)\//)[0]} />
    </div>
  );
}

// Attachments uploaded to GitHub: github.com/user-attachments/assets/<id>, or the older
// github.com/<owner>/<repo>/assets/<n>/<id>.
const ATTACHMENT = /^https:\/\/github\.com\/(?:user-attachments\/assets|[^/]+\/[^/]+\/assets\/\d+)\/([0-9a-f-]{36})(?:[?#]|$)/i;

/**
 * Public repos' attachments load as they are. A private repo's need a github.com login the
 * webview doesn't have; on failure, swap in the signed link the API hands out instead.
 */
function GitHubImage({ src, pull, ...props }: { src: string; pull: MarkdownHome } & Omit<ComponentProps<"img">, "src">) {
  const [signed, setSigned] = useState<string | null>(null);
  if (!isGitHubHosted(src)) return <ExternalImage src={src} {...props} />;
  const id = ATTACHMENT.exec(src)?.[1];
  const onError = () => {
    // A draft has no page on GitHub to sign its links from.
    if (!id || signed || pull.number === null) return;
    const number = pull.number;
    // Signed links expire after 5 minutes; reuse a lookup for 4.
    revalidate(`attachments:${pull.url}`, () => github.attachments(repoOf(pull.url), number), 240_000)
      .then((urls) => urls[id] && setSigned(urls[id]))
      .catch(() => {});
  };
  return <img src={signed ?? src} onError={onError} {...props} />;
}

/**
 * An image hosted outside GitHub loads only on click: fetching it tells that host your IP
 * and when you read the PR. github.com hides both behind its image proxy; GitViber has none.
 */
function ExternalImage({ src, alt, ...props }: { src: string } & Omit<ComponentProps<"img">, "src">) {
  const [load, setLoad] = useState(false);
  if (load) return <img src={src} alt={alt} {...props} />;
  let host = src;
  try {
    host = new URL(src).hostname;
  } catch {
    // Shown as written.
  }
  return (
    <button
      type="button"
      title={src}
      onClick={() => setLoad(true)}
      className="inline-flex max-w-full items-center gap-1.5 rounded-md border border-border bg-panel px-2 py-1 align-middle text-[12px] text-muted-foreground hover:text-foreground focus-visible:text-foreground"
    >
      <ImageIcon className="size-3.5 shrink-0" />
      <span className="truncate">
        {alt ? `${alt} · ` : ""}Load image from {host}
      </span>
    </button>
  );
}
