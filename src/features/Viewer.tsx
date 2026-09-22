import { ArrowDown, ArrowUp, Check, Columns2, Contrast, Copy, ExternalLink, Eye, FileCode2, FoldVertical, GitCommitHorizontal, GitCompareArrows, Rows2, X } from "lucide-react";
import { Component, type ReactNode, useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Tip } from "@/components/ui/tooltip";
import { api, type DiffKind, type DiffPair, errorMessage, type FileChange, type RepoStatus } from "@/lib/api";
import { type Selection, selectionPath } from "@/lib/selection";
import { bindingsFor, type CommandId, formatChord, useCommands, useShortcut } from "@/lib/keybindings";
import { updateSettings, useSettings } from "@/lib/settings";
import { FIT, type Zoom } from "@/lib/svg";
import { toast } from "@/lib/toast";
import { cn, relativeTime } from "@/lib/utils";
import { CodeView, type CodeViewHandle } from "./CodeView";
import { SortableList, useSortableItem } from "@/components/Sortable";
import { ConflictView } from "./ConflictView";
import { IssueStateIcon } from "./IssuesPanel";
import { IssueView } from "./IssueView";
import { CopyLinkButton, openOnGitHub, PullStateIcon } from "./PullsPanel";
import { PullView } from "./PullView";
import { prefetchHighlight } from "@/lib/highlight";
import { languageFor } from "@/lib/language";
import { FileIcon } from "./FileIcon";
import { isSvg, MediaView, mediaKind, SvgView } from "./MediaView";
import { isMarkdown, MarkdownView } from "./MarkdownView";
import { LineCounts, PathLabel, StatusPill } from "./StatusBadge";

export interface Tab {
  key: string;
  sel: Selection;
  preview: boolean;
}

interface ViewerProps {
  tabs: Tab[];
  active: Tab | null;
  status: RepoStatus | null;
  revision: number;
  viewed: (sel: Selection) => boolean;
  toggleViewed: (sel: Selection) => void;
  onActivate: (key: string) => void;
  onClose: (key: string) => void;
  onPin: (key: string) => void;
  onMoveTab: (from: number, to: number) => void;
  onOpen: (s: Selection) => void;
}

export function Viewer(props: ViewerProps) {
  const { tabs, active } = props;
  return (
    <div className="flex h-full min-w-0 flex-col bg-background">
      <TabStrip {...props} />
      {active ? (
        <PaneBoundary key={active.key}>
          {active.sel.kind === "conflict" ? (
            <ConflictView file={active.sel.file} operation={props.status?.operation ?? null} revision={props.revision} />
          ) : active.sel.kind === "pull" ? (
            <PullView pull={active.sel.pull} onOpen={props.onOpen} />
          ) : active.sel.kind === "issue" ? (
            <IssueView issue={active.sel.issue} onDeleted={() => props.onClose(active.key)} />
          ) : (
            <Pane tab={active} sel={active.sel} {...props} />
          )}
        </PaneBoundary>
      ) : (
        <EmptyViewer hasTabs={tabs.length > 0} />
      )}
    </div>
  );
}

/** A render error in one file's view stays in that tab instead of blanking the app. */
class PaneBoundary extends Component<{ children: ReactNode }, { error: string | null }> {
  state = { error: null as string | null };
  static getDerivedStateFromError(e: unknown) {
    return { error: errorMessage(e) };
  }
  render() {
    return this.state.error ? <Placeholder title="This view crashed" detail={this.state.error} /> : this.props.children;
  }
}

// ---------------------------------------------------------------- tabs

function tabLabel(sel: Selection) {
  if (sel.kind === "pull" || sel.kind === "issue") return selectionPath(sel);
  const path = selectionPath(sel);
  return path.slice(path.lastIndexOf("/") + 1);
}

function TabStrip({ tabs, active, onActivate, onClose, onPin, onMoveTab }: ViewerProps) {
  return (
    <div data-tauri-drag-region className="flex h-9 shrink-0 items-stretch overflow-x-auto overflow-y-hidden border-b border-border bg-panel [&::-webkit-scrollbar]:hidden">
      <SortableList ids={tabs.map((t) => t.key)} axis="x" onMove={onMoveTab}>
        {tabs.map((t) => (
          <TabItem key={t.key} tab={t} active={t.key === active?.key} onActivate={onActivate} onClose={onClose} onPin={onPin} />
        ))}
      </SortableList>
    </div>
  );
}

