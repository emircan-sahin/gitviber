import { ChevronsUpDown, FolderDown, Plus } from "lucide-react";
import { useState } from "react";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { useCommands, useShortcut } from "@/lib/commands/keybindings";
import type { RepoData } from "@/lib/repo/useRepo";
import { folderName } from "@/lib/path";
import { openClone } from "@/features/projects/CloneDialog";
import { ProjectList, ProjectTile } from "@/features/projects/ProjectList";
import { changeTotals } from "@/features/changes/changeList";

export interface ProjectSwitcherProps {
  repo: RepoData;
  /** The main worktree: the project this window belongs to, even inside a linked worktree. */
  main: string;
  recent: string[];
  onOpenRepo: (path?: string) => void;
  onForgetRepo: (path: string) => void;
  onReorderRepos: (list: string[]) => void;
  onLocateRepo: (path: string) => void;
}

export function ProjectSwitcher({ repo, main, recent, onOpenRepo, onForgetRepo, onReorderRepos, onLocateRepo }: ProjectSwitcherProps) {
  const [open, setOpen] = useState(false);
  useCommands({ "file.switchProject": () => setOpen(true) });
  const totals = changeTotals(repo);
  const name = folderName(main);
  const openKey = useShortcut("file.openRepo");
  const pick = (p?: string) => {
    setOpen(false);
    onOpenRepo(p);
  };
  const locate = (p: string) => {
    setOpen(false);
    onLocateRepo(p);
  };
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button className="flex h-7 max-w-64 min-w-0 items-center gap-2 rounded-md px-2 hover:bg-hover focus-visible:bg-hover data-[state=open]:bg-active">
          <ProjectTile name={name} />
          <span className="truncate text-[12.5px] font-semibold">{name}</span>
          {totals.files > 0 && (
            <span className="shrink-0 font-mono text-[10.5px]">
              <span className="text-added">+{totals.add}</span> <span className="text-removed">-{totals.del}</span>
            </span>
          )}
          <ChevronsUpDown className="size-3 shrink-0 text-subtle" />
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" className="flex w-80 flex-col overflow-hidden">
        <div className="px-3 pt-2.5 pb-1 text-[10.5px] font-semibold tracking-[0.08em] text-subtle uppercase">Projects</div>
        <div className="max-h-[360px] min-h-0 overflow-x-hidden overflow-y-auto p-1">
          <ProjectList recent={recent} current={main} onOpen={pick} onForget={onForgetRepo} onReorder={onReorderRepos} onLocate={locate} />
        </div>
        <div className="border-t border-border p-1">
          <button onClick={() => pick()} className="flex h-7 w-full items-center gap-2 rounded-sm px-2 text-[12px] hover:bg-hover focus-visible:bg-hover">
            <Plus className="size-3.5 text-muted-foreground" /> Open repository…
            <span className="ml-auto font-mono text-[11px] text-subtle">{openKey}</span>
          </button>
          <button
            onClick={() => {
              setOpen(false);
              openClone();
            }}
            className="flex h-7 w-full items-center gap-2 rounded-sm px-2 text-[12px] hover:bg-hover focus-visible:bg-hover"
          >
            <FolderDown className="size-3.5 text-muted-foreground" /> Clone repository…
          </button>
        </div>
      </PopoverContent>
    </Popover>
  );
}
