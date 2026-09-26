// Pieces of GitViber's own UI, rebuilt from its classes (src/features/*) so the mockups on this
// page look like the app: same tokens, sizes, fonts and icons. Rendered at the app's real size;
// <Scaled> fits a whole window into the page.
import {
  ArrowDownToLine,
  Check,
  ChevronDown,
  ChevronRight,
  ChevronsDownUp,
  ChevronsUpDown,
  Columns2,
  Copy,
  Ellipsis,
  FileCode2,
  FolderGit2,
  GitBranch,
  GitCompareArrows,
  PanelLeft,
  PanelLeftClose,
  PanelRight,
  PanelRightClose,
  Plus,
  RefreshCw,
  Redo2,
  Rows2,
  Search,
  Settings2,
  Sparkles,
  SquareTerminal,
  Trash2,
  Undo2,
  UploadCloud,
  UserSearch,
  X,
} from "lucide-react";
import { useEffect, useRef, type ReactNode } from "react";
import folderApi from "material-icon-theme/icons/folder-api.svg?url";
import folderApiOpen from "material-icon-theme/icons/folder-api-open.svg?url";
import folderIcon from "material-icon-theme/icons/folder.svg?url";
import folderOpen from "material-icon-theme/icons/folder-open.svg?url";
import folderPublic from "material-icon-theme/icons/folder-public.svg?url";
import folderPublicOpen from "material-icon-theme/icons/folder-public-open.svg?url";
import folderSrc from "material-icon-theme/icons/folder-src.svg?url";
import folderSrcOpen from "material-icon-theme/icons/folder-src-open.svg?url";
import htmlIcon from "material-icon-theme/icons/html.svg?url";
import markdownIcon from "material-icon-theme/icons/markdown.svg?url";
import nodeIcon from "material-icon-theme/icons/nodejs.svg?url";
import readmeIcon from "material-icon-theme/icons/readme.svg?url";
import tsconfigIcon from "material-icon-theme/icons/tsconfig.svg?url";
import viteIcon from "material-icon-theme/icons/vite.svg?url";
import reactIcon from "material-icon-theme/icons/react_ts.svg?url";
import testIcon from "material-icon-theme/icons/test-ts.svg?url";
import tsIcon from "material-icon-theme/icons/typescript.svg?url";
import { Logo } from "../icons.tsx";
import { cx } from "../ui.tsx";

export function fileIcon(path: string) {
  const name = path.split("/").pop()!;
  if (name === "package.json") return nodeIcon;
  if (name.startsWith("tsconfig")) return tsconfigIcon;
  if (name.startsWith("vite.config")) return viteIcon;
  if (name === "README.md") return readmeIcon;
  if (name.endsWith(".html")) return htmlIcon;
  if (/\.test\.tsx?$/.test(path)) return testIcon;
  if (path.endsWith(".tsx")) return reactIcon;
  if (path.endsWith(".md")) return markdownIcon;
  return tsIcon;
}
const FOLDERS: Record<string, [string, string]> = {
  src: [folderSrc, folderSrcOpen],
  public: [folderPublic, folderPublicOpen],
  api: [folderApi, folderApiOpen],
};
const folderIconFor = (name: string, open: boolean) => (FOLDERS[name] ?? [folderIcon, folderOpen])[open ? 1 : 0];

export type Status = "M" | "A" | "D";
const STATUS_COLOR: Record<Status, string> = { M: "text-modified", A: "text-added", D: "text-removed" };
const STATUS_FILL: Record<Status, [string, string]> = {
  M: ["bg-modified", "Modified"],
  A: ["bg-added", "Added"],
  D: ["bg-removed", "Deleted"],
};

