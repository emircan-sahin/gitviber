import { ChevronLeft, ChevronRight } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Tip } from "@/components/ui/tooltip";
import { StatusDot } from "@/components/StatusDot";
import { useShortcut } from "@/lib/commands/keybindings";
import { lookLabel, paneLook } from "@/lib/terminal/agentLook";
import { paneDir, panesInOrder, revealPane, stepPane, type TerminalGroup } from "@/lib/terminal/terminals";
import { folderName } from "@/lib/path";
import { cn } from "@/lib/utils";
import { showPaneSwitch } from "./PaneLayout";

type PaneInfo = TerminalGroup["panes"][number];

/** What a pane's header calls it: the user's name for it, else the program's title, else its folder. */
export const paneTitle = (p: PaneInfo) => p.name ?? (p.title || folderName(paneDir(p.id) ?? p.cwd));

/** "2 · api — Claude Code working": a pane by number, and its dot's state. */
export function paneLine(p: PaneInfo, n: number) {
  const look = paneLook(p);
  return `${n} · ${paneTitle(p)}` + (look ? ` — ${lookLabel(look, p.agent?.name)}` : "");
}

/**
 * A split's panes as dots, by number: each in its agent's color (agentLook), the focused one larger.
 * Agent dots are the full size, so a question's halo and a finish's hollow ring still read. `onPick`:
 * each dot is a button to its pane.
 */
export function PaneDots({ panes, focused, onPick }: { panes: PaneInfo[]; focused: number; onPick?: (id: number) => void }) {
  return (
    <span className={cn("flex shrink-0 items-center", onPick ? "gap-0" : "gap-1")}>
      {panes.map((p, i) => {
        const look = paneLook(p);
        const big = p.id === focused;
        const dot = look ? (
          <StatusDot look={look} className={cn(big && "size-2")} />
        ) : (
          <span className={cn("shrink-0 rounded-full bg-current", big ? "size-1.5 text-muted-foreground" : "size-1 text-subtle")} />
        );
        if (!onPick) return <span key={p.id} className="flex">{dot}</span>;
        const label = paneLine(p, i + 1);
        return (
          <Tip key={p.id} label={label}>
            <button aria-label={`Pane ${label}`} aria-current={big || undefined} tabIndex={-1} onClick={() => onPick(p.id)} className="flex size-4 items-center justify-center rounded-sm hover:bg-active">
              {dot}
            </button>
          </Tip>
        );
      })}
    </span>
  );
}

/**
 * While one pane of a split fills the panel (zoomed): which page it is, the others' dots (an agent
 * working out of sight), and a click to another, still zoomed. In the panel's toolbar, so it takes no height.
 */
export function ZoomPager({ group }: { group: TerminalGroup }) {
  const panes = panesInOrder(group);
  const n = panes.findIndex((p) => p.id === group.focused) + 1;
  const [prevKey, nextKey] = [useShortcut("terminal.prevPane"), useShortcut("terminal.nextPane")];
  const step = (dir: 1 | -1) => showPaneSwitch(stepPane(dir));
  return (
    <div className="flex shrink-0 items-center">
      <Tip label="Previous pane" shortcut={prevKey}>
        <Button variant="ghost" size="icon-sm" className="w-5" onClick={() => step(-1)}>
          <ChevronLeft className="size-3" />
        </Button>
      </Tip>
      <span className="px-0.5 font-mono text-[11px] text-muted-foreground tabular-nums">
        {n} / {panes.length}
      </span>
      <Tip label="Next pane" shortcut={nextKey}>
        <Button variant="ghost" size="icon-sm" className="w-5" onClick={() => step(1)}>
          <ChevronRight className="size-3" />
        </Button>
      </Tip>
      <PaneDots
        panes={panes}
        focused={group.focused}
        onPick={(id) => {
          revealPane(id);
          showPaneSwitch(id);
        }}
      />
    </div>
  );
}
