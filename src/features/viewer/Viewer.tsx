import { ArrowDown, ArrowUp, Check, Columns2, Contrast, Copy, Eye, FileCode2, FoldVertical, GitCompareArrows, Rows2, Space, UserSearch } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Segmented } from "@/components/ui/segmented";
import { Tip } from "@/components/ui/tooltip";
import { api, type DiffPair, errorMessage, type FileChange, type RepoStatus } from "@/lib/api";
import { withNetActivity } from "@/lib/repo/netActivity";
import { onReveal, revealWaits } from "@/lib/editor/reveal";
import { type Selection, selectionPath } from "@/lib/repo/selection";
import { bindingsFor, type CommandId, formatChord, useCommands, useShortcut } from "@/lib/commands/keybindings";
import { diffWhitespace, updateSettings, useSettings } from "@/lib/settings";
import { FIT, type Zoom } from "@/lib/ui/svg";
import { toast } from "@/lib/app/toast";
import { cn } from "@/lib/utils";
import { copyText } from "@/lib/app/clipboard";
import { type CodeViewHandle, MonacoView } from "./MonacoView";
import { ConflictView } from "./ConflictView";
import { IssueView } from "@/features/github/issues/IssueView";
import { PullView } from "@/features/github/pulls/PullView";
import { FileIcon } from "@/components/FileIcon";
import { useReview } from "@/features/github/pulls/ReviewThreads";
import { isSvg, MediaView, mediaKind, SvgView } from "./MediaView";
import { isMarkdown, MarkdownView } from "./MarkdownView";
import { LineCounts, PathLabel, StatusPill } from "@/components/StatusBadge";
import { type FileSelection, linkSides, pairArgs, useBlame, usePair } from "./diffPairs";
import type { Tab } from "./tabs";
import { TabStrip } from "./TabStrip";
import { CommitBar } from "./CommitBar";
import { ErrorBoundary } from "@/components/ErrorBoundary";

interface ViewerProps {
  tabs: Tab[];
  active: Tab | null;
  status: RepoStatus | null;
  revision: number;
  viewed: (sel: Selection) => boolean;
  toggleViewed: (sel: Selection) => void;
  onActivate: (key: string) => void;
  onClose: (key: string) => void;
  onCloseTabs: (keys: string[]) => void;
  onPin: (key: string) => void;
  onMoveTab: (from: number, to: number) => void;
  onOpen: (s: Selection) => void;
  /** History, filtered to a file's commits. */
  onShowHistory: (path: string) => void;
  /** Blame's link: a commit in History, with `path` (its name in that commit) open. */
  onShowCommit: (sha: string, path: string) => void;
  /** Reads the repo's status again, after staging lines here. */
  refresh: () => unknown;
}

export function Viewer(props: ViewerProps) {
  const { tabs, active } = props;
  return (
    <div className="flex h-full min-w-0 flex-col bg-background">
      <TabStrip {...props} />
      {active ? (
        // A render error in one file's view stays in that tab instead of blanking the app.
        <ErrorBoundary key={active.key} fallback={(e) => <Placeholder title="This view crashed" detail={errorMessage(e)} />}>
          {active.sel.kind === "conflict" ? (
            <ConflictView file={active.sel.file} operation={props.status?.operation ?? null} revision={props.revision} />
          ) : active.sel.kind === "pull" ? (
            <PullView pull={active.sel.pull} onOpen={props.onOpen} />
          ) : active.sel.kind === "issue" ? (
            <IssueView issue={active.sel.issue} onDeleted={() => props.onClose(active.key)} />
          ) : (
            <Pane tab={active} sel={active.sel} {...props} />
          )}
        </ErrorBoundary>
      ) : (
        <EmptyViewer hasTabs={tabs.length > 0} />
      )}
    </div>
  );
}

// ---------------------------------------------------------------- pane

function findChange(status: RepoStatus | null, path: string): Selection | null {
  if (!status) return null;
  const unstaged = status.unstaged.find((f) => f.path === path);
  if (unstaged) return { kind: "unstaged", file: unstaged };
  const staged = status.staged.find((f) => f.path === path);
  return staged ? { kind: "staged", file: staged } : null;
}