/** Fits a fixed-size piece of UI into its container's width, like a screenshot would. */
export function Scaled({ width, height, children, className }: { width: number; height: number; children: ReactNode; className?: string }) {
  const outer = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = outer.current;
    if (!el) return;
    const ro = new ResizeObserver(([entry]) => el.style.setProperty("--s", String(entry.contentRect.width / width)));
    ro.observe(el);
    return () => ro.disconnect();
  }, [width]);
  return (
    <div ref={outer} className={cx("relative w-full overflow-hidden", className)} style={{ aspectRatio: `${width} / ${height}` }}>
      <div
        className="absolute top-0 left-0 origin-top-left [transform:scale(var(--s,1))]"
        style={{ width, height }}
      >
        {children}
      </div>
    </div>
  );
}

/** The window frame: rounded, hairline border, the traffic lights over the top bar. */
export function Window({ children, className, lights = true }: { children: ReactNode; className?: string; lights?: boolean }) {
  return (
    <div
      className={cx(
        "relative flex h-full w-full flex-col overflow-hidden rounded-[10px] border border-border-strong bg-sidebar font-sans text-[12px] leading-[1.4] text-fg select-none",
        className,
      )}
    >
      {lights && (
        <span className="absolute top-[14px] left-[14px] z-10 flex gap-2">
          <i className="size-3 rounded-full bg-[#ff5f57]" />
          <i className="size-3 rounded-full bg-[#febc2e]" />
          <i className="size-3 rounded-full bg-[#28c840]" />
        </span>
      )}
      {children}
    </div>
  );
}

const divider = <span className="mx-2 h-4 w-px shrink-0 bg-border-strong" />;

function repoHue(name: string) {
  let h = 0;
  for (const c of name) h = (h * 31 + c.charCodeAt(0)) % 360;
  return h;
}

function Ghost({ children, active }: { children: ReactNode; active?: boolean }) {
  return (
    <span className={cx("grid size-7 place-items-center rounded-md [&_svg]:size-3.5", active ? "text-fg" : "text-muted")}>
      {children}
    </span>
  );
}

export function TopBar({
  repo,
  branch,
  worktree,
  worktrees,
  added,
  removed,
  menuOpen,
}: {
  repo: string;
  branch: string;
  /** Folder of the linked worktree we're in, or undefined for the main one. */
  worktree?: string;
  worktrees: number;
  added: number;
  removed: number;
  menuOpen?: boolean;
}) {
  return (
    <div className="flex h-10 shrink-0 items-center gap-1 border-b border-border bg-sidebar pr-2 pl-[86px] whitespace-nowrap [&>*]:shrink-0">
      <span className="flex items-center gap-1.5">
        <Logo className="size-[18px]" />
        <span className="text-[12.5px] font-semibold tracking-tight">GitViber</span>
      </span>
      {divider}
      <span className="flex h-7 items-center gap-2 rounded-md px-2">
        <span
          className="grid size-4 place-items-center rounded-[4px] text-[9.5px] font-bold text-black/80 uppercase"
          style={{ background: `hsl(${repoHue(repo)} 55% 62%)` }}
        >
          {repo[0]}
        </span>
        <span className="text-[12.5px] font-semibold">{repo}</span>
        <span className="font-mono text-[10.5px]">
          <span className="text-added">+{added}</span> <span className="text-removed">-{removed}</span>
        </span>
        <ChevronsUpDown className="size-3 text-subtle" />
      </span>
      <span className="text-[13px] text-border-strong">/</span>
      <span className="flex h-7 items-center gap-1.5 rounded-md px-2">
        <GitBranch className="size-3.5 text-primary" />
        <span className="font-mono text-[12px]">{branch}</span>
        <ChevronsUpDown className="size-3 text-subtle" />
      </span>
      <span
        className={cx(
          "flex h-7 items-center gap-1.5 rounded-md px-2",
          worktree && "bg-primary/10",
          menuOpen && "bg-active",
        )}
      >
        <FolderGit2 className={cx("size-3.5", worktree ? "text-primary" : "text-subtle")} />
        {worktree ? (
          <span className="font-mono text-[12px]">{worktree}</span>
        ) : (
          <span className="text-[12px]">
            Worktrees<span className="text-muted"> ({worktrees})</span>
          </span>
        )}
        <ChevronsUpDown className="size-3 text-subtle" />
      </span>
      <span className="!shrink flex-1" />
      <Ghost>
        <Undo2 />
      </Ghost>
      <Ghost>
        <Redo2 />
      </Ghost>
      <span className="mx-1 h-4 w-px bg-border-strong" />
      <Ghost>
        <RefreshCw />
      </Ghost>
      <span className="ml-1 flex">
        <span className="flex h-7 items-center gap-1.5 rounded-l-md border border-border-strong bg-elevated px-2.5 text-[12px] font-medium">
          <ArrowDownToLine className="size-3.5" /> Pull
        </span>
        <span className="flex h-7 w-5 items-center justify-center rounded-r-md border border-l-0 border-border-strong bg-elevated">
          <ChevronDown className="size-3" />
        </span>
      </span>
      <span className="ml-1.5 flex h-7 items-center gap-1.5 rounded-md bg-primary px-2.5 text-[12px] font-medium text-white">
        <UploadCloud className="size-3.5" /> Publish
      </span>
      <span className="mx-1 h-4 w-px bg-border-strong" />
      <Ghost active>
        <SquareTerminal />
      </Ghost>
      <Ghost active>
        <PanelLeft />
      </Ghost>
      <Ghost active>
        <PanelRight />
      </Ghost>
      <Ghost>
        <Settings2 />
      </Ghost>
    </div>
  );
}

