import { ArrowDown, ArrowUp, Check, Columns2, Contrast, Copy, ExternalLink, Eye, FileCode2, FoldVertical, GitCommitHorizontal, GitCompareArrows, History, Rows2, Space, UserSearch, X } from "lucide-react";
import { Component, type ReactNode, type RefObject, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuTrigger } from "@/components/ui/context-menu";
import { Tip } from "@/components/ui/tooltip";
import { api, type Blame, type DiffKind, type DiffPair, type DiffRow, errorMessage, type FileChange, type RepoStatus, type Whitespace } from "@/lib/api";
import { onReveal, revealWaits } from "@/lib/reveal";
import { type Selection, selectionPath } from "@/lib/selection";
import { bindingsFor, type CommandId, formatChord, matchesCommand, useCommands, useShortcut } from "@/lib/keybindings";
import { diffWhitespace, getSettings, updateSettings, useSettings } from "@/lib/settings";
import { FIT, type Zoom } from "@/lib/svg";
import { toast } from "@/lib/toast";
import { isMenuKey, openRowMenu } from "@/lib/useListNav";
import { cn, relativeTime } from "@/lib/utils";
import { type CodeViewHandle, MonacoView } from "./MonacoView";
import { SortableList, useSortableItem } from "@/components/Sortable";
import { ConflictView } from "./ConflictView";
import { IssueStateIcon } from "./IssuesPanel";
import { IssueView } from "./IssueView";
import { CopyLinkButton, openOnGitHub, PullStateIcon } from "./PullsPanel";
import { PullView } from "./PullView";
import { FileIcon } from "./FileIcon";
import { SignatureBadge, TrailerChips, useCommitDetails } from "./HistoryPanel";
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
  /** History, filtered to a file's commits. */
  onShowHistory: (path: string) => void;
  /** Blame's link: a commit in History, with `path` (its name in that commit) open. */
  onShowCommit: (sha: string, path: string) => void;
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

/**
 * A tablist: the open tab is its one tab stop; ←/→ (Home/End) switch tabs, ↵ or Space keeps a
 * preview tab, ⌫ closes, ⌥←/⌥→ reorder (tab.moveLeft / tab.moveRight), ⇧F10 opens the tab's menu.
 */
function TabStrip({ tabs, active, onActivate, onClose, onPin, onMoveTab, onShowHistory }: ViewerProps) {
  const strip = useRef<HTMLDivElement>(null);
  // Set when a tab holding focus closes: focus goes on to the tab that opens in its place.
  const lostFocus = useRef(false);
  useLayoutEffect(() => {
    if (!lostFocus.current) return;
    lostFocus.current = false;
    strip.current?.querySelector<HTMLElement>('[role="tab"][tabindex="0"]')?.focus();
  });

  const onKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    const el = e.target instanceof HTMLElement && e.target.getAttribute("role") === "tab" ? e.target : null;
    if (!el) return;
    const els = [...e.currentTarget.querySelectorAll<HTMLElement>('[role="tab"]')];
    const i = els.indexOf(el);
    const step = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
    const shift = matchesCommand("tab.moveRight", e.nativeEvent) ? 1 : matchesCommand("tab.moveLeft", e.nativeEvent) ? -1 : 0;
    if (!shift && (e.metaKey || e.ctrlKey)) return;
    if (shift) {
      if (!tabs[i + shift]) return;
      onMoveTab(i, i + shift);
      // React may move this very node, and a node taken out of the page loses focus.
      requestAnimationFrame(() => {
        el.focus();
        el.scrollIntoView({ block: "nearest", inline: "nearest" });
      });
    } else if (isMenuKey(e)) openRowMenu(el);
    else if (e.shiftKey || e.altKey) return;
    else if (step || e.key === "Home" || e.key === "End") {
      const to = e.key === "Home" ? 0 : e.key === "End" ? els.length - 1 : Math.max(0, Math.min(els.length - 1, i + step));
      onActivate(tabs[to].key);
      els[to].focus();
      els[to].scrollIntoView({ block: "nearest", inline: "nearest" });
    } else if (e.key === "Enter" || e.key === " ") onPin(tabs[i].key);
    else if (e.key === "Backspace" || e.key === "Delete") onClose(tabs[i].key);
    else return;
    e.preventDefault();
  };

  return (
    <div
      ref={strip}
      role="tablist"
      aria-label="Open tabs"
      onKeyDown={onKeyDown}
      data-tauri-drag-region
      data-scrollbar="none"
      className="flex h-9 shrink-0 items-stretch overflow-x-auto overflow-y-hidden border-b border-border bg-panel"
    >
      <SortableList ids={tabs.map((t) => t.key)} axis="x" onMove={onMoveTab}>
        {tabs.map((t, i) => (
          <TabItem
            key={t.key}
            tab={t}
            active={t.key === active?.key}
            tabStop={active ? t.key === active.key : i === 0}
            lostFocus={lostFocus}
            onActivate={onActivate}
            onClose={onClose}
            onPin={onPin}
            onShowHistory={onShowHistory}
          />
        ))}
      </SortableList>
    </div>
  );
}

