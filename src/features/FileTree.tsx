import { ChevronRight } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type ChangeStatus, type Entry, errorMessage, type RepoStatus } from "@/lib/api";
import { type Selection, selectionKey } from "@/lib/selection";
import { toast } from "@/lib/toast";
import { cn } from "@/lib/utils";
import { FileIcon, FolderIcon } from "./FileIcon";
import { statusInfo } from "./StatusBadge";

interface Props {
  status: RepoStatus | null;
  revision: number;
  activeKey: string | null;
  onOpen: (s: Selection, pin?: boolean) => void;
  onHover: (s: Selection) => void;
}

const INDENT = 12;

/** Lazy tree of the working directory, like VS Code's explorer, annotated with git status. */
export function FileTree({ status, revision, activeKey, onOpen, onHover }: Props) {
  const [children, setChildren] = useState<Record<string, Entry[]>>({});
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set([""]));

  // Per-path request counter: a slow, older listing must not overwrite a newer one.
  const requests = useRef(new Map<string, number>());
  const loadDir = useCallback(async (path: string) => {
    const id = (requests.current.get(path) ?? 0) + 1;
    requests.current.set(path, id);
    try {
      const entries = await api.listDir(path);
      if (requests.current.get(path) === id) setChildren((c) => ({ ...c, [path]: entries }));
    } catch (e) {
      if (requests.current.get(path) !== id) return;
      // Folder deleted (agents do that): close it quietly instead of erroring on every refresh.
      setExpanded((x) => {
        const next = new Set(x);
        next.delete(path);
        return next;
      });
      setChildren(({ [path]: _gone, ...rest }) => rest);
      if (path === "") toast("error", "Could not list repository", errorMessage(e));
    }
  }, []);

  // Re-list open folders whenever the disk changes so new/deleted files show up.
  useEffect(() => {
    expanded.forEach((p) => loadDir(p));
  }, [revision, loadDir]);

  const { fileStatus, dirtyDirs } = useMemo(() => {
    const fileStatus = new Map<string, ChangeStatus>();
    for (const f of [...(status?.staged ?? []), ...(status?.unstaged ?? []), ...(status?.conflicted ?? [])]) {
      fileStatus.set(f.path, f.status);
    }
    const dirtyDirs = new Set<string>();
    for (const path of fileStatus.keys()) {
      const parts = path.split("/");
      for (let i = 1; i < parts.length; i++) dirtyDirs.add(parts.slice(0, i).join("/"));
    }
    return { fileStatus, dirtyDirs };
  }, [status]);

  const toggle = (path: string) => {
    const next = new Set(expanded);
    if (next.has(path)) next.delete(path);
    else {
      next.add(path);
      // Always re-list: the cached listing may be from before the agent changed it.
      loadDir(path);
    }
    setExpanded(next);
  };

  const renderDir = (dir: string, depth: number): React.ReactNode =>
    children[dir]?.map((e) => {
      const isOpen = expanded.has(e.path);
      const st = fileStatus.get(e.path);
      const tone = st ? statusInfo(st).text : undefined;
      const sel: Selection = { kind: "file", path: e.path };
      const active = !e.isDir && activeKey === selectionKey(sel);
      return (
        <div key={e.path}>
          <div
            role="button"
            onClick={() => (e.isDir ? toggle(e.path) : onOpen(sel))}
            onDoubleClick={() => !e.isDir && onOpen(sel, true)}
            onMouseEnter={() => !e.isDir && !e.ignored && onHover(sel)}
            style={{ paddingLeft: 8 + depth * INDENT }}
            className={cn(
              "relative flex h-6 cursor-pointer items-center gap-1.5 pr-2 text-[12px]",
              active ? "bg-primary/15" : "hover:bg-hover",
              e.ignored && "opacity-40",
            )}
          >
            {active && <span className="absolute inset-y-0 left-0 w-0.5 bg-primary" />}
            {/* indent guides */}
            {Array.from({ length: depth }, (_, i) => (
              <span key={i} className="absolute inset-y-0 w-px bg-border" style={{ left: 14 + i * INDENT }} />
            ))}
            {e.isDir ? (
              <>
                <ChevronRight className={cn("size-3 shrink-0 text-subtle transition-transform duration-100", isOpen && "rotate-90")} />
                <FolderIcon name={e.name} open={isOpen} />
              </>
            ) : (
              <>
                <span className="w-3 shrink-0" />
                <FileIcon path={e.path} />
              </>
            )}
            <span className={cn("truncate", tone ?? "text-foreground/85")}>{e.name}</span>
            {st && <span className={cn("ml-auto font-mono text-[10.5px] font-bold", tone)}>{statusInfo(st).letter}</span>}
            {!st && e.isDir && dirtyDirs.has(e.path) && <span className="ml-auto size-1.5 rounded-full bg-modified" />}
          </div>
          {e.isDir && isOpen && renderDir(e.path, depth + 1)}
        </div>
      );
    });

  return <div className="h-full overflow-x-hidden overflow-y-auto py-1">{renderDir("", 0)}</div>;
}