export interface WorktreeRow {
  branch: string;
  path: string;
  main?: boolean;
  working?: boolean;
  time: string;
  changes?: number;
}

export function WorktreeMenu({ rows, current, highlighted }: { rows: WorktreeRow[]; current: number; highlighted?: number }) {
  return (
    <div className="w-96 max-w-full rounded-md border border-border-strong bg-elevated p-1 shadow-lg shadow-black/50">
      <p className="px-2 pt-2 pb-1 text-[10.5px] font-semibold tracking-[0.08em] text-subtle uppercase">Worktrees</p>
      {rows.map((row, i) => {
        const hi = i === highlighted;
        return (
          <div key={row.path} className={cx("flex h-9 items-center gap-2 rounded-sm px-2", hi && "bg-primary text-white")}>
            {i === current ? <Check className="size-3.5 shrink-0" /> : <GitBranch className="size-3.5 shrink-0 opacity-60" />}
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-1.5">
                <span className="truncate font-mono text-[11.5px]">{row.branch}</span>
                {row.main && (
                  <span className={cx("rounded-sm px-1 text-[10px] leading-4", hi ? "bg-white/20" : "bg-active text-muted")}>
                    main
                  </span>
                )}
                {row.working && (
                  <span className={cx("rounded-sm px-1 text-[10px] leading-4", hi ? "bg-white/20" : "bg-primary/15 text-primary")}>
                    working
                  </span>
                )}
              </div>
              <div className={cx("flex items-center gap-1 text-[10.5px]", hi ? "text-white/75" : "text-subtle")}>
                <FolderGit2 className="size-3" />
                <span className="truncate">{row.path}</span>
              </div>
            </div>
            <div className={cx("flex flex-col items-end text-[10.5px] leading-4", hi ? "text-white/75" : "text-subtle")}>
              <span>{row.time}</span>
              {row.changes ? (
                <span className={hi ? "" : "text-removed"}>{row.changes} changes</span>
              ) : (
                <span>no changes</span>
              )}
            </div>
          </div>
        );
      })}
      <div className="mt-1 border-t border-border px-2.5 py-1.5 text-[10.5px] text-subtle">New worktree…</div>
    </div>
  );
}

