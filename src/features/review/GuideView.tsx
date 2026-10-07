import { LoaderCircle, RotateCw, Sparkles, Square, TriangleAlert } from "lucide-react";
import { type ReactNode, useMemo, useReducer, useRef } from "react";
import type { Components } from "react-markdown";
import { PageFind } from "@/components/FindBox";
import { Button } from "@/components/ui/button";
import { FileIcon } from "@/components/FileIcon";
import type { RepoStatus } from "@/lib/api";
import { relativeTime } from "@/lib/format";
import { programOf } from "@/lib/git/suggest";
import { markdownLink } from "@/lib/github/markdown";
import { type ChangesSelection, type GuideSelection, type Selection, selectionKey } from "@/lib/repo/selection";
import { type GuideSection, parseGuide, placeFiles, sectionBadge } from "@/lib/review/guide";
import { useSettings } from "@/lib/settings";
import { cn } from "@/lib/utils";
import { followLink, MarkdownBody } from "@/features/viewer/MarkdownView";
import { Mermaid } from "@/features/viewer/markdown/Mermaid";
import { FileBlock, fileRevision, type ListFile } from "@/features/viewer/AllChanges";
import { useFixedFiles } from "@/features/viewer/fixedFiles";
import { fileMemo } from "@/features/viewer/stackedMemo";
import { openSettings } from "@/features/settings/SettingsDialog";
import { cancelGuide, generateGuide, guideId, markDone, useGuide } from "./guides";
import { GuideDiagram } from "./GuideDiagram";

const PREFIX = "guide-";
const components: Components = { a: markdownLink((href) => followLink(href, () => {}, PREFIX)) };
// No files to read before a branch's guide says which range it read: a list useFixedFiles leaves be.
const NO_FILES: ChangesSelection = { kind: "changes", list: "unstaged" };

interface Props {
  sel: GuideSelection;
  status: RepoStatus | null;
  revision: number;
  viewed: (sel: Selection) => boolean;
  toggleViewed: (sel: Selection) => void;
  onOpen: (s: Selection, pin?: boolean) => void;
}

/**
 * A commit or branch explained by the user's agent CLI, kept once written: an overview and a
 * diagram of the changed parts, then a row a section in reading order, its prose beside its files'
 * diffs (drawn as Open All draws them, only near the screen), each with a reviewed mark.
 */
