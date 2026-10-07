import { LoaderCircle, RotateCw, Sparkles, Square, TriangleAlert } from "lucide-react";
import { type ReactNode, useMemo } from "react";
import type { Components } from "react-markdown";
import { Button } from "@/components/ui/button";
import { FileIcon } from "@/components/FileIcon";
import { LineCounts } from "@/components/StatusBadge";
import { useAsyncValue } from "@/hooks/useAsyncValue";
import { api, type FileChange, type RepoStatus } from "@/lib/api";
import { relativeTime } from "@/lib/format";
import { programOf } from "@/lib/git/suggest";
import { markdownLink } from "@/lib/github/markdown";
import type { GuideSelection, Selection } from "@/lib/repo/selection";
import { type GuideSection, matchPath, parseGuide, unplaced } from "@/lib/review/guide";
import { useSettings } from "@/lib/settings";
import { cn } from "@/lib/utils";
import { followLink, MarkdownBody, MarkdownPage } from "@/features/viewer/MarkdownView";
import { Mermaid } from "@/features/viewer/markdown/Mermaid";
import { openSettings } from "@/features/settings/SettingsDialog";
import { cancelGuide, generateGuide, guideId, markDone, useGuide } from "./guides";

const PREFIX = "guide-";
const components: Components = { a: markdownLink((href) => followLink(href, () => {}, PREFIX)) };

interface Props {
  sel: GuideSelection;
  status: RepoStatus | null;
  onOpen: (s: Selection, pin?: boolean) => void;
}

/**
 * A commit or branch explained by the user's agent CLI, kept once written: a summary, a diagram
 * of the changed parts, then sections in reading order, each with its files and a done mark.
 */