function TabItem({
  tab: t,
  active: isActive,
  tabStop,
  lostFocus,
  onActivate,
  onClose,
  onPin,
  onShowHistory,
}: {
  tab: Tab;
  active: boolean;
  tabStop: boolean;
  lostFocus: RefObject<boolean>;
  onActivate: (key: string) => void;
  onClose: (key: string) => void;
  onPin: (key: string) => void;
  onShowHistory: (path: string) => void;
}) {
  const { props, dragging, guard } = useSortableItem(t.key);
  const el = useRef<HTMLDivElement | null>(null);
  // Runs before the node leaves the page, while it can still say whether it had focus.
  useLayoutEffect(
    () => () => {
      if (el.current?.contains(document.activeElement)) lostFocus.current = true;
    },
    [lostFocus],
  );
  const tab = (
    <div
      {...props}
      ref={(node) => {
        props.ref(node);
        el.current = node;
      }}
      role="tab"
      aria-selected={isActive}
      tabIndex={tabStop ? 0 : -1}
      onClick={guard(() => onActivate(t.key))}
      onDoubleClick={() => onPin(t.key)}
      onAuxClick={(e) => e.button === 1 && onClose(t.key)}
      className={cn(
        "group relative flex max-w-56 shrink-0 cursor-pointer items-center gap-1.5 border-r border-border pr-1.5 pl-3 text-[12px] outline-none select-none focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset",
        isActive ? "bg-background text-foreground" : "bg-panel text-muted-foreground hover:bg-hover hover:text-foreground focus:bg-hover focus:text-foreground",
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
        aria-label="Close tab"
        // Off the Tab order: the tab closes with ⌫, and one stop per tab would crowd it.
        tabIndex={-1}
        // Pressing the close button must not start a drag.
        onPointerDown={(e) => e.stopPropagation()}
        onClick={(e) => {
          e.stopPropagation();
          onClose(t.key);
        }}
        className={cn(
          "flex size-5 items-center justify-center rounded-sm text-subtle hover:bg-active focus-visible:bg-active hover:text-foreground focus-visible:text-foreground",
          !isActive && "opacity-0 group-focus-within:opacity-100 group-hover:opacity-100",
        )}
      >
        <X className="size-3" />
      </button>
    </div>
  );
  if (t.sel.kind === "pull" || t.sel.kind === "issue") return tab;
  const path = selectionPath(t.sel);
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{tab}</ContextMenuTrigger>
      <ContextMenuContent>
        <ContextMenuItem onSelect={() => onShowHistory(path)}>
          <History /> Show History
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
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

function pairArgs(sel: FileSelection, revision: number, whitespace: Whitespace | null = null) {
  const kind: DiffKind =
    sel.kind === "file" ? "worktree" : sel.kind === "conflict" ? "unstaged" : sel.kind === "pr-file" ? "range" : sel.kind;
  const path = selectionPath(sel);
  const oldPath = sel.kind === "file" ? null : sel.file.oldPath;
  const sha = sel.kind === "commit" ? sel.commit.sha : sel.kind === "pr-file" ? sel.range.head : null;
  const base = sel.kind === "pr-file" ? sel.range.base : null;
  // Commits and PR ranges never change, so only working-tree views follow the revision counter.
  const rev = sel.kind === "commit" || sel.kind === "pr-file" ? 0 : revision;
  return { kind, path, oldPath, sha, base, whitespace, key: `${kind}\0${path}\0${oldPath}\0${sha}\0${base}\0${rev}\0${whitespace}` };
}

// Recently loaded/prefetched diffs; the key includes the revision, so stale entries never match.
// Revisions restart per repo, so the cache is per repo too: `generation` changes on switch and
// responses still in flight from the previous repo are dropped.
const pairCache = new Map<string, DiffPair>();
let generation = 0;
export function resetPairCache() {
  pairCache.clear();
  blames.clear();
  generation++;
}
function remember(key: string, pair: DiffPair, gen: number) {
  if (gen !== generation) return;
  pairCache.set(key, pair);
  if (pairCache.size > 32) pairCache.delete(pairCache.keys().next().value!);
}

/** Loads a diff in the background, so opening it next is instant. */
export function prefetchSelection(sel: Selection, revision: number) {
  if (sel.kind === "pull" || sel.kind === "issue") return;
  const { kind, path, oldPath, sha, base, whitespace, key } = pairArgs(sel, revision, diffWhitespace(getSettings()));
  if (pairCache.has(key)) return;
  const gen = generation;
  api
    .diffPair(kind, path, oldPath, sha, base, whitespace)
    .then((p) => remember(key, p, gen))
    .catch(() => {});
}

// Every change anywhere in the repo bumps the revision; a file that didn't change keeps its
// pair object, so the code view doesn't rebuild and re-render every row for nothing. The rows
// differ on the same texts when whitespace is ignored or no longer is.
const samePair = (a: DiffPair | null, b: DiffPair) =>
  !!a && sameText(a.original, b.original) && sameText(a.modified, b.modified) && sameRows(a.rows, b.rows);
const sameText = (a: DiffPair["original"], b: DiffPair["original"]) =>
  a.text === b.text && a.exists === b.exists && a.binary === b.binary && a.tooLarge === b.tooLarge && a.lossy === b.lossy && a.lfsMissing === b.lfsMissing;
const sameRows = (a: DiffRow[], b: DiffRow[]) =>
  a.length === b.length && a.every((r, i) => r.k === b[i].k && r.o === b[i].o && r.n === b[i].n && String(r.e) === String(b[i].e));

function usePair(sel: FileSelection, revision: number, ws: Whitespace | null) {
  const { kind, path, oldPath, sha, base, whitespace, key } = pairArgs(sel, revision, ws);
  const [pair, setPair] = useState<DiffPair | null>(() => pairCache.get(key) ?? null);
  const [error, setError] = useState<string | null>(null);
  const latest = useRef(0);
  const applied = useRef(0);

  useEffect(() => {
    const hit = pairCache.get(key);
    if (hit) {
      // Counts as the newest reply: one still in flight for another key (the other whitespace
      // setting, say) must not replace it when it lands.
      applied.current = ++latest.current;
      setPair((prev) => (samePair(prev, hit) ? prev : hit));
      setError(null);
      return;
    }
    // Apply any response newer than the last applied one, instead of dropping superseded
    // requests: a file rewritten faster than it loads would otherwise never update.
    const seq = ++latest.current;
    const gen = generation;
    api
      .diffPair(kind, path, oldPath, sha, base, whitespace)
      .then((p) => {
        remember(key, p, gen);
        if (seq > applied.current) {
          applied.current = seq;
          setPair((prev) => (samePair(prev, p) ? prev : p));
          setError(null);
        }
      })
      .catch((e) => seq >= latest.current && setError(errorMessage(e)));
  }, [key, kind, path, oldPath, sha, base, whitespace]);
  return { pair, error };
}

function findChange(status: RepoStatus | null, path: string): Selection | null {
  if (!status) return null;
  const unstaged = status.unstaged.find((f) => f.path === path);
  if (unstaged) return { kind: "unstaged", file: unstaged };
  const staged = status.staged.find((f) => f.path === path);
  return staged ? { kind: "staged", file: staged } : null;
}

function Pane({ tab, sel, status, revision, viewed, toggleViewed, onOpen, onShowCommit }: ViewerProps & { tab: Tab; sel: FileSelection }) {
  const s = useSettings();
  const { pair, error } = usePair(sel, revision, diffWhitespace(s));
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
  // SVGs open however the last one was left, diffs included.
  const preview = svg ? s.svgPreview : markdownPreview;
  const setPreview = (on: boolean) => (svg ? updateSettings({ svgPreview: on }) : setMarkdownPreview(on));
  const rendered = (markdown || svg) && preview;
  const toCode = useRef(() => {});
  toCode.current = () => void (isFile && rendered && setPreview(false));
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
  });

  return (
    <>
      {sel.kind === "commit" && <CommitBar commit={sel.commit} url={sel.url} />}
      <div className="flex h-9 shrink-0 items-center gap-2 border-b border-border pr-2 pl-3">
        <FileIcon path={selectionPath(sel)} />
        <PathLabel path={selectionPath(sel)} className="min-w-0 text-[12px]" />
        <Tip label="Copy path">
          <button className="text-subtle hover:text-foreground focus-visible:text-foreground" onClick={() => copy(selectionPath(sel), "Path copied")}>
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
            />
          )
        )}
      </div>
    </>
  );
}

