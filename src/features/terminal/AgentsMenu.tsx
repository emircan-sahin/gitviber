import { Bot } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Tip } from "@/components/ui/tooltip";
import { NeedsYouDot, WorkingDot } from "@/components/NeedsYouDot";
import type { Worktree } from "@/lib/api";
import { toast } from "@/lib/app/toast";
import { useCommands } from "@/lib/commands/keybindings";
import { plural, shortDuration } from "@/lib/format";
import { folderName } from "@/lib/path";
import { worktreeOf } from "@/lib/git/worktrees";
import { useAgentList } from "@/lib/terminal/agents";
import { type AgentEntry, agentsWaiting } from "@/lib/terminal/agentState";
import { revealPane } from "@/lib/terminal/terminals";
import { cn } from "@/lib/utils";

const LABEL: Record<AgentEntry["state"], string> = { working: "Working", waiting: "Needs you", finished: "Finished", running: "Running" };

/** The worktree a pane opened in, with its branch; else its folder. */
function where(cwd: string, worktrees: Worktree[]) {
  const w = worktreeOf(cwd, worktrees);
  if (!w) return folderName(cwd) || cwd;
  return `${folderName(w.path)} · ${w.branch ?? "detached"}`;
}

/**
 * Every agent running in a terminal, in any tab or project: the ones that need the user first,
 * how long each has been in its state, and a click to its pane. Shown once there's an agent.
 */
export function AgentsMenu({ worktrees }: { worktrees: Worktree[] }) {
  const list = useAgentList();
  const [open, setOpen] = useState(false);
  // An agent was picked: the keys stay with its pane once the menu closes.
  const picked = useRef(false);
  // The times move on while it's open; nothing runs while it's closed.
  const [, tick] = useState(0);
  useEffect(() => {
    if (!open) return;
    const t = window.setInterval(() => tick((n) => n + 1), 30_000);
    return () => window.clearInterval(t);
  }, [open]);
  useCommands({
    "terminal.showAgents": () => (list.length ? setOpen(true) : toast("info", "No agents in the terminals", "Claude Code and other coding agents show here while they run in a terminal.")),
  });
  if (!list.length) return null;

  const waiting = agentsWaiting(list);
  const working = list.filter((e) => e.state === "working").length;
  const summary = [waiting && `${waiting} ${waiting === 1 ? "needs" : "need"} you`, working && `${working} working`].filter(Boolean).join(" · ");
  const now = Date.now();
  return (
    <DropdownMenu open={open} onOpenChange={setOpen}>
      <Tip label={`Agents${summary ? ` · ${summary}` : ""}`}>
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" size="icon" aria-label={`Agents: ${plural(list.length, "agent")}${summary ? `, ${summary}` : ""}`} className="relative">
            <Bot />
            {waiting > 0 ? <NeedsYouDot className="absolute top-1 right-1" /> : working > 0 && <WorkingDot className="absolute top-1 right-1" />}
          </Button>
        </DropdownMenuTrigger>
      </Tip>
      {/* Radix would hand focus back to the trigger after revealPane gave it to the pane. */}
      <DropdownMenuContent
        align="end"
        className="w-80"
        onCloseAutoFocus={(e) => {
          if (!picked.current) return;
          picked.current = false;
          e.preventDefault();
        }}
      >
        <DropdownMenuLabel>Agents</DropdownMenuLabel>
        {list.map((e) => {
          const calling = e.state === "waiting" || e.unseen;
          return (
            <DropdownMenuItem
              key={e.pane}
              onSelect={() => {
                picked.current = true;
                revealPane(e.pane);
              }}
              className="items-start py-1.5"
            >
              <span className="flex h-4 w-1.5 shrink-0 items-center">
                {calling ? <NeedsYouDot className="in-data-[highlighted]:bg-primary-foreground" /> : e.state === "working" && <WorkingDot className="in-data-[highlighted]:border-primary-foreground" />}
              </span>
              <span className="min-w-0 flex-1">
                <span className="flex min-w-0 items-center gap-2">
                  <span className="truncate font-medium">{e.name}</span>
                  <span className={cn("ml-auto shrink-0 text-[10.5px] tabular-nums", calling ? "text-primary in-data-[highlighted]:text-primary-foreground" : "opacity-70")}>
                    {LABEL[e.state]}
                    {e.since > 0 && ` · ${shortDuration((now - e.since) / 1000)}`}
                  </span>
                </span>
                <span className="block truncate text-[10.5px] opacity-70">{where(e.cwd, worktrees)}</span>
              </span>
            </DropdownMenuItem>
          );
        })}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
