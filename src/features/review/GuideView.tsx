import { LoaderCircle, RotateCw, Sparkles, Square, TriangleAlert } from "lucide-react";
import { useMemo, useState } from "react";
import { flushSync } from "react-dom";
import { PageFind } from "@/components/FindBox";
import { Button } from "@/components/ui/button";
import type { RepoStatus } from "@/lib/api";
import { relativeTime } from "@/lib/format";
import { programOf, reviewAgent } from "@/lib/git/suggest";
import type { ChangesSelection, GuideSelection, Selection } from "@/lib/repo/selection";
import { type Category, type GuideSection, matchPath, parseGuide, placeFiles } from "@/lib/review/guide";
import { useSettings } from "@/lib/settings";
import { cn } from "@/lib/utils";
import type { BranchChange } from "@/features/changes/BranchReview";
import { Mermaid } from "@/features/viewer/markdown/Mermaid";
import { type Annotations, type ListFile, useStackedFiles } from "@/features/viewer/StackedFiles";
import { useFixedFiles } from "@/features/viewer/fixedFiles";
import { openSettings } from "@/features/settings/SettingsDialog";
import { cancelGuide, generateGuide, guideId, markDone, useGuide } from "./guides";
import { GuideDiagram } from "./GuideDiagram";
import { CATEGORY_UI } from "./categories";
import { CategoryFilter, GuideNav } from "./GuideNav";
import { GuideMarkdown, Notice, SectionRow } from "./GuideSection";

// No files to read before a branch's guide says which range it read: a list useFixedFiles leaves be.
const NO_FILES: ChangesSelection = { kind: "changes", list: "unstaged" };
const OTHER: GuideSection = { title: "Other changes", category: "other", summary: "In no section of the guide.", files: [], check: "", risk: "", importance: "medium", fileNotes: [], lineNotes: [] };
// The category each guide shows, and whether only its high importance sections, by guide id, until GitViber quits.
interface Filter {
  category: Category | null;
  high: boolean;
}
const ALL: Filter = { category: null, high: false };
const filters = new Map<string, Filter>();

interface Props {
  sel: GuideSelection;
  status: RepoStatus | null;
  /** The branch review's files as last loaded; null while there's none. */
  branchRows: BranchChange[] | null;
  revision: number;
  viewed: (sel: Selection) => boolean;
  toggleViewed: (sel: Selection) => void;
  onOpen: (s: Selection, pin?: boolean) => void;
}

/**
 * A commit or branch explained by the user's agent CLI, kept once written: an overview and a
 * diagram of the changed parts, then a row a section in reading order, its prose beside its files'
 * diffs (a stacked diff, drawn only near the screen), each with a reviewed mark.
 */