function Pane({ tab, sel, status, revision, viewed, toggleViewed, onOpen, onShowCommit, refresh }: ViewerProps & { tab: Tab; sel: FileSelection }) {
  const s = useSettings();
  const { pair, error } = usePair(sel, revision, diffWhitespace(s));
  const range = sel.kind === "pr-file" ? sel.range : null;
  const review = useReview(range?.pullUrl, range?.number, range?.head ?? "", selectionPath(sel));
  const view = useRef<CodeViewHandle>(null);
  const isFile = sel.kind === "file";
  const file: FileChange | null = isFile ? null : sel.file;
  const change = isFile ? findChange(status, sel.path) : null;
  const media = mediaKind(selectionPath(sel)) !== null;
  const markdown = isMarkdown(selectionPath(sel));
  const svg = isSvg(selectionPath(sel));
  // Reading a markdown file starts rendered (unless turned off); reviewing its changes starts on the
  // diff; a search result on its code, where the match shows.
  const [markdownPreview, setMarkdownPreview] = useState(isFile && s.markdownPreview && !revealWaits(selectionPath(sel)));
  // SVGs open however the last one was left, diffs included. A reveal shows this one's code
  // without changing that.
  const [svgCode, setSvgCode] = useState(false);
  const preview = svg ? s.svgPreview && !svgCode : markdownPreview;
  const setPreview = (on: boolean) => (svg ? (setSvgCode(false), updateSettings({ svgPreview: on })) : setMarkdownPreview(on));
  const rendered = (markdown || svg) && preview;
  const toCode = useRef(() => {});
  toCode.current = () => void (isFile && rendered && (svg ? setSvgCode(true) : setMarkdownPreview(false)));
  useEffect(() => {
    const path = selectionPath(sel);
    if (revealWaits(path)) toCode.current();
    return onReveal((r) => r.path === path && toCode.current());
  }, [sel]);
  const [zoom, setZoom] = useState<Zoom>(FIT);
  const [contrast, setContrast] = useState(false);
  const special = pair && (media ? (isFile && !pair.modified.exists ? "This file no longer exists" : null) : placeholderFor(pair, isFile));

  const diff = !isFile && !media && !rendered;
  const note = diff && pair && !special ? (pair.eolOnly ? "Only line endings changed" : pair.whitespaceHidden ? "Whitespace changes hidden" : null) : null;
  const code = !media && !rendered && !!pair && !special;
  const blame = useBlame(isFile && code && s.blame ? sel.path : null, pair, status?.head ?? null);
  useCommands({
    "diff.nextChange": diff ? () => view.current?.next() : undefined,
    "diff.prevChange": diff ? () => view.current?.prev() : undefined,
    "diff.stageChange": code && sel.kind === "unstaged" ? () => view.current?.lineAction("stage") : undefined,
    "diff.unstageChange": code && sel.kind === "staged" ? () => view.current?.lineAction("unstage") : undefined,
    "diff.discardChange": code && sel.kind === "unstaged" ? () => view.current?.lineAction("discard") : undefined,
  });

  return (
    <>
      {sel.kind === "commit" && <CommitBar commit={sel.commit} url={sel.url} />}
      <div className="flex h-9 shrink-0 items-center gap-2 border-b border-border pr-2 pl-3">
        <FileIcon path={selectionPath(sel)} />
        <PathLabel path={selectionPath(sel)} className="min-w-0 text-[12px]" />
        <Tip label="Copy path">
          <button className="text-subtle hover:text-foreground focus-visible:text-foreground" onClick={() => copyText(selectionPath(sel), "Path copied")}>
            <Copy className="size-3" />
          </button>
        </Tip>
        {file?.oldPath && <span className="truncate text-[11.5px] text-subtle">← {file.oldPath}</span>}
        {file && <LineCounts file={file} />}
        {file && <StatusPill status={file.status} />}
        {note && (
          // Gives way first as the pane narrows (shrinks far faster than the file name), never the buttons.
          <Tip label={note}>
            <span className="ml-auto min-w-0 shrink-[1000] truncate text-[11.5px] text-subtle">{note}</span>
          </Tip>
        )}
        <div className={cn("flex shrink-0 items-center gap-1", !note && "ml-auto")}>
          {diff && (
            <>
              <IconBtn label="Previous change" command="diff.prevChange" onClick={() => view.current?.prev()}>
                <ArrowUp />
              </IconBtn>
              <IconBtn label="Next change" command="diff.nextChange" onClick={() => view.current?.next()}>
                <ArrowDown />
              </IconBtn>
              <Sep />
              <LayoutToggle />
              <IconBtn label="Collapse unchanged lines" command="diff.toggleCollapse" active={s.hideUnchanged} onClick={() => updateSettings({ hideUnchanged: !s.hideUnchanged })}>
                <FoldVertical />
              </IconBtn>
              <IconBtn
                label={s.whitespaceMode === "all" ? "Ignore all whitespace" : "Ignore whitespace changes"}
                command="diff.toggleWhitespace"
                active={s.ignoreWhitespace}
                onClick={() => updateSettings({ ignoreWhitespace: !s.ignoreWhitespace })}
              >
                <Space />
              </IconBtn>
              <Sep />
            </>
          )}
          {rendered && svg && (
            <>
              {!isFile && <LayoutToggle />}
              <Tip label="Scroll to zoom, drag to pan, double-click to fit">
                <div>
                  <Segmented
                    value={zoom.scale === null ? "fit" : zoom.scale === 1 ? "actual" : "custom"}
                    onChange={(v) => setZoom(v === "fit" ? FIT : { scale: 1, u: 0.5, v: 0.5 })}
                    options={[
                      { value: "fit", label: "Fit" },
                      { value: "actual", label: "1:1" },
                    ]}
                  />
                </div>
              </Tip>
              <Tip label={contrast ? "Theme background" : s.dark ? "Light background" : "Dark background"}>
                <Button variant="ghost" size="icon-sm" onClick={() => setContrast(!contrast)} className={cn(contrast && "bg-primary/15 text-primary hover:bg-primary/20 focus-visible:bg-primary/20 hover:text-primary focus-visible:text-primary")}>
                  <Contrast />
                </Button>
              </Tip>
              <Sep />
            </>
          )}
          {isFile && code && (
            <>
              <IconBtn label="Blame" command="editor.toggleBlame" active={s.blame} onClick={() => updateSettings({ blame: !s.blame })}>
                <UserSearch />
              </IconBtn>
              {blame?.unavailable && (
                <span className="truncate text-[11.5px] text-subtle" title={blame.unavailable}>
                  No blame for LFS files
                </span>
              )}
              <Sep />
            </>
          )}
          {isFile && change && (
            <Button variant="secondary" size="sm" onClick={() => onOpen(change)}>
              <GitCompareArrows /> Changes
            </Button>
          )}
          {!isFile && sel.kind !== "commit" && sel.kind !== "pr-file" && file?.status !== "D" && (
            <Button variant="secondary" size="sm" onClick={() => onOpen({ kind: "file", path: selectionPath(sel) })}>
              <FileCode2 /> Open file
            </Button>
          )}
          {(sel.kind === "unstaged" || sel.kind === "staged") && (
            <Tip label={sel.kind === "staged" ? "Unstage" : viewed(sel) ? "Mark as not viewed" : "Mark as viewed"}>
              <Button
                variant={viewed(sel) ? "default" : "secondary"}
                size="sm"
                onClick={() => toggleViewed(sel)}
                className={cn(viewed(sel) && "bg-added-fill text-on-status hover:bg-added-fill/85 focus-visible:bg-added-fill/85")}
              >
                <Check /> Viewed
              </Button>
            </Tip>
          )}
          {/* Last, so it stays put while the buttons before it change with the mode. */}
          {(markdown || svg) && (
            <div className="ml-1">
              <Segmented
                value={preview ? "preview" : "code"}
                onChange={(v) => setPreview(v === "preview")}
                options={[
                  { value: "code", label: isFile ? "Code" : "Diff", icon: isFile ? FileCode2 : GitCompareArrows },
                  { value: "preview", label: "Preview", icon: Eye },
                ]}
              />
            </div>
          )}
        </div>
      </div>
      <div className="relative min-h-0 flex-1">
        {error ? (
          <Placeholder title="Could not load" detail={error} />
        ) : special ? (
          <Placeholder
            title={special}
            action={pair && (pair.modified.lfsMissing || pair.original.lfsMissing) ? { label: "Download with Git LFS", run: () => downloadLfs(selectionPath(sel), refresh) } : undefined}
          />
        ) : rendered && svg ? (
          pair && (
            <SvgView
              before={!isFile && pair.original.exists ? pair.original.text : null}
              after={pair.modified.exists ? pair.modified.text : null}
              stacked={!isFile && !s.sideBySide}
              zoom={zoom}
              onZoom={setZoom}
              backdrop={contrast ? (s.dark ? "light" : "dark") : "theme"}
            />
          )
        ) : rendered ? (
          pair && <MarkdownView text={pair.modified.exists ? pair.modified.text : pair.original.text} src={pairArgs(sel, revision)} onOpen={onOpen} />
        ) : media ? (
          pair && <MediaView src={pairArgs(sel, revision)} before={!isFile && pair.original.exists} after={pair.modified.exists} />
        ) : (
          pair && (
            <MonacoView
              ref={view}
              pair={pair}
              path={selectionPath(sel)}
              mode={isFile ? "file" : s.sideBySide ? "split" : "unified"}
              collapse={s.hideUnchanged}
              wrap={s.wordWrap}
              scrollKey={tab.key}
              onDisk={sel.kind === "file" || sel.kind === "unstaged"}
              blame={blame?.unavailable ? null : blame}
              blameColumn={!!s.blame && isFile && !blame?.unavailable}
              onBlameClick={(c) => onShowCommit(c.sha, c.path)}
              links={linkSides(sel, revision)}
              staging={sel.kind === "unstaged" || sel.kind === "staged" ? { kind: sel.kind, refresh } : null}
              review={review}
            />
          )
        )}
      </div>
    </>
  );
}

