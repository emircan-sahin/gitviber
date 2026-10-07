import { TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";
import type { Components } from "react-markdown";
import { FileIcon } from "@/components/FileIcon";
import { markdownLink } from "@/lib/github/markdown";
import { type GuideSection, sectionBadge } from "@/lib/review/guide";
import { cn } from "@/lib/utils";
import { followLink, MarkdownBody } from "@/features/viewer/MarkdownView";

// The guide's Markdown ids all start with this, so none can take a section row's heading id.
const MD = "guide-md-";
const components: Components = { a: markdownLink((href) => followLink(href, () => {}, MD)) };

/** Markdown the agent wrote; `id` sets its block apart from the others on the page. */
export function GuideMarkdown({ text, id = "" }: { text: string; id?: string }) {
  return (
    <div className="markdown select-text">
      <MarkdownBody text={text} components={components} idPrefix={MD + id} />
    </div>
  );
}

export function Notice({ icon, role, className, children }: { icon: ReactNode; role?: "status" | "alert"; className?: string; children: ReactNode }) {
  return (
    <div role={role} className={cn("mb-4 flex items-start gap-2 rounded-md border border-border bg-panel px-3 py-2 text-[12.5px] [&>svg]:mt-0.5 [&>svg]:size-3.5 [&>svg]:shrink-0", className)}>
      {icon}
      <div className="min-w-0">{children}</div>
    </div>
  );
}

/**
 * One part of the change: its prose (held in view while its diffs scroll by), beside its files'
 * diffs; one column when the tab is narrow. Marked reviewed, it folds to its title. Without `n`,
 * the files no section has.
 */
export function SectionRow({
  n,
  total,
  section: s,
  done = false,
  onDone,
  files,
  named = [],
  onSection,
}: {
  n?: number;
  total?: number;
  section: GuideSection;
  done?: boolean;
  onDone?: (on: boolean) => void;
  /** Its files' diffs; null while the files load. */
  files: ReactNode[] | null;
  /** The files it names that another section shows (`at`), or that the diff doesn't have. */
  named?: { path: string; at: number | null }[];
  onSection?: (n: number) => void;
}) {
  const heading = `guide-section-${n ?? "other"}`;
  const title = s.title || "Untitled";
  return (
    <section data-section={n} aria-labelledby={heading} className={cn("grid gap-x-6 gap-y-3 border-t border-border py-5 @4xl:grid-cols-[minmax(0,2fr)_minmax(0,3fr)]", done && "opacity-60")}>
      <div className="min-w-0 @4xl:sticky @4xl:top-0 @4xl:self-start @4xl:pt-1">
        <div className="flex items-baseline gap-2">
          <h2 id={heading} tabIndex={-1} className="min-w-0 flex-1 text-[15px] font-semibold outline-none select-text">
            {title}
          </h2>
          {n && total && (
            <span className="shrink-0 font-mono text-[11px] text-subtle">
              {sectionBadge(n)} / {sectionBadge(total)}
            </span>
          )}
        </div>
        {onDone && (
          <label className="mt-1.5 inline-flex items-center gap-1.5 text-[12px] text-muted-foreground">
            <input type="checkbox" checked={done} onChange={(e) => onDone(e.target.checked)} aria-label={`Reviewed: ${title}`} />
            Reviewed
          </label>
        )}
        {!done && (
          <div className="mt-3 flex flex-col gap-2">
            {s.summary && <GuideMarkdown text={s.summary} id={`${n ?? "other"}-`} />}
            {s.risk && (
              <Notice icon={<TriangleAlert />} className="text-modified">
                {s.risk}
              </Notice>
            )}
            {named.map(({ path, at }) => (
              <div key={path} className="flex min-w-0 items-center gap-1.5 text-[12px]">
                <FileIcon path={path} />
                <span className="min-w-0 truncate font-mono text-subtle" title={path}>
                  {path}
                </span>
                {at ? (
                  <button className="shrink-0 text-primary hover:underline" onClick={() => onSection?.(at)}>
                    in section {sectionBadge(at)}
                  </button>
                ) : (
                  <span className="shrink-0 text-subtle">not in this diff</span>
                )}
              </div>
            ))}
          </div>
        )}
      </div>
      {!done && (
        <div className="min-w-0">
          {files === null ? (
            <div className="text-[12px] text-subtle">Loading files…</div>
          ) : (
            files.length > 0 && <div className="overflow-clip rounded-md border border-border [&>section:last-child]:border-b-0">{files}</div>
          )}
        </div>
      )}
    </section>
  );
}