export function GuideView({ sel, status, revision, viewed, toggleViewed, onOpen }: Props) {
  const { suggestEnabled, suggestCommand } = useSettings();
  const program = programOf(suggestCommand);
  const branch = status?.branch ?? null;
  const root = status?.root ?? "";
  const id = guideId(root, sel, branch);
  const { saved, run } = useGuide(id);
  const guide = useMemo(() => (saved ? parseGuide(saved.text) : null), [saved]);

  // The files as Open All reads them: the commit's, or the range the guide read.
  const base = saved?.base;
  const head = saved?.head;
  const changes = useMemo<ChangesSelection>(
    () =>
      !base || !head
        ? NO_FILES
        : sel.of === "commit"
          ? { kind: "changes", list: "commit", commit: sel.commit }
          : { kind: "changes", list: "range", range: { base, head, label: `${sel.label}...${head.slice(0, 7)}` } },
    [sel, base, head],
  );
  const fixed = useFixedFiles(changes);
  // Each file as the tabs it opens have it, and as viewed marks key it.
  const byPath = useMemo(() => {
    const files = (fixed.files ?? []).flatMap<ListFile>((file) =>
      changes.list === "commit" ? [{ kind: "commit", commit: changes.commit, file }] : changes.list === "range" ? [{ kind: "pr-file", range: changes.range, file }] : [],
    );
    return new Map(files.map((f) => [f.file.path, f]));
  }, [fixed.files, changes]);
  const placed = useMemo(() => (guide && fixed.files ? placeFiles(guide, [...byPath.keys()]) : null), [guide, fixed.files, byPath]);
  const titles = useMemo(() => guide?.sections.map((s) => s.title) ?? [], [guide]);

  const page = useRef<HTMLDivElement>(null);
  // A file's memo is Open All's, so it opens as far, and as tall, as it was there.
  const memo = (f: ListFile) => fileMemo(`${root}\0${selectionKey(f)}`);
  const [, redraw] = useReducer((n: number) => n + 1, 0);
  const isOpen = (f: ListFile) => !(memo(f).shut ?? viewed(f));
  const markViewed = (f: ListFile) => {
    delete memo(f).shut;
    toggleViewed(f);
    // Closing the file being read would leave the view somewhere in the next one: keep its header in view.
    const el = page.current?.querySelector(`[data-file="${CSS.escape(selectionKey(f))}"]`);
    requestAnimationFrame(() => {
      const top = page.current?.getBoundingClientRect().top;
      if (el && top !== undefined && el.getBoundingClientRect().top < top) el.scrollIntoView({ block: "start" });
    });
  };
  const goTo = (n: number) => {
    page.current?.querySelector(`[data-section="${n}"]`)?.scrollIntoView({ block: "start" });
    document.getElementById(`${PREFIX}section-${n}`)?.focus({ preventScroll: true });
  };
  // A PR's or comparison's files take no notes, as in Open All; a commit's are marked as on it.
  const notes = sel.of === "commit" ? { at: `commit ${sel.commit.shortSha}` } : null;
  const diffs = (paths: string[]) =>
    paths.map((path) => {
      const f = byPath.get(path)!;
      const open = isOpen(f);
      return (
        <FileBlock
          key={path}
          sel={f}
          memo={memo(f)}
          revision={fileRevision(f, memo(f), status, revision)}
          notes={notes}
          open={open}
          viewed={viewed(f)}
          onToggleOpen={() => {
            memo(f).shut = open;
            redraw();
          }}
          onToggleViewed={() => markViewed(f)}
          onOpen={() => onOpen(f)}
        />
      );
    });

  const running = run === "running";
  const generate = () => void generateGuide(id, sel);
  // status.head is HEAD's short id.
  const moved = sel.of === "branch" && saved && status?.head && !saved.head.startsWith(status.head);
  const done = guide && saved ? guide.sections.filter((_, i) => saved.done.includes(i)).length : 0;
  const filesViewed = [...byPath.values()].filter(viewed).length;
  const drawn = !!guide && (guide.models.length > 0 || guide.flows.length > 0 || !!guide.diagram);

  return (
    // Focusable so the keyboard can scroll it (focusPanel("code") lands here); diffs load as they near its view.
    <div ref={page} data-code-scroll tabIndex={0} className="@container h-full overflow-auto outline-none">
      <PageFind />
      <div className="mx-auto max-w-[1440px] px-6 py-5">
        <header className="mb-5">
          <div className="flex items-start gap-3">
            <h1 className="min-w-0 flex-1 text-[20px] leading-snug font-semibold select-text">{guide?.title || (sel.of === "commit" ? sel.commit.subject : `${branch ?? "HEAD"} since ${sel.label}`)}</h1>
            {running ? (
              <Button size="sm" variant="secondary" onClick={cancelGuide}>
                <Square /> Cancel
              </Button>
            ) : (
              <Button size="sm" variant="secondary" disabled={!suggestEnabled} onClick={generate}>
                {saved ? <RotateCw /> : <Sparkles />} {saved ? "Regenerate" : `Ask ${program}`}
              </Button>
            )}
          </div>
          <div className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-[12px] text-muted-foreground">
            <Sparkles className="size-3.5 shrink-0 text-subtle" />
            {sel.of === "commit" ? (
              <span>
                <span className="font-mono">{sel.commit.shortSha}</span> by {sel.commit.authorName}, {relativeTime(sel.commit.timestamp)}
              </span>
            ) : (
              <span>
                <span className="font-mono">{branch ?? "HEAD"}</span> since <span className="font-mono">{sel.label}</span>
                {saved && (
                  <>
                    {" "}
                    at <span className="font-mono">{saved.head.slice(0, 7)}</span>
                  </>
                )}
              </span>
            )}
            {saved && (
              <span>
                · written by {saved.program}, {relativeTime(saved.at / 1000)}
              </span>
            )}
            {guide && guide.sections.length > 0 && (
              <span>
                · <Count n={done} of={guide.sections.length} /> sections reviewed
              </span>
            )}
            {byPath.size > 0 && (
              <span>
                · <Count n={filesViewed} of={byPath.size} /> files viewed
              </span>
            )}
          </div>
        </header>

        {running && (
          <Notice role="status" icon={<LoaderCircle className="animate-spin" />}>
            Asking {program} to explain {sel.of === "commit" ? "this commit" : "this branch"}. It can take a few minutes, and it goes on while you look at other tabs.
          </Notice>
        )}
        {run === "stopped" && <Notice icon={<Square />}>Stopped before {program} was done{saved ? ": this is the guided review from before." : "."}</Notice>}
        {typeof run === "object" && run && (
          <Notice role="alert" icon={<TriangleAlert />} className="text-destructive">
            <span className="whitespace-pre-wrap">Couldn't write a guided review: {run.error}</span>
          </Notice>
        )}
        {!suggestEnabled && !running && (
          <Notice icon={<TriangleAlert />}>
            Suggestions are off.{" "}
            <button className="text-primary hover:underline" onClick={() => openSettings("commit")}>
              Turn them on in Settings → Commit Messages
            </button>{" "}
            to ask your agent CLI for one.
          </Notice>
        )}
        {moved && !running && (
          <Notice icon={<TriangleAlert />} className="text-modified">
            Outdated: {branch ?? "HEAD"} has moved since this review read it at <span className="font-mono">{saved.head.slice(0, 7)}</span>. Regenerate to review where it is now.
          </Notice>
        )}
        {fixed.error && (
          <Notice role="alert" icon={<TriangleAlert />} className="text-destructive">
            Couldn't read the files: {fixed.error}
          </Notice>
        )}

        {!saved ? (
          !running && (
            <p className="text-[13px] text-muted-foreground">
              {program} reads {sel.of === "commit" ? "the commit's message and diff" : "the branch's commits and their diff (not uncommitted changes)"} and explains it as sections in the order to review them, with a diagram of what changed.
            </p>
          )
        ) : !guide ? (
          <>
            <Notice icon={<TriangleAlert />}>The answer wasn't in the shape asked for; here it is as written.</Notice>
            <div className="markdown select-text">
              <MarkdownBody text={saved.text} components={components} idPrefix={PREFIX} />
            </div>
          </>
        ) : (
          <>
            <div className={cn("mb-6 grid gap-5", drawn && "@4xl:grid-cols-[minmax(0,2fr)_minmax(0,3fr)]")}>
              {guide.summary && (
                <div className="min-w-0">
                  <h2 className="mb-2 text-[13px] font-semibold">Overview</h2>
                  <div className="markdown select-text">
                    <MarkdownBody text={guide.summary} components={components} idPrefix={PREFIX} />
                  </div>
                </div>
              )}
              {drawn && (
                <div className="min-w-0 rounded-lg border border-border p-3">
                  <h2 className="mb-3 text-[13px] font-semibold">Before / after</h2>
                  <GuideDiagram models={guide.models} flows={guide.flows} titles={titles} onSection={goTo} />
                  {guide.diagram && (
                    // A guide from before structured diagrams.
                    <div className="markdown">
                      <Mermaid
                        code={guide.diagram}
                        fallback={
                          <pre>
                            <code>{guide.diagram}</code>
                          </pre>
                        }
                      />
                    </div>
                  )}
                </div>
              )}
            </div>
            {guide.sections.map((s, i) => (
              <SectionRow key={i} n={i + 1} total={guide.sections.length} section={s} done={saved.done.includes(i)} onDone={(on) => markDone(id, i, on)} files={placed && diffs(placed.shown[i])}>
                {placed?.named[i].map(({ path, at }) => (
                  <div key={path} className="flex min-w-0 items-center gap-1.5 text-[12px]">
                    <FileIcon path={path} />
                    <span className="min-w-0 truncate font-mono text-subtle" title={path}>
                      {path}
                    </span>
                    {at ? (
                      <button className="shrink-0 text-primary hover:underline" onClick={() => goTo(at)}>
                        in section {sectionBadge(at)}
                      </button>
                    ) : (
                      <span className="shrink-0 text-subtle">not in this diff</span>
                    )}
                  </div>
                ))}
              </SectionRow>
            ))}
            {!!placed?.rest.length && <SectionRow section={{ title: "Other changes", summary: "In no section of the guide.", files: [], risk: "" }} files={diffs(placed.rest)} />}
          </>
        )}
      </div>
    </div>
  );
}

function Count({ n, of }: { n: number; of: number }) {
  return (
    <>
      <span className={cn("font-semibold", n === of ? "text-added" : "text-foreground")}>{n}</span> / {of}
    </>
  );
}

function Notice({ icon, role, className, children }: { icon: ReactNode; role?: "status" | "alert"; className?: string; children: ReactNode }) {
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
function SectionRow({
  n,
  total,
  section: s,
  done = false,
  onDone,
  files,
  children,
}: {
  n?: number;
  total?: number;
  section: GuideSection;
  done?: boolean;
  onDone?: (on: boolean) => void;
  /** Null while the files load. */
  files: ReactNode[] | null;
  children?: ReactNode;
}) {
  const heading = `${PREFIX}section-${n ?? "other"}`;
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
            {s.summary && (
              <div className="markdown select-text">
                <MarkdownBody text={s.summary} components={components} idPrefix={`${PREFIX}${n ?? "other"}-`} />
              </div>
            )}
            {s.risk && (
              <Notice icon={<TriangleAlert />} className="text-modified">
                {s.risk}
              </Notice>
            )}
            {children}
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