function TabItem({
  tab: t,
  active: isActive,
  onActivate,
  onClose,
  onPin,
}: {
  tab: Tab;
  active: boolean;
  onActivate: (key: string) => void;
  onClose: (key: string) => void;
  onPin: (key: string) => void;
}) {
  const { props, dragging, guard } = useSortableItem(t.key);
  return (
    <div
      {...props}
      role="tab"
      onClick={guard(() => onActivate(t.key))}
      onDoubleClick={() => onPin(t.key)}
      onAuxClick={(e) => e.button === 1 && onClose(t.key)}
      className={cn(
        "group relative flex max-w-56 shrink-0 cursor-pointer items-center gap-1.5 border-r border-border pr-1.5 pl-3 text-[12px] select-none",
        isActive ? "bg-background text-foreground" : "bg-panel text-muted-foreground hover:bg-hover hover:text-foreground",
        dragging && "cursor-grabbing bg-elevated text-foreground shadow-lg ring-1 shadow-black/50 ring-border-strong",
      )}
    >
      {isActive && <span className="absolute inset-x-0 top-0 h-px bg-primary" />}
      {isActive && !dragging && <span className="absolute inset-x-0 -bottom-px h-px bg-background" />}
      {t.sel.kind === "pull" ? (
        <PullStateIcon pull={t.sel.pull} />
      ) : t.sel.kind === "issue" ? (
        <IssueStateIcon issue={t.sel.issue} />
      ) : (
        <FileIcon path={selectionPath(t.sel)} />
      )}
      <span className={cn("truncate", t.preview && "italic")}>{tabLabel(t.sel)}</span>
      <TabKind sel={t.sel} />
      <button
        // Pressing the close button must not start a drag.
        onPointerDown={(e) => e.stopPropagation()}
        onClick={(e) => {
          e.stopPropagation();
          onClose(t.key);
        }}
        className={cn("flex size-5 items-center justify-center rounded-sm text-subtle hover:bg-active hover:text-foreground", !isActive && "opacity-0 group-hover:opacity-100")}
      >
        <X className="size-3" />
      </button>
    </div>
  );
}

function TabKind({ sel }: { sel: Selection }) {
  const labels: Partial<Record<Selection["kind"], string>> = { staged: "staged", unstaged: "diff", conflict: "conflict" };
  const label = sel.kind === "commit" ? sel.commit.shortSha : sel.kind === "pr-file" ? `#${sel.range.number}` : labels[sel.kind];
  return label ? <span className="shrink-0 font-mono text-[10px] text-subtle">{label}</span> : null;
}

// ---------------------------------------------------------------- pane

/** Tabs whose content is a diff of one file (everything except PR and issue overviews). */
export type FileSelection = Exclude<Selection, { kind: "pull" | "issue" }>;

function pairArgs(sel: FileSelection, revision: number) {
  const kind: DiffKind =
    sel.kind === "file" ? "worktree" : sel.kind === "conflict" ? "unstaged" : sel.kind === "pr-file" ? "range" : sel.kind;
  const path = selectionPath(sel);
  const oldPath = sel.kind === "file" ? null : sel.file.oldPath;
  const sha = sel.kind === "commit" ? sel.commit.sha : sel.kind === "pr-file" ? sel.range.head : null;
  const base = sel.kind === "pr-file" ? sel.range.base : null;
  // Commits and PR ranges never change, so only working-tree views follow the revision counter.
  const rev = sel.kind === "commit" || sel.kind === "pr-file" ? 0 : revision;
  return { kind, path, oldPath, sha, base, key: `${kind}\0${path}\0${oldPath}\0${sha}\0${base}\0${rev}` };
}

// Recently loaded/prefetched diffs; the key includes the revision, so stale entries never match.
// Revisions restart per repo, so the cache is per repo too: `generation` changes on switch and
// responses still in flight from the previous repo are dropped.
const pairCache = new Map<string, DiffPair>();
let generation = 0;
export function resetPairCache() {
  pairCache.clear();
  generation++;
}
function remember(key: string, pair: DiffPair, gen: number) {
  if (gen !== generation) return;
  pairCache.set(key, pair);
  if (pairCache.size > 32) pairCache.delete(pairCache.keys().next().value!);
}

/** Loads a diff and its syntax colors in the background, so opening it next is instant. */
export function prefetchSelection(sel: Selection, revision: number, theme: string) {
  if (sel.kind === "pull" || sel.kind === "issue") return;
  const { kind, path, oldPath, sha, base, key } = pairArgs(sel, revision);
  if (pairCache.has(key)) return;
  const gen = generation;
  api
    .diffPair(kind, path, oldPath, sha, base)
    .then((p) => {
      remember(key, p, gen);
      // Same text CodeView detects from, so the prefetched tokens are the ones it asks for.
      const lang = languageFor(path, p.modified.exists ? p.modified.text : p.original.text);
      if (kind !== "worktree") prefetchHighlight(p.original.text, lang, theme);
      prefetchHighlight(p.modified.text, lang, theme);
    })
    .catch(() => {});
}