// Blame by file content and HEAD: the same text at the same HEAD blames the same, so a file is
// blamed once per version, not on every refresh.
const blames = new Map<string, Promise<Blame>>();

function useBlame(path: string | null, pair: DiffPair | null, head: string | null) {
  const [result, setResult] = useState<{ key: string; blame: Blame } | null>(null);
  const text = path ? pair?.modified.text : undefined;
  const version = useMemo(() => text !== undefined && `${text.length}:${hash(text)}`, [text]);
  const key = path && version ? `${path}\0${head}\0${version}` : null;
  useEffect(() => {
    if (!key || !path) return;
    let p = blames.get(key);
    if (!p) {
      p = api.blame(path);
      blames.set(key, p);
      p.catch(() => blames.delete(key));
      if (blames.size > 32) blames.delete(blames.keys().next().value!);
    }
    let alive = true;
    p.then(
      (blame) => alive && setResult({ key, blame }),
      (e) => alive && toast("error", "Could not blame this file", errorMessage(e)),
    );
    return () => {
      alive = false;
    };
  }, [key, path]);
  return result && result.key === key ? result.blame : null;
}

/** FNV-1a: tells file versions apart for the blame cache. */
function hash(text: string) {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) h = Math.imul(h ^ text.charCodeAt(i), 0x01000193);
  return (h >>> 0).toString(36);
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