function PathLabel({ path, dim }: { path: string; dim?: boolean }) {
  const cut = path.lastIndexOf("/") + 1;
  return (
    <span className={cx("flex min-w-0 items-baseline", dim && "opacity-45")}>
      <span className="truncate text-subtle">{path.slice(0, cut)}</span>
      <span className="shrink-0 truncate text-fg">{path.slice(cut)}</span>
    </span>
  );
}

function Counts({ add, del }: { add: number; del: number }) {
  return (
    <span className="font-mono text-[11px] tabular-nums">
      {add > 0 && <span className="text-added">+{add}</span>}
      {add > 0 && del > 0 && " "}
      {del > 0 && <span className="text-removed">-{del}</span>}
    </span>
  );
}

export interface ChangeFile {
  path: string;
  add: number;
  del: number;
  status: Status;
  viewed?: boolean;
}

function PanelTab({ children, active, count }: { children: ReactNode; active?: boolean; count?: number }) {
  return (
    <span
      className={cx(
        "flex h-6 items-center gap-1.5 rounded-md px-2 text-[12px] font-medium",
        active ? "bg-active text-fg" : "text-subtle",
      )}
    >
      {children}
      {count !== undefined && (
        <span className="rounded-sm bg-modified px-1 font-mono text-[10px] leading-4 text-black/85">{count}</span>
      )}
    </span>
  );
}

export function ChangeRow({ file, active, className }: { file: ChangeFile; active?: boolean; className?: string }) {
  return (
    <div className={cx("relative flex h-[26px] items-center gap-2 pr-2 pl-2 text-[12px]", active && "bg-primary/15", className)}>
      {active && <span className="absolute inset-y-0 left-0 w-0.5 bg-primary" />}
      <span
        className={cx(
          "viewed-box grid size-3.5 shrink-0 place-items-center rounded-[3px] border",
          file.viewed ? "border-added bg-added text-black/85" : "border-border-strong",
        )}
      >
        {file.viewed && <Check className="size-2.5" strokeWidth={3} />}
      </span>
      <img src={fileIcon(file.path)} alt="" className="size-4 shrink-0" />
      <span className="min-w-0 flex-1">
        <PathLabel path={file.path} dim={file.viewed} />
      </span>
      <Counts add={file.add} del={file.del} />
      <span className={cx("w-3 text-center font-mono text-[11px] font-bold", STATUS_COLOR[file.status])}>{file.status}</span>
    </div>
  );
}

export function GitPanel({
  files,
  active = 0,
  width = 320,
  children,
  footer = <CommitBox />,
}: {
  files: ChangeFile[];
  active?: number;
  width?: number;
  /** Replaces the rows, for a panel whose rows animate on their own. */
  children?: ReactNode;
  /** The commit box by default; `false` leaves it out. */
  footer?: ReactNode;
}) {
  const add = files.reduce((s, f) => s + f.add, 0);
  const del = files.reduce((s, f) => s + f.del, 0);
  const reviewed = files.filter((f) => f.viewed).length;
  return (
    <div className="flex h-full shrink-0 flex-col border-r border-border bg-panel" style={{ width }}>
      <div className="flex h-9 shrink-0 items-center gap-0.5 border-b border-border pr-1 pl-2">
        <PanelTab active count={files.length}>
          Changes
        </PanelTab>
        <PanelTab>History</PanelTab>
        <PanelTab>PRs</PanelTab>
        <PanelTab>Issues</PanelTab>
        <span className="ml-auto grid size-6 place-items-center text-subtle">
          <GitCompareArrows className="size-3.5" />
        </span>
        <span className="grid size-6 place-items-center text-subtle">
          <PanelLeftClose className="size-3.5" />
        </span>
      </div>
      <div className="shrink-0 border-b border-border px-3 py-2">
        <div className="flex items-center gap-2 text-[11.5px]">
          <span className="text-muted">
            <b className="font-semibold text-fg">{files.length}</b> files
          </span>
          <Counts add={add} del={del} />
          <span className="ml-auto text-muted">
            <span className={cx("font-semibold", files.length && reviewed === files.length ? "text-added" : "text-fg")}>{reviewed}</span>/
            {files.length} reviewed
          </span>
        </div>
        <div className="mt-1.5 h-[3px] overflow-hidden bg-border">
          <div className="h-full bg-added transition-[width] duration-500" style={{ width: `${files.length ? (reviewed / files.length) * 100 : 0}%` }} />
        </div>
      </div>
      <div className="flex h-7 shrink-0 items-center gap-1 border-b border-border pr-1.5 pl-2 text-[10.5px] font-semibold tracking-[0.08em] text-subtle uppercase">
        <ChevronDown className="size-3" /> Changes
        <span className="ml-1 font-mono tracking-normal text-muted">{files.length}</span>
      </div>
      <div className="flex-1 py-0.5">
        {children ?? files.map((f, i) => <ChangeRow key={f.path} file={f} active={i === active} />)}
      </div>
      {footer}
    </div>
  );
}

