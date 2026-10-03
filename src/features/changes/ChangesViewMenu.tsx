import { List, ListTree } from "lucide-react";
import { DropdownMenu, DropdownMenuContent, DropdownMenuLabel, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Tip } from "@/components/ui/tooltip";
import { type ChangeSort, changesView, setChangesView } from "./changesView";

/** How Changes lists files: a list or a folder tree, by name or most recently modified first. */
export function ChangesViewMenu() {
  const view = changesView.use();
  const Icon = view.tree ? ListTree : List;
  return (
    <DropdownMenu>
      <Tip label="View and sort">
        <DropdownMenuTrigger asChild>
          <button
            aria-label="View and sort"
            className="flex size-6 items-center justify-center rounded-sm text-subtle outline-none hover:bg-hover focus-visible:bg-hover hover:text-foreground focus-visible:text-foreground data-[state=open]:bg-active data-[state=open]:text-foreground"
          >
            <Icon className="size-3.5" />
          </button>
        </DropdownMenuTrigger>
      </Tip>
      <DropdownMenuContent align="end">
        <DropdownMenuLabel>View</DropdownMenuLabel>
        <DropdownMenuRadioGroup value={view.tree ? "tree" : "list"} onValueChange={(v) => setChangesView({ tree: v === "tree" })}>
          <DropdownMenuRadioItem value="list">List</DropdownMenuRadioItem>
          <DropdownMenuRadioItem value="tree">Tree</DropdownMenuRadioItem>
        </DropdownMenuRadioGroup>
        <DropdownMenuSeparator />
        <DropdownMenuLabel>Sort by</DropdownMenuLabel>
        <DropdownMenuRadioGroup value={view.sort} onValueChange={(v) => setChangesView({ sort: v as ChangeSort })}>
          <DropdownMenuRadioItem value="name">Name</DropdownMenuRadioItem>
          <DropdownMenuRadioItem value="recent">Recently Modified</DropdownMenuRadioItem>
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