function copy(text: string, what: string) {
  navigator.clipboard.writeText(text).then(() => toast("success", what));
}

function CommitBar({ commit, url }: { commit: import("@/lib/api").Commit; url?: string }) {
  const [open, setOpen] = useState(false);
  const details = useCommitDetails(commit.sha);
  return (
    <div className="shrink-0 border-b border-border bg-panel px-3 py-2">
      <div className="flex items-center gap-2">
        <GitCommitHorizontal className="size-3.5 shrink-0 text-primary" />
        <span className="truncate text-[12.5px] font-semibold select-text">{commit.subject}</span>
        <div className="ml-auto flex shrink-0 items-center gap-2 text-[11.5px] text-muted-foreground">
          <span>{commit.authorName}</span>
          <span className="text-subtle">·</span>
          <span title={new Date(commit.timestamp * 1000).toLocaleString()}>{relativeTime(commit.timestamp)}</span>
          {/* Rebased or cherry-picked: it landed later than it was written, maybe by someone else. */}
          {relativeTime(commit.committedAt) !== relativeTime(commit.timestamp) && (
            <>
              <span className="text-subtle">·</span>
              <span title={new Date(commit.committedAt * 1000).toLocaleString()}>
                committed {relativeTime(commit.committedAt)}
                {commit.committerName !== commit.authorName && ` by ${commit.committerName}`}
              </span>
            </>
          )}
          {details && <SignatureBadge details={details} />}
          <button className="rounded-sm bg-elevated px-1.5 py-px font-mono text-[11px] hover:text-foreground focus-visible:text-foreground" onClick={() => copy(commit.sha, "Commit SHA copied")}>
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
      {details && <TrailerChips details={details} className="mt-1.5 pl-5.5" />}
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
      <Button variant="ghost" size="icon-sm" onClick={onClick} className={cn(active && "bg-primary/15 text-primary hover:bg-primary/20 focus-visible:bg-primary/20 hover:text-primary focus-visible:text-primary")}>
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
            // The wrapper's overflow-hidden would clip an outer focus ring.
            "flex items-center gap-1 px-2 text-[11.5px] font-medium transition-colors outline-none focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset",
            i > 0 && "border-l border-border-strong",
            value === o.value ? "bg-active text-foreground" : "text-subtle hover:text-foreground focus-visible:text-foreground",
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