export function GuideView({ sel, status, onOpen }: Props) {
  const { suggestEnabled, suggestCommand } = useSettings();
  const program = programOf(suggestCommand);
  const branch = status?.branch ?? null;
  const id = guideId(status?.root ?? "", sel, branch);
  const { saved, run } = useGuide(id);
  const guide = useMemo(() => (saved ? parseGuide(saved.text) : null), [saved]);
  const files = useAsyncValue(
    saved ? () => (sel.of === "commit" ? api.commitFiles(sel.commit.sha) : api.rangeFiles(saved.base, saved.head)) : null,
    [saved?.base, saved?.head],
    [] as FileChange[],
  );
  const byPath = useMemo(() => new Map(files.map((f) => [f.path, f])), [files]);
  const open =
    saved &&
    ((file: FileChange) =>
      onOpen(sel.of === "commit" ? { kind: "commit", commit: sel.commit, file } : { kind: "pr-file", range: { base: saved.base, head: saved.head, label: `${sel.label}...${saved.head.slice(0, 7)}` }, file }));

  const running = run === "running";
  const generate = () => void generateGuide(id, sel);
  // status.head is HEAD's short id.
  const moved = sel.of === "branch" && saved && status?.head && !saved.head.startsWith(status.head);
  const done = guide && saved ? guide.sections.filter((_, i) => saved.done.includes(i)).length : 0;
  const rest = guide && files.length ? unplaced(guide, files.map((f) => f.path)) : [];

  return (
    <MarkdownPage>
      <div className="mb-5 flex flex-wrap items-center gap-x-2 gap-y-1 text-[12px] text-muted-foreground">
        <Sparkles className="size-3.5 shrink-0 text-subtle" />
        {sel.of === "commit" ? (
          <span className="min-w-0 truncate">
            <span className="font-mono">{sel.commit.shortSha}</span> {sel.commit.subject}
          </span>
        ) : (
          <span>
            <span className="font-mono">{branch ?? "HEAD"}</span> since <span className="font-mono">{sel.label}</span>
          </span>
        )}
        {saved && (
          <span>
            · {saved.program}, {relativeTime(saved.at / 1000)}
          </span>
        )}
        {guide && guide.sections.length > 0 && (
          <span>
            · {done} of {guide.sections.length} sections done
          </span>
        )}
        <span className="ml-auto">
          {running ? (
            <Button size="sm" variant="secondary" onClick={cancelGuide}>
              <Square /> Cancel
            </Button>
          ) : (
            <Button size="sm" variant="secondary" disabled={!suggestEnabled} onClick={generate}>
              {saved ? <RotateCw /> : <Sparkles />} {saved ? "Regenerate" : `Ask ${program}`}
            </Button>
          )}
        </span>
      </div>

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

      {!saved ? (
        !running && (
          <p className="text-muted-foreground">
            {program} reads {sel.of === "commit" ? "the commit's message and diff" : "the branch's commits and their diff (not uncommitted changes)"} and explains it as sections in the order to review them, with a diagram of what changed.
          </p>
        )
      ) : !guide ? (
        <>
          <Notice icon={<TriangleAlert />}>The answer wasn't in the shape asked for; here it is as written.</Notice>
          <MarkdownBody text={saved.text} components={components} idPrefix={PREFIX} />
        </>
      ) : (
        <>
          {guide.title && <h1>{guide.title}</h1>}
          {guide.summary && <MarkdownBody text={guide.summary} components={components} idPrefix={PREFIX} />}
          {guide.diagram && (
            <Mermaid
              code={guide.diagram}
              fallback={
                <pre>
                  <code>{guide.diagram}</code>
                </pre>
              }
            />
          )}
          {guide.sections.map((s, i) => (
            <Section key={i} n={i + 1} section={s} done={saved.done.includes(i)} onDone={(on) => markDone(id, i, on)}>
              <FileList paths={s.files} byPath={byPath} open={open} />
            </Section>
          ))}
          {rest.length > 0 && (
            <>
              <h2>In no section</h2>
              <FileList paths={rest} byPath={byPath} open={open} />
            </>
          )}
        </>
      )}
    </MarkdownPage>
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

/** One part of the change; marked done, it folds to its title. */
function Section({ n, section: s, done, onDone, children }: { n: number; section: GuideSection; done: boolean; onDone: (on: boolean) => void; children: ReactNode }) {
  return (
    <section className={cn(done && "opacity-60")}>
      <div className="flex items-baseline gap-2">
        {/* Named by the section's title, outside the heading, so the heading reads as a title alone. */}
        <input type="checkbox" checked={done} onChange={(e) => onDone(e.target.checked)} aria-labelledby={`${PREFIX}section-${n}`} />
        <h2 id={`${PREFIX}section-${n}`} className="min-w-0 flex-1">
          {n}. {s.title || "Untitled"}
        </h2>
      </div>
      {!done && (
        <>
          {s.summary && <MarkdownBody text={s.summary} components={components} idPrefix={`${PREFIX}${n}-`} />}
          {s.risk && (
            <Notice icon={<TriangleAlert />} className="text-modified">
              {s.risk}
            </Notice>
          )}
          {children}
        </>
      )}
    </section>
  );
}

/** Each path opens its diff; one the diff doesn't have (the model misnamed it) shows as text. */
function FileList({ paths, byPath, open }: { paths: string[]; byPath: Map<string, FileChange>; open: ((f: FileChange) => void) | null }) {
  // As the diff names them: a model's `b/src/x.ts` and `src/x.ts` are one file.
  const known = new Set(byPath.keys());
  const shown = [...new Set(paths.map((p) => matchPath(p, known) ?? p))];
  if (!shown.length) return null;
  return (
    <div className="mb-4 flex flex-col gap-0.5 text-[12.5px]">
      {shown.map((p) => {
        const file = byPath.get(p);
        return (
          <div key={p} className="flex min-w-0 items-center gap-1.5">
            <FileIcon path={p} />
            {file && open ? (
              <button className="min-w-0 truncate font-mono text-primary hover:underline" title={`Open the diff of ${p}`} onClick={() => open(file)}>
                {p}
              </button>
            ) : (
              <span className="min-w-0 truncate font-mono text-subtle" title="Not in this diff">
                {p}
              </span>
            )}
            {file && <LineCounts file={file} />}
          </div>
        );
      })}
    </div>
  );
}