export interface CommitState {
  summary?: string;
  description?: string;
  /** Which field the caret is in while the agent writes. */
  caret?: "summary" | "description";
  /** The sparkle while claude -p is writing. */
  asking?: boolean;
  pressed?: boolean;
}

const Caret = () => <span className="ml-px inline-block h-3.5 w-px translate-y-[3px] animate-blink bg-primary" />;

export function CommitBox({ summary, description, caret, asking, pressed }: CommitState) {
  return (
    <div className="shrink-0 border-t border-border bg-panel p-2">
      <div
        className={cx(
          "flex h-7 items-center overflow-hidden rounded-md border bg-bg px-2.5 font-medium whitespace-nowrap",
          caret === "summary" ? "border-primary" : "border-border-strong",
          !summary && "text-subtle",
        )}
      >
        {summary || "Summary"}
        {caret === "summary" && <Caret />}
      </div>
      <div
        className={cx(
          "mt-1.5 h-[52px] overflow-hidden rounded-md border bg-bg px-3 py-1.5 leading-relaxed",
          caret === "description" ? "border-primary" : "border-border-strong",
          description ? "text-fg" : "text-subtle",
        )}
      >
        {description || "Description"}
        {caret === "description" && <Caret />}
      </div>
      <div className="mt-1.5 flex items-center gap-2">
        <span className="flex items-center gap-1.5 text-[11.5px] text-muted">
          <span className="size-3 rounded-[3px] border border-border-strong" /> Amend
        </span>
        <span
          className={cx(
            "grid size-6 place-items-center rounded-md transition-colors",
            asking ? "bg-primary/15 text-primary" : "text-muted",
          )}
        >
          <Sparkles className={cx("size-3.5", asking && "animate-pulse")} />
        </span>
        <span className="grid size-6 place-items-center text-muted">
          <Ellipsis className="size-3.5" />
        </span>
        <span
          className={cx(
            "ml-auto flex h-7 flex-1 items-center justify-center rounded-md bg-primary text-[12px] font-medium text-white transition",
            pressed && "scale-[0.97] brightness-90",
          )}
        >
          Commit all
        </span>
      </div>
    </div>
  );
}

/** The editor's tab: a diff says so; a file just opened from the tree is a preview, in italics. */
export function EditorTab({ name, kind, preview = true, unsaved }: { name: string; kind?: string; preview?: boolean; unsaved?: boolean }) {
  return (
    <div className="flex h-9 shrink-0 border-b border-border bg-panel">
      <div className="relative flex items-center gap-1.5 border-r border-border bg-bg pr-1.5 pl-3 text-[12px]">
        <span className="absolute inset-x-0 top-0 h-px bg-primary" />
        <span className="absolute inset-x-0 -bottom-px h-px bg-bg" />
        <img src={fileIcon(name)} alt="" className="size-4" />
        <span className={cx(preview && "italic")}>{name}</span>
        {kind && <span className="font-mono text-[10px] text-subtle">{kind}</span>}
        <span className="grid size-5 place-items-center text-subtle">
          {unsaved ? <span className="size-2 rounded-full bg-current" /> : <X className="size-3" />}
        </span>
      </div>
    </div>
  );
}

