import { Check, Palette } from "lucide-react";
import { ContextMenuItem, ContextMenuSeparator, ContextMenuSub, ContextMenuSubContent, ContextMenuSubTrigger } from "@/components/ui/context-menu";
import { type Hue, HUE_NAMES, hueColor } from "@/lib/git/worktrees";
import { setWorktreeColor } from "@/lib/git/worktreeColors";

/** A worktree row's Color submenu: the eight hues, or none. `hue`: the one it shows now. */
export function WorktreeColorMenu({ path, hue }: { path: string; hue: Hue | null }) {
  return (
    <ContextMenuSub>
      <ContextMenuSubTrigger>
        <Palette /> Color
      </ContextMenuSubTrigger>
      <ContextMenuSubContent className="min-w-36">
        {HUE_NAMES.map((h) => (
          <ContextMenuItem key={h} onSelect={() => setWorktreeColor(path, h)}>
            <span className="size-3 shrink-0 rounded-full" style={{ background: hueColor(h) }} />
            <span className="capitalize">{h}</span>
            {hue === h && <Check className="ml-auto" />}
          </ContextMenuItem>
        ))}
        <ContextMenuSeparator />
        <ContextMenuItem onSelect={() => setWorktreeColor(path, "none")}>
          <span className="size-3 shrink-0 rounded-full border border-subtle" />
          None
          {hue === null && <Check className="ml-auto" />}
        </ContextMenuItem>
      </ContextMenuSubContent>
    </ContextMenuSub>
  );
}