export function GuideView({ sel, status, branchRows, revision, viewed, toggleViewed, onOpen }: Props) {
  const settings = useSettings();
  const { suggestEnabled } = settings;
  const program = programOf(reviewAgent(settings).command);
  const branch = status?.branch ?? null;
  const root = status?.root ?? "";
  const id = guideId(root, sel, branch);
  const { saved, run } = useGuide(id);
  const guide = useMemo(() => (saved ? parseGuide(saved.text) : null), [saved]);
  // status.head is HEAD's short id.
  const moved = sel.of === "branch" && saved && status?.head && !saved.head.startsWith(status.head);

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
  // The agent's notes, by the changed file they're on.
  const annotations = useMemo(() => {
    const at = new Map<string, Annotations>();
    const all = new Set(byPath.keys());
    const of = (p: string) => {
      const path = matchPath(p, all);
      if (path && !at.has(path)) at.set(path, { file: [], lines: [] });
      return path ? at.get(path)! : null;
    };
    for (const s of guide?.sections ?? []) {
      for (const n of s.fileNotes) of(n.path)?.file.push({ text: n.text, critical: n.critical });
      for (const n of s.lineNotes) of(n.path)?.lines.push({ old: n.side === "old", line: n.line, text: n.text, critical: n.critical });
    }
    return at;
  }, [guide, byPath]);
  const counts = useMemo(() => {
    const at = new Map<Category, number>();
    for (const s of guide?.sections ?? []) at.set(s.category, (at.get(s.category) ?? 0) + 1);
    return at;
  }, [guide]);
  const highs = useMemo(() => guide?.sections.filter((s) => s.importance === "high").length ?? 0, [guide]);
  const [picked, setPicked] = useState(() => filters.get(id) ?? ALL);
  // One the regenerated guide has none of shows them all.
  const filter = picked.category && counts.has(picked.category) ? picked.category : null;
  const highOnly = picked.high && highs > 0;
  const filtered = !!filter || highOnly;
  const fits = (s: GuideSection) => (!filter || s.category === filter) && (!highOnly || s.importance === "high");
  const setFilter = (f: Filter) => {
    if (f.category || f.high) filters.set(id, f);
    else filters.delete(id);
    setPicked(f);
  };

  // Branch Review's own row for a file, while it reads the same commits and the file has no
  // uncommitted change: the same diff, so one viewed mark for both. Other files keep this guide's.
  const shared = useMemo(() => {
    if (sel.of !== "branch" || !saved || !status?.head || moved) return new Map<string, BranchChange>();
    const dirty = new Set([...status.staged, ...status.unstaged].map((f) => f.path));
    return new Map((branchRows ?? []).filter((r) => r.base === saved.base && !dirty.has(r.file.path)).map((r) => [r.file.path, r]));
  }, [sel.of, saved, status, moved, branchRows]);
  const markOf = (f: ListFile) => shared.get(f.file.path) ?? f;
  const isViewed = (f: ListFile) => viewed(markOf(f));
  const { scroller, block } = useStackedFiles({
    root,
    status,
    revision,
    viewed: isViewed,
    toggleViewed: (f) => toggleViewed(markOf(f)),
    // A range's files take no notes, as in Open All; a commit's are marked as on it.
    notes: sel.of === "commit" && byPath.size ? { at: `commit ${sel.commit.shortSha}` } : null,
    onOpen,
  });
  // Its diffs; none to wait for once the files couldn't be read.
  const diffs = (paths: string[] | undefined) => (paths ? paths.map((p) => block(byPath.get(p)!, annotations.get(p))) : fixed.error ? [] : null);
  const goTo = (n: number) => {
    // A section the filter hides (a diagram's link, Other changes): show them all first.
    const s = guide?.sections[n - 1];
    if (filtered && (!s || !fits(s))) flushSync(() => setFilter(ALL));
    const row = scroller.current?.querySelector(`[data-section="${n}"]`);
    row?.scrollIntoView({ block: "start" });
    row?.querySelector<HTMLElement>("h2")?.focus({ preventScroll: true });
  };

  const running = run === "running";
  const generate = () => void generateGuide(id, sel);
  const done = guide && saved ? guide.sections.filter((_, i) => saved.done.includes(i)).length : 0;
  const filesViewed = [...byPath.values()].filter(isViewed).length;
  const drawn = !!guide && (guide.models.length > 0 || guide.flows.length > 0 || !!guide.diagram);
  const shown = (guide?.sections ?? []).flatMap((section, i) => (fits(section) ? [{ n: i + 1, section, done: !!saved?.done.includes(i) }] : []));
  const nav = shown.length > 1;

  return (
    // Focusable so the keyboard can scroll it (focusPanel("code") lands here); diffs load as they near its view.
    <div ref={scroller} data-code-scroll tabIndex={0} className="@container relative h-full overflow-auto outline-none">
      <PageFind />
      {/* --stick: the narrow navigator's height, which the files' sticky headers keep below. */}
      <div className={cn("mx-auto max-w-[1440px] px-6 py-5", nav && "[--stick:36px] @6xl:[--stick:0px]")}>
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
                · written by {saved.program}
                {saved.details?.length ? ` (${saved.details.join(", ")})` : ""}, {relativeTime(saved.at / 1000)}
              </span>
            )}
            {guide && guide.sections.length > 0 && (
              <span>
                · <Count n={done} of={guide.sections.length} /> sections reviewed
              </span>
            )}
            {byPath.size > 0 && (
              <span title={sel.of === "branch" ? "Shared with Branch Review while it reads these same commits and the file has no uncommitted change; otherwise this review's own." : undefined}>
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
              {program} reads {sel.of === "commit" ? "the commit's message and diff" : "the branch's commits and their diff (not uncommitted changes)"} and explains it as sections in the order to review them, by category, with a diagram when a flow or data model changes.
            </p>
          )
        ) : !guide ? (
          <>
            <Notice icon={<TriangleAlert />}>The answer wasn't in the shape asked for; here it is as written.</Notice>
            <GuideMarkdown text={saved.text} />
          </>
        ) : (
          <div className={cn(nav && "@6xl:grid @6xl:grid-cols-[220px_minmax(0,1fr)] @6xl:gap-6")}>
            {nav && <GuideNav items={shown} onGo={goTo} />}
            <div className="min-w-0">
              <div className={cn("mb-6 grid gap-5", nav && "mt-4 @6xl:mt-0", drawn && "@4xl:grid-cols-[minmax(0,2fr)_minmax(0,3fr)]")}>
                {guide.summary && (
                  <div className="min-w-0">
                    <h2 className="mb-2 text-[13px] font-semibold">Overview</h2>
                    <GuideMarkdown text={guide.summary} />
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
              {(counts.size > 1 || (highs > 0 && highs < guide.sections.length)) && (
                <CategoryFilter
                  counts={counts}
                  total={guide.sections.length}
                  value={filter}
                  onChange={(category) => setFilter({ ...picked, category })}
                  high={highs > 0 && highs < guide.sections.length ? { count: highs, on: highOnly, onChange: (high) => setFilter({ ...picked, high }) } : null}
                />
              )}
              {!!placed?.rest.length && !filtered && (
                <Notice icon={<TriangleAlert />}>
                  {placed.rest.length === 1 ? "1 file wasn't" : `${placed.rest.length} files weren't`} placed in a section by {saved.program}.{" "}
                  <button className="text-primary hover:underline" onClick={() => goTo(0)}>
                    See them under Other changes
                  </button>
                </Notice>
              )}
              {shown.map(({ n, section, done }) => (
                <SectionRow
                  key={n}
                  n={n}
                  total={guide.sections.length}
                  section={section}
                  done={done}
                  onDone={(on) => markDone(id, n - 1, on)}
                  files={diffs(placed?.shown[n - 1])}
                  named={placed?.named[n - 1]}
                  onSection={goTo}
                />
              ))}
              {filtered && !shown.length && (
                <p className="text-[13px] text-muted-foreground">
                  No {filter && CATEGORY_UI[filter].label} section is high importance.{" "}
                  <button className="text-primary hover:underline" onClick={() => setFilter(ALL)}>
                    Show all
                  </button>
                </p>
              )}
              {!!placed?.rest.length && !filtered && <SectionRow n={0} section={OTHER} files={diffs(placed.rest)} />}
            </div>
          </div>
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
