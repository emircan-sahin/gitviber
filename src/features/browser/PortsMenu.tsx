import { Network } from "lucide-react";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Tip } from "@/components/ui/tooltip";
import { portUrl, scanPorts, usePorts } from "@/lib/browser/ports";
import { ptysIn } from "@/lib/terminal/terminals";

/** What this worktree's terminals serve (a dev server's port), read again as the menu opens; picking one opens it here. */
export function PortsMenu({ root, onOpen }: { root: string; onOpen: (url: string) => void }) {
  const ptys = ptysIn(root);
  const ports = usePorts(ptys);
  return (
    <DropdownMenu onOpenChange={(open) => open && void scanPorts(ptysIn(root))}>
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
