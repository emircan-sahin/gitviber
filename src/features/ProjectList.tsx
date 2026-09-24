import { ArrowUpToLine, Check, Copy, ExternalLink, FolderOpen, FolderSearch, X } from "lucide-react";
import { arrayMove } from "@dnd-kit/sortable";
import { useEffect, useState } from "react";
import { SortableList, useSortableItem } from "@/components/Sortable";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuShortcut, ContextMenuTrigger } from "@/components/ui/context-menu";
import { Tip } from "@/components/ui/tooltip";
import { api, errorMessage, type ProjectInfo } from "@/lib/api";
import { REVEAL_LABEL } from "@/lib/commands";
import { matchesCommand } from "@/lib/keybindings";
import { toast } from "@/lib/toast";
import { cn } from "@/lib/utils";
import { folderName } from "@/lib/worktrees";
import { openOnGitHub } from "./PullsPanel";

export interface ProjectListProps {
  recent: string[];
  /** The open project: it can't be opened again or removed. */
  current?: string;
  onOpen: (path: string) => void;
  onForget: (path: string) => void;
  onReorder: (list: string[]) => void;
  /** Pick the new place of a folder that was moved. */
  onLocate: (path: string) => void;
}

const copy = (text: string, what: string) =>
  navigator.clipboard.writeText(text).then(
    () => toast("success", what),
    (e) => toast("error", "Could not copy", errorMessage(e)),
  );

const reveal = (path: string) => api.revealProject(path).catch((e) => toast("error", "Could not reveal in Finder", errorMessage(e)));

/** Asked each time a list mounts (the switcher mounts on open), so a moved folder shows up at once. */
function useProjectInfo(paths: string[]) {
  const [info, setInfo] = useState(new Map<string, ProjectInfo>());
  const key = paths.join("\n");
  useEffect(() => {
    let live = true;
    const list = key ? key.split("\n") : [];
    api
      .projectInfo(list)
      .then((all) => live && setInfo(new Map(list.map((p, i) => [p, all[i]]))))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [key]);
  return info;
}

/**
 * The saved projects, shared by the top bar's switcher and the welcome screen. Rows open on
 * click or ↵, reorder by dragging, and have hover actions and a right-click menu (also ⇧F10).
 */
export function ProjectList({ recent, current, onOpen, onForget, onReorder, onLocate }: ProjectListProps) {
  const info = useProjectInfo(recent);
  const tabStop = current && recent.includes(current) ? current : recent[0];

  const onKey = (e: React.KeyboardEvent) => {
    const row = e.target instanceof HTMLElement && e.target.dataset.project !== undefined ? e.target : null;
    if (!row) return;
    const rows = [...e.currentTarget.querySelectorAll<HTMLElement>("[data-project]")];
    const i = rows.indexOf(row);
    const path = row.dataset.project!;
    if (matchesCommand("project.forget", e.nativeEvent)) {
      if (path !== current) forget(row, path, onForget);
    } else if (e.altKey || e.metaKey || e.ctrlKey) return;
    else if (e.shiftKey ? e.key === "F10" : e.key === "ContextMenu") {
      // Radix opens the menu only on a contextmenu event; place it as a click on the row's start would.
      const r = row.getBoundingClientRect();
      row.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, clientX: r.left + 8, clientY: r.top + r.height / 2 }));
    } else if (e.shiftKey) return;
    else if (e.key === "ArrowDown") rows[Math.min(rows.length - 1, i + 1)].focus();
    else if (e.key === "ArrowUp") rows[Math.max(0, i - 1)].focus();
    else if (e.key === "Enter" || e.key === " ") {
      if (info.get(path)?.exists === false) onLocate(path);
      else if (path !== current) onOpen(path);
    }
    else return;
    e.preventDefault();
  };

  return (
    <div data-project-list onKeyDown={onKey}>
      <SortableList ids={recent} axis="y" onMove={(from, to) => onReorder(arrayMove(recent, from, to))}>
        {recent.map((p, i) => (
          <ProjectRow
            key={p}
            path={p}
            info={info.get(p)}
            current={p === current}
            first={i === 0}
            tabStop={p === tabStop}
            onOpen={onOpen}
            onForget={onForget}
            onLocate={onLocate}
            onMoveToTop={() => onReorder([p, ...recent.filter((x) => x !== p)])}
          />
        ))}
      </SortableList>
    </div>
  );
}