function placeholderFor(pair: DiffPair, isFile: boolean) {
  const { original: a, modified: b } = pair;
  if (isFile && !b.exists) return "This file no longer exists";
  if (b.lfsMissing || a.lfsMissing) return b.lfsMissing ?? a.lfsMissing;
  if (a.binary || b.binary) return "Binary file";
  if (a.tooLarge || b.tooLarge) return "File is too large to display";
  if (!isFile && !pair.rows.some((r) => r.k !== 0)) return pair.whitespaceHidden ? "Only whitespace changed (hidden)" : "No textual changes";
  return null;
}

function EmptyViewer({ hasTabs }: { hasTabs: boolean }) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3 text-center">
      <GitCompareArrows className="size-8 text-border-strong" strokeWidth={1.5} />
      <div className="text-[12.5px] text-muted-foreground">{hasTabs ? "No tab selected" : "Select a file to review"}</div>
      <div className="grid grid-cols-[auto_auto] gap-x-4 gap-y-1.5 text-left text-[11.5px] text-subtle">
        <Kbd ids={["workbench.showCommands"]} /> <span>All commands</span>
        <Kbd ids={["workbench.quickOpen"]} /> <span>Open a file</span>
        <Kbd ids={["view.changes", "view.history", "view.pulls", "view.issues"]} /> <span>Changes · History · PRs · Issues</span>
        <Kbd ids={["view.toggleGitPanel", "view.toggleExplorer"]} /> <span>Toggle git panel · explorer</span>
        <Kbd ids={["review.nextFile", "review.prevFile"]} /> <span>Next / previous file</span>
        <Kbd ids={["diff.nextChange", "diff.prevChange"]} /> <span>Next / previous change</span>
        <Kbd ids={["diff.toggleSplit", "diff.toggleCollapse", "editor.toggleWrap"]} /> <span>Split · Collapse · Wrap</span>
        <Kbd ids={["review.toggleViewed"]} /> <span>Mark file viewed</span>
        <Kbd ids={["view.zoomIn", "view.zoomOut"]} /> <span>Zoom</span>
        <Kbd ids={["workbench.openSettings"]} /> <span>Settings & shortcuts</span>
      </div>
    </div>
  );
}