// Every change anywhere in the repo bumps the revision; a file that didn't change keeps its
// pair object, so the code view doesn't rebuild and re-render every row for nothing.
const samePair = (a: DiffPair | null, b: DiffPair) =>
  !!a && sameText(a.original, b.original) && sameText(a.modified, b.modified);
const sameText = (a: DiffPair["original"], b: DiffPair["original"]) =>
  a.text === b.text && a.exists === b.exists && a.binary === b.binary && a.tooLarge === b.tooLarge && a.lossy === b.lossy;

function usePair(sel: FileSelection, revision: number) {
  const { kind, path, oldPath, sha, base, key } = pairArgs(sel, revision);
  const [pair, setPair] = useState<DiffPair | null>(() => pairCache.get(key) ?? null);
  const [error, setError] = useState<string | null>(null);
  const latest = useRef(0);
  const applied = useRef(0);

  useEffect(() => {
    const hit = pairCache.get(key);
    if (hit) {
      setPair((prev) => (samePair(prev, hit) ? prev : hit));
      setError(null);
      return;
    }
    // Apply any response newer than the last applied one, instead of dropping superseded
    // requests: a file rewritten faster than it loads would otherwise never update.
    const seq = ++latest.current;
    const gen = generation;
    api
      .diffPair(kind, path, oldPath, sha, base)
      .then((p) => {
        remember(key, p, gen);
        if (seq > applied.current) {
          applied.current = seq;
          setPair((prev) => (samePair(prev, p) ? prev : p));
          setError(null);
        }
      })
      .catch((e) => seq >= latest.current && setError(errorMessage(e)));
  }, [key, kind, path, oldPath, sha, base]);
  return { pair, error };
}

function findChange(status: RepoStatus | null, path: string): Selection | null {
  if (!status) return null;
  const unstaged = status.unstaged.find((f) => f.path === path);
  if (unstaged) return { kind: "unstaged", file: unstaged };
  const staged = status.staged.find((f) => f.path === path);
  return staged ? { kind: "staged", file: staged } : null;
}

