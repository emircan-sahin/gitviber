import { FolderOpen } from "lucide-react";
import { Button } from "@/components/ui/button";

export function Welcome({ recent, onOpenRepo }: { recent: string[]; onOpenRepo: (path?: string) => void }) {
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
          <span className="ml-auto font-mono text-[11px] opacity-70">⌘O</span>
        </Button>
        {recent.length > 0 && (
          <div className="mt-6 border-t border-border pt-3">
            <div className="mb-1.5 text-[10.5px] font-semibold tracking-[0.08em] text-subtle uppercase">Recent</div>
            {recent.map((p) => (
              <button key={p} onClick={() => onOpenRepo(p)} className="flex w-full items-baseline gap-2 rounded-md px-2 py-1.5 text-left hover:bg-hover">
                <span className="shrink-0 text-[12.5px] font-medium">{p.split("/").pop()}</span>
                <span className="truncate text-[11px] text-subtle">{p}</span>
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