export function FileHeader({
  path,
  add,
  del,
  status,
  compact,
  file,
}: {
  path: string;
  add: number;
  del: number;
  status: Status;
  /** Without the view toggles and buttons, for a phone-wide panel. */
  compact?: boolean;
  /** The file itself rather than its diff: Blame and a way back to the changes. */
  file?: boolean;
}) {
  const [fill, label] = STATUS_FILL[status];
  return (
    <div className="flex h-9 shrink-0 items-center gap-2 border-b border-border bg-bg pr-2 pl-3">
      <img src={fileIcon(path)} alt="" className="size-4" />
      <span className="text-[12px]">
        <PathLabel path={path} />
      </span>
      <Copy className="size-3 text-subtle" />
      <Counts add={add} del={del} />
      <span className={cx("rounded-sm px-1.5 py-px text-[10.5px] font-semibold text-black/85", fill)}>{label}</span>
      {!compact && file && (
        <span className="ml-auto flex items-center gap-1">
          <span className="grid size-6 place-items-center text-subtle">
            <UserSearch className="size-3.5" />
          </span>
          <span className="mx-0.5 h-4 w-px bg-border-strong" />
          <span className="flex h-6 items-center gap-1.5 rounded-md border border-border-strong bg-elevated px-2 text-[11.5px] font-medium">
            <GitCompareArrows className="size-3.5" /> Changes
          </span>
        </span>
      )}
      {!compact && !file && (
        <span className="ml-auto flex items-center gap-1">
          <span className="flex h-6 overflow-hidden rounded-md border border-border-strong text-[11.5px] font-medium">
            <span className="flex items-center gap-1 bg-active px-2">
              <Rows2 className="size-3.5" /> Unified
            </span>
            <span className="flex items-center gap-1 border-l border-border-strong px-2 text-subtle">
              <Columns2 className="size-3.5" /> Split
            </span>
          </span>
          <span className="mx-0.5 h-4 w-px bg-border-strong" />
          <span className="flex h-6 items-center gap-1.5 rounded-md border border-border-strong bg-elevated px-2 text-[11.5px] font-medium">
            <FileCode2 className="size-3.5" /> Open file
          </span>
          <span className="flex h-6 items-center gap-1.5 rounded-md border border-border-strong bg-elevated px-2 text-[11.5px] font-medium">
            <Check className="size-3.5" /> Viewed
          </span>
        </span>
      )}
    </div>
  );
}

export interface TerminalTab {
  folder: string;
  branch: string;
  panes?: number;
}

/** The terminal's tab bar; `fresh` slides a just-opened tab in. */
export function TerminalTabs({ tabs, active = 0, fresh }: { tabs: TerminalTab[]; active?: number; fresh?: number }) {
  return (
    <div className="flex h-9 shrink-0 items-stretch border-b border-border bg-panel">
      {tabs.map((tab, i) => (
        <div
          key={tab.folder}
          className={cx(
            "relative flex shrink-0 items-center gap-1.5 border-r border-border pr-1.5 pl-3 text-[12px] whitespace-nowrap",
            i === active ? "bg-bg text-fg" : "text-muted",
            i === fresh && "tab-in",
          )}
        >
          {i === active && (
            <>
              <span className="absolute inset-x-0 top-0 h-px bg-primary" />
              <span className="absolute inset-x-0 -bottom-px h-px bg-bg" />
            </>
          )}
          <SquareTerminal className={cx("size-3.5", i === active ? "text-primary" : "text-subtle")} />
          {tab.folder}
          <span className="font-mono text-[10.5px] text-subtle">{tab.branch}</span>
          {tab.panes && (
            <span className="rounded-sm bg-elevated px-1 font-mono text-[10px] leading-4 text-muted">{tab.panes}</span>
          )}
          <span className="grid size-5 place-items-center text-subtle">
            <X className="size-3" />
          </span>
        </div>
      ))}
      <span className="ml-auto flex items-center gap-0.5 px-1.5 text-muted [&>span]:grid [&>span]:size-6 [&>span]:place-items-center [&_svg]:size-3.5">
        <span>
          <Plus />
        </span>
        <span className="!w-4">
          <ChevronDown className="!size-3" />
        </span>
        <span>
          <Columns2 />
        </span>
        <span>
          <Trash2 />
        </span>
        <i className="mx-0.5 h-4 w-px bg-border-strong" />
        <span>
          <ChevronDown />
        </span>
      </span>
    </div>
  );
}