function Pane({ tab, sel, status, revision, viewed, toggleViewed, onOpen }: ViewerProps & { tab: Tab; sel: FileSelection }) {
  const s = useSettings();
  const { pair, error } = usePair(sel, revision);
  const view = useRef<CodeViewHandle>(null);
  const isFile = sel.kind === "file";
  const file: FileChange | null = isFile ? null : sel.file;
  const change = isFile ? findChange(status, sel.path) : null;
  const media = mediaKind(selectionPath(sel)) !== null;
  const markdown = isMarkdown(selectionPath(sel));
  const svg = isSvg(selectionPath(sel));
  // Reading a markdown file starts rendered (unless turned off); reviewing its changes starts on the diff.
  const [markdownPreview, setMarkdownPreview] = useState(isFile && s.markdownPreview);
  // SVGs open however the last one was left, diffs included.
  const preview = svg ? s.svgPreview : markdownPreview;
  const setPreview = (on: boolean) => (svg ? updateSettings({ svgPreview: on }) : setMarkdownPreview(on));
  const rendered = (markdown || svg) && preview;
  const [zoom, setZoom] = useState<Zoom>(FIT);
  const [contrast, setContrast] = useState(false);
  const special = pair && (media ? (isFile && !pair.modified.exists ? "This file no longer exists" : null) : placeholderFor(pair, isFile));

  const diff = !isFile && !media && !rendered;
  useCommands({
    "diff.nextChange": diff ? () => view.current?.next() : undefined,
    "diff.prevChange": diff ? () => view.current?.prev() : undefined,
  });

  return (
    <>
      {sel.kind === "commit" && <CommitBar commit={sel.commit} url={sel.url} />}
      <div className="flex h-9 shrink-0 items-center gap-2 border-b border-border pr-2 pl-3">
        <FileIcon path={selectionPath(sel)} />
        <PathLabel path={selectionPath(sel)} className="min-w-0 text-[12px]" />
        <Tip label="Copy path">
          <button className="text-subtle hover:text-foreground" onClick={() => copy(selectionPath(sel), "Path copied")}>
            <Copy className="size-3" />
          </button>
        </Tip>
        {file?.oldPath && <span className="truncate text-[11.5px] text-subtle">← {file.oldPath}</span>}
        {file && <LineCounts file={file} />}
        {file && <StatusPill status={file.status} />}
        <div className="ml-auto flex shrink-0 items-center gap-1">
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
                <Button variant="ghost" size="icon-sm" onClick={() => setContrast(!contrast)} className={cn(contrast && "bg-primary/15 text-primary hover:bg-primary/20 hover:text-primary")}>
                  <Contrast />
                </Button>
              </Tip>
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
                className={cn(viewed(sel) && "bg-added-fill text-on-status hover:bg-added-fill/85")}
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
          <Placeholder title={special} />
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
            <CodeView
              ref={view}
              pair={pair}
              path={selectionPath(sel)}
              mode={isFile ? "file" : s.sideBySide ? "split" : "unified"}
              collapse={s.hideUnchanged}
              wrap={s.wordWrap}
              scrollKey={tab.key}
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
  if (a.binary || b.binary) return "Binary file";
  if (a.tooLarge || b.tooLarge) return "File is too large to display";
  if (!isFile && !pair.rows.some((r) => r.k !== 0)) return "No textual changes";
  return null;
}

function copy(text: string, what: string) {
  navigator.clipboard.writeText(text).then(() => toast("success", what));
}

function CommitBar({ commit, url }: { commit: import("@/lib/api").Commit; url?: string }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="shrink-0 border-b border-border bg-panel px-3 py-2">
      <div className="flex items-center gap-2">
        <GitCommitHorizontal className="size-3.5 shrink-0 text-primary" />
        <span className="truncate text-[12.5px] font-semibold select-text">{commit.subject}</span>
        <div className="ml-auto flex shrink-0 items-center gap-2 text-[11.5px] text-muted-foreground">
          <span>{commit.authorName}</span>
          <span className="text-subtle">·</span>
          <span title={new Date(commit.timestamp * 1000).toLocaleString()}>{relativeTime(commit.timestamp)}</span>
          <button className="rounded-sm bg-elevated px-1.5 py-px font-mono text-[11px] hover:text-foreground" onClick={() => copy(commit.sha, "Commit SHA copied")}>
            {commit.shortSha}
          </button>
          {commit.body && (
            <button className="font-medium text-primary hover:underline" onClick={() => setOpen(!open)}>
              {open ? "less" : "more"}
            </button>
          )}
          {url && (
            <div className="-my-1 flex items-center">
              <CopyLinkButton url={url} />
              <Tip label="Open on GitHub">
                <Button variant="ghost" size="icon-sm" aria-label="Open on GitHub" onClick={() => openOnGitHub(url)}>
                  <ExternalLink />
                </Button>
              </Tip>
            </div>
          )}
        </div>
      </div>
      {open && <pre className="mt-2 max-h-48 overflow-auto pl-5.5 font-sans text-[12px] leading-relaxed whitespace-pre-wrap text-muted-foreground select-text">{commit.body}</pre>}
    </div>
  );
}

function EmptyViewer({ hasTabs }: { hasTabs: boolean }) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3 text-center">
      <GitCompareArrows className="size-8 text-border-strong" strokeWidth={1.5} />
      <div className="text-[12.5px] text-muted-foreground">{hasTabs ? "No tab selected" : "Select a file to review"}</div>
      <div className="grid grid-cols-[auto_auto] gap-x-4 gap-y-1.5 text-left text-[11.5px] text-subtle">
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

function Placeholder({ title, detail }: { title: string; detail?: string }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-2 p-6 text-center">
      <div className="text-[12.5px] text-muted-foreground">{title}</div>
      {detail && <pre className="max-w-xl font-mono text-[11.5px] whitespace-pre-wrap text-subtle select-text">{detail}</pre>}
    </div>
  );
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
      <Button variant="ghost" size="icon-sm" onClick={onClick} className={cn(active && "bg-primary/15 text-primary hover:bg-primary/20 hover:text-primary")}>
        {children}
      </Button>
    </Tip>
  );
}

function Segmented<T extends string>({
  value,
  onChange,
  options,
}: {
  value: T;
  onChange: (v: T) => void;
  options: { value: T; label: string; icon?: React.ComponentType<{ className?: string }> }[];
}) {
  return (
    <div className="flex h-6 overflow-hidden rounded-md border border-border-strong">
      {options.map((o, i) => (
        <button
          key={o.value}
          onClick={() => onChange(o.value)}
          className={cn(
            "flex items-center gap-1 px-2 text-[11.5px] font-medium transition-colors",
            i > 0 && "border-l border-border-strong",
            value === o.value ? "bg-active text-foreground" : "text-subtle hover:text-foreground",
          )}
        >
          {o.icon && <o.icon className="size-3.5" />}
          {o.label}
        </button>
      ))}
    </div>
  );
}

function Sep() {
  return <div className="mx-0.5 h-4 w-px bg-border-strong" />;
}

