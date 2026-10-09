import { Network } from "lucide-react";
import { useState } from "react";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Tip } from "@/components/ui/tooltip";
import { browserApi, type ListeningPort } from "@/lib/api";
import { portUrl } from "@/lib/browser/url";
import { ptysIn } from "@/lib/terminal/terminals";

/** What a terminal session serves, read as a menu opens; the list on show stays while nothing changed. */
export function usePortsOnOpen(ptys: () => number[]) {
  const [ports, setPorts] = useState<ListeningPort[]>([]);
  const read = () => {
    const sessions = ptys();
    if (!sessions.length) return setPorts([]);
    void browserApi
      .ports(sessions)
      .then((next) => setPorts((was) => (JSON.stringify(was) === JSON.stringify(next) ? was : next)))
      .catch(() => {});
  };
  return { ports, read };
}

/** What this worktree's terminals serve (a dev server's port); picking one opens it here. */
export function PortsMenu({ root, onOpen }: { root: string; onOpen: (url: string) => void }) {
  const { ports, read } = usePortsOnOpen(() => ptysIn(root));
  return (
    <DropdownMenu onOpenChange={(open) => open && read()}>
      <Tip label="Ports the terminals serve">
        <DropdownMenuTrigger asChild>
          <button
            type="button"
            aria-label="Ports the terminals serve"
            className="flex size-6 shrink-0 items-center justify-center rounded-md text-muted-foreground outline-none hover:bg-hover focus-visible:bg-hover hover:text-foreground focus-visible:text-foreground data-[state=open]:bg-active data-[state=open]:text-foreground"
          >
            <Network className="size-3.5" />
          </button>
        </DropdownMenuTrigger>
      </Tip>
      <DropdownMenuContent align="end">
        <DropdownMenuLabel>{ports.length ? "Served by this worktree's terminals" : "No terminal here serves a port"}</DropdownMenuLabel>
        {ports.map((p) => (
          <DropdownMenuItem key={p.port} onSelect={() => onOpen(portUrl(p.port))}>
            <span className="font-mono">localhost:{p.port}</span>
            <span className="ml-auto pl-4 text-[11px] text-subtle">{p.process}</span>
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