/**
 * Terminal text: the code font at 12.5px with xterm's 1.2 line height, the app's ANSI colors.
 * `wrap` breaks long lines at the edge the way xterm does in a narrow pane.
 */
export function TerminalBody({ children, className, wrap }: { children: ReactNode; className?: string; wrap?: boolean }) {
  return (
    <div
      className={cx(
        "bg-bg pt-2 pb-1 pl-3 font-mono text-[12.5px] leading-[15px] text-fg",
        wrap ? "pr-3 break-all whitespace-pre-wrap" : "whitespace-pre",
        className,
      )}
    >
      {children}
    </div>
  );
}

export const ansi = {
  cyan: "text-[#56c8d8]",
  green: "text-[#57d17a]",
  yellow: "text-[#e2b84c]",
  red: "text-[#f47067]",
  blue: "text-[#4a9ff5]",
  magenta: "text-[#b392f0]",
  dim: "text-[#6c6c73]",
};

export function Prompt({ folder, branch }: { folder: string; branch: string }) {
  return (
    <>
      <span className={ansi.cyan}>{folder}</span> <span className={ansi.green}>{branch}</span> %{" "}
    </>
  );
}

export const Cursor = () => <span className="inline-block h-[15px] w-[8px] translate-y-[3px] border border-primary align-top" />;

export function StatusBar({ files, add, del, reviewed }: { files: number; add: number; del: number; reviewed: number }) {
  return (
    <div className="flex h-6 shrink-0 items-center gap-3 border-t border-border bg-sidebar px-3 text-[11px] text-subtle">
      <span>
        {files} changed · <span className="font-mono text-added">+{add}</span>{" "}
        <span className="font-mono text-removed">-{del}</span> · {reviewed}/{files} reviewed
      </span>
      <span className="ml-auto">Nord</span>
      <span>SF Mono 12.5</span>
      <span>Unified</span>
      <span>Wrap</span>
      <span>TypeScript</span>
      <span>v0.1.0 · macOS</span>
    </div>
  );
}

/**
 * The explorer on the right (src/features/explorer/FileTree.tsx): folders first, indent guides,
 * changed files in their status color with the letter, a dot on folders holding changes.
 */
