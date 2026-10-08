import { ChevronDown, Copy, MessageSquareText, SquareTerminal } from "lucide-react";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Tip } from "@/components/ui/tooltip";
import { programOf, reviewAgent } from "@/lib/git/suggest";
import { useSettings } from "@/lib/settings";
import { type Ask, askAgent, copyAsPrompt, pasteToRunningAgent } from "./askAgent";

/**
 * Hands part of a guided review to the user's agent in `root`'s terminal: a new session by
 * default, the agent already running there or the clipboard from its menu. `ask` is read on click.
 */
export function AskAgentButton({ root, ask, label = "Ask Agent", what = "this" }: { root: string; ask: () => Ask; label?: string; what?: string }) {
  const program = programOf(reviewAgent(useSettings()).command);
  const claude = program === "claude";
  const tip = claude ? `A new Claude Code session in the terminal, with ${what} as context` : `Copies ${what} for ${program || "your agent"} to paste`;
  return (
    <div className="inline-flex shrink-0">
      <Tip label={tip}>
        <Button size="sm" variant="secondary" className="rounded-r-none" disabled={!root} onClick={() => void askAgent(root, ask())}>
          <MessageSquareText /> {label}
        </Button>
      </Tip>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button size="sm" variant="secondary" className="rounded-l-none border-l border-border px-1" disabled={!root} aria-label={`More ways to ${label.toLowerCase()}`}>
            <ChevronDown />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="w-60">
          <DropdownMenuItem onSelect={() => void pasteToRunningAgent(root, ask())}>
            <SquareTerminal /> Paste into Running Agent
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={() => copyAsPrompt(ask())}>
            <Copy /> Copy as Prompt
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}
