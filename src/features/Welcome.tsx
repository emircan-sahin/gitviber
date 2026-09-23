import { FolderDown, FolderOpen } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useShortcut } from "@/lib/keybindings";
import { openClone } from "./CloneDialog";
import { ProjectList } from "./ProjectList";

export function Welcome({
  recent,
  onOpenRepo,
  onForgetRepo,
  onReorderRepos,
  onLocateRepo,
}: {
  recent: string[];
  onOpenRepo: (path?: string) => void;
  onForgetRepo: (path: string) => void;
  onReorderRepos: (list: string[]) => void;
  onLocateRepo: (path: string) => void;
}) {
  const openKey = useShortcut("file.openRepo");
  return (
    <div data-tauri-drag-region className="flex h-full items-center justify-center bg-background">
      <div className="w-[400px]">
        <div className="flex items-center gap-3">
          <img src="/icon.svg" alt="" className="size-11" />
          <div>
            <h1 className="text-[20px] font-semibold tracking-tight">GitViber</h1>
            <p className="text-[12px] text-muted-foreground">Review what your agent wrote. Plain git, no workspace.</p>
          </div>
        </div>
        <Button size="lg" className="mt-6 w-full justify-start" onClick={() => onOpenRepo()}>
          <FolderOpen /> Open repository
          <span className="ml-auto font-mono text-[11px] opacity-70">{openKey}</span>
        </Button>
        <Button size="lg" variant="secondary" className="mt-2 w-full justify-start" onClick={openClone}>
          <FolderDown /> Clone repository
        </Button>
        {recent.length > 0 && (
          <div className="mt-6 border-t border-border pt-3">
            <div className="mb-1.5 text-[10.5px] font-semibold tracking-[0.08em] text-subtle uppercase">Recent</div>
            <ProjectList recent={recent} onOpen={onOpenRepo} onForget={onForgetRepo} onReorder={onReorderRepos} onLocate={onLocateRepo} />
          </div>
        )}
      </div>
    </div>
  );
}