export function ExplorerPanel({
  paths,
  open,
  active,
  status,
  width = 260,
}: {
  /** Every entry in tree order; folders end in "/". */
  paths: string[];
  open: string[];
  active?: string;
  status: Record<string, Status>;
  width?: number;
}) {
  const shown = paths.filter((p) => {
    const parts = p.replace(/\/$/, "").split("/");
    return parts.slice(0, -1).every((_, i) => open.includes(parts.slice(0, i + 1).join("/") + "/"));
  });
  const changed = Object.keys(status);
  return (
    <div className="flex h-full shrink-0 flex-col border-l border-border bg-panel" style={{ width }}>
      <div className="flex h-9 shrink-0 items-center gap-0.5 border-b border-border pr-1 pl-2">
        <PanelTab active>Explorer</PanelTab>
        <PanelTab>Search</PanelTab>
        <span className="ml-auto grid size-6 place-items-center text-subtle">
          <Search className="size-3.5" />
        </span>
        <span className="grid size-6 place-items-center text-subtle">
          <ChevronsDownUp className="size-3.5" />
        </span>
        <span className="grid size-6 place-items-center text-subtle">
          <PanelRightClose className="size-3.5" />
        </span>
      </div>
      <div className="min-h-0 flex-1 overflow-hidden py-1">
        {shown.map((p) => {
          const dir = p.endsWith("/");
          const parts = p.replace(/\/$/, "").split("/");
          const depth = parts.length - 1;
          const name = parts[depth];
          const isOpen = dir && open.includes(p);
          const st = status[p];
          const isActive = p === active;
          return (
            <div
              key={p}
              className={cx("relative flex h-6 items-center gap-1.5 pr-2 text-[12px] transition-colors duration-150", isActive && "bg-primary/15")}
              style={{ paddingLeft: 8 + depth * 12 }}
            >
              {Array.from({ length: depth }, (_, i) => (
                <span key={i} className="absolute inset-y-0 w-px bg-border" style={{ left: 14 + i * 12 }} />
              ))}
              {isActive && <span className="absolute inset-y-0 left-0 w-0.5 bg-primary" />}
              {dir ? (
                <>
                  <ChevronRight className={cx("size-3 shrink-0 text-subtle transition-transform duration-100", isOpen && "rotate-90")} />
                  <img src={folderIconFor(name, isOpen)} alt="" className="size-4 shrink-0" />
                </>
              ) : (
                <>
                  <span className="w-3 shrink-0" />
                  <img src={fileIcon(p)} alt="" className="size-4 shrink-0" />
                </>
              )}
              <span className={cx("truncate", st ? STATUS_COLOR[st] : "text-fg/85")}>{name}</span>
              {st && <span className={cx("ml-auto font-mono text-[10.5px] font-bold", STATUS_COLOR[st])}>{st}</span>}
              {dir && changed.some((c) => c.startsWith(p)) && <span className="ml-auto size-1.5 rounded-full bg-modified" />}
            </div>
          );
        })}
      </div>
    </div>
  );
}

/** Quick open (src/features/palette/CommandPalette.tsx): the query, then files with the hits in bold. */
export function QuickOpen({ query, items }: { query: string; items: { path: string; hits: number[] }[] }) {
  return (
    <div className="menu-in absolute inset-0 z-40 bg-black/55">
      <div className="absolute top-[12%] left-1/2 flex w-[576px] -translate-x-1/2 flex-col overflow-hidden rounded-md border border-border-strong bg-elevated shadow-lg shadow-black/60">
        <div className={cx("flex h-10 shrink-0 items-center border-b border-border px-3 text-[13px]", !query && "text-subtle")}>
          {query || "Search files by name · type > for commands"}
          {query && <span className="ml-px inline-block h-4 w-px animate-blink bg-fg" />}
        </div>
        <div className="p-1">
          {items.map((item, i) => {
            const cut = item.path.lastIndexOf("/") + 1;
            const bold = (text: string, offset: number) =>
              [...text].map((c, j) => (item.hits.includes(offset + j) ? <b key={j} className="font-semibold">{c}</b> : c));
            return (
              <div
                key={item.path}
                className={cx("flex h-7 items-center gap-2 rounded-sm px-2 text-[12.5px]", i === 0 ? "bg-primary text-white" : "text-fg")}
              >
                <img src={fileIcon(item.path)} alt="" className="size-4 shrink-0" />
                <span className="shrink-0">{bold(item.path.slice(cut), cut)}</span>
                <span className="min-w-0 flex-1 truncate text-[11.5px] opacity-60">{bold(item.path.slice(0, cut - 1), 0)}</span>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