/** Shows the user's current bindings, not the defaults. */
function Kbd({ ids }: { ids: CommandId[] }) {
  const { keybindings } = useSettings();
  const k = ids
    .map((id) => bindingsFor(id, keybindings)[0])
    .filter(Boolean)
    .map(formatChord)
    .join(" ");
  return <span className="text-right font-mono text-muted-foreground">{k}</span>;
}

function Placeholder({ title, detail, action }: { title: string; detail?: string; action?: { label: string; run: () => void } }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-2 p-6 text-center">
      <div className="text-[12.5px] text-muted-foreground">{title}</div>
      {detail && <pre className="max-w-xl font-mono text-[11.5px] whitespace-pre-wrap text-subtle select-text">{detail}</pre>}
      {action && (
        <Button variant="secondary" size="sm" className="mt-1" onClick={action.run}>
          {action.label}
        </Button>
      )}
    </div>
  );
}

/** A Git LFS file's object, fetched as a network command the top bar shows. */
async function downloadLfs(path: string, refresh: () => unknown) {
  try {
    await withNetActivity("Download LFS file", (op) => api.lfsPull(path, op));
    toast("success", `Downloaded ${path}`);
  } catch (e) {
    toast("error", "Could not download the LFS file", errorMessage(e));
  }
  await refresh();
}

/** Unified / split for diffs; a before/after preview stacks or sits side by side to match. */
function LayoutToggle() {
  const s = useSettings();
  return (
    <Tip label="Unified / split" shortcut={useShortcut("diff.toggleSplit")}>
      <div>
        <Segmented
          value={s.sideBySide ? "split" : "unified"}
          onChange={(v) => updateSettings({ sideBySide: v === "split" })}
          options={[
            { value: "unified", label: "Unified", icon: Rows2 },
            { value: "split", label: "Split", icon: Columns2 },
          ]}
        />
      </div>
    </Tip>
  );
}

function IconBtn({ label, command, active, onClick, children }: { label: string; command: CommandId; active?: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <Tip label={label} shortcut={useShortcut(command)}>
      <Button variant="ghost" size="icon-sm" onClick={onClick} className={cn(active && "bg-primary/15 text-primary hover:bg-primary/20 focus-visible:bg-primary/20 hover:text-primary focus-visible:text-primary")}>
        {children}
      </Button>
    </Tip>
  );
}

function Sep() {
  return <div className="mx-0.5 h-4 w-px bg-border-strong" />;
}