function ProjectRow({
  path,
  info,
  current,
  first,
  tabStop,
  onOpen,
  onForget,
  onLocate,
  onMoveToTop,
}: {
  path: string;
  info: ProjectInfo | undefined;
  current: boolean;
  first: boolean;
  tabStop: boolean;
  onOpen: (p: string) => void;
  onForget: (p: string) => void;
  onLocate: (p: string) => void;
  onMoveToTop: () => void;
}) {
  const { props, dragging, guard } = useSortableItem(path);
  const name = folderName(path);
  // Unknown until project_info answers; treat it as there rather than flash every row dimmed.
  const missing = info?.exists === false;
  const github = info?.github;
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <div
          {...props}
          role="button"
          tabIndex={tabStop ? 0 : -1}
          data-project={path}
          aria-current={current || undefined}
          onClick={guard(() => (missing ? onLocate(path) : !current && onOpen(path)))}
          className={cn(
            "group flex h-9 items-center gap-2.5 rounded-sm px-2 outline-none select-none focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset",
            current ? "cursor-default bg-active" : "cursor-pointer hover:bg-hover focus:bg-hover data-[state=open]:bg-hover",
            dragging && "cursor-grabbing bg-elevated shadow-lg ring-1 shadow-black/40 ring-border-strong",
          )}
        >
          <ProjectTile name={name} large className={cn(missing && "opacity-45")} />
          <div className="min-w-0 flex-1">
            <div className={cn("truncate text-[12.5px] font-medium", missing && "text-muted-foreground")}>{name}</div>
            <div className={cn("truncate text-[10.5px]", missing ? "text-conflict" : "text-subtle")}>{missing ? "Folder not found" : path}</div>
          </div>
          {current && <Check className="size-3.5 shrink-0 text-primary group-focus-within:hidden group-hover:hidden" />}
          {!dragging && (
            <div className="hidden shrink-0 items-center group-focus-within:flex group-hover:flex">
              {missing ? (
                <RowAction label="Locate…" onClick={() => onLocate(path)}>
                  <FolderSearch />
                </RowAction>
              ) : (
                <>
                  <RowAction label={REVEAL_LABEL} onClick={() => reveal(path)}>
                    <FolderSearch />
                  </RowAction>
                  <RowAction label="Copy path" onClick={() => copy(path, "Path copied")}>
                    <Copy />
                  </RowAction>
                </>
              )}
              {!current && (
                <RowAction label="Remove from list" onClick={(e) => forget(e.currentTarget, path, onForget)}>
                  <X />
                </RowAction>
              )}
            </div>
          )}
        </div>
      </ContextMenuTrigger>
      <ContextMenuContent>
        {missing ? (
          <ContextMenuItem onSelect={() => onLocate(path)}>
            <FolderSearch /> Locate…
          </ContextMenuItem>
        ) : (
          <>
            <ContextMenuItem disabled={current} onSelect={() => onOpen(path)}>
              <FolderOpen /> Open
              <ContextMenuShortcut>↵</ContextMenuShortcut>
            </ContextMenuItem>
            <ContextMenuItem onSelect={() => reveal(path)}>
              <FolderSearch /> {REVEAL_LABEL}
            </ContextMenuItem>
            {github && (
              <ContextMenuItem onSelect={() => openOnGitHub(`https://github.com/${github}`)}>
                <ExternalLink /> Open on GitHub
              </ContextMenuItem>
            )}
          </>
        )}
        <ContextMenuSeparator />
        <ContextMenuItem onSelect={() => copy(path, "Path copied")}>
          <Copy /> Copy Path
        </ContextMenuItem>
        <ContextMenuItem onSelect={() => copy(name, "Name copied")}>
          <Copy /> Copy Name
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem disabled={first} onSelect={onMoveToTop}>
          <ArrowUpToLine /> Move to Top
        </ContextMenuItem>
        <ContextMenuItem disabled={current} onSelect={() => forget(document.querySelector(`[data-project="${CSS.escape(path)}"]`), path, onForget)}>
          <X /> Remove from List
          <ContextMenuShortcut>⌫</ContextMenuShortcut>
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}

/** Removing a row takes the focus with it, to the page: hand it to a neighbouring row first. */
function forget(inRow: Element | null, path: string, onForget: (p: string) => void) {
  const row = inRow?.closest("[data-project]");
  const rows = [...(row?.closest("[data-project-list]")?.querySelectorAll<HTMLElement>("[data-project]") ?? [])];
  const i = rows.findIndex((r) => r === row);
  if (i >= 0) (rows[i + 1] ?? rows[i - 1])?.focus();
  onForget(path);
}

function RowAction({ label, onClick, children }: { label: string; onClick: (e: React.MouseEvent<HTMLButtonElement>) => void; children: React.ReactNode }) {
  return (
    <Tip label={label}>
      <button
        aria-label={label}
        // Pressing the button must not start a drag, and the click must not open the row.
        onPointerDown={(e) => e.stopPropagation()}
        onClick={(e) => {
          e.stopPropagation();
          onClick(e);
        }}
        className="flex size-5 items-center justify-center rounded-sm text-subtle outline-none hover:bg-active hover:text-foreground focus-visible:bg-active focus-visible:text-foreground focus-visible:ring-1 focus-visible:ring-ring [&_svg]:size-3"
      >
        {children}
      </button>
    </Tip>
  );
}

/** Square letter tile with a stable hue per project, so repos are recognizable at a glance. */
export function ProjectTile({ name, large, className }: { name: string; large?: boolean; className?: string }) {
  let h = 0;
  for (const c of name) h = (h * 31 + c.charCodeAt(0)) % 360;
  return (
    <span
      className={cn(
        "flex shrink-0 items-center justify-center rounded-[4px] font-bold text-black/80 uppercase",
        large ? "size-6 text-[11px]" : "size-4 text-[9.5px]",
        className,
      )}
      style={{ background: `hsl(${h} 55% 62%)` }}
    >
      {name[0]}
    </span>
  );
}
