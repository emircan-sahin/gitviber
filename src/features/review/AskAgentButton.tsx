import { ChevronDown, Copy, MessageSquareText, SquareTerminal } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Tip } from "@/components/ui/tooltip";
import { QuestionBox, useAskModel } from "@/components/QuestionBox";
import { reviewAgent } from "@/lib/git/suggest";
import { useSettings } from "@/lib/settings";
import { agentName } from "@/lib/terminal/handoff";
import { type Ask, askAgent, copyAsPrompt, pasteToRunningAgent } from "./askAgent";

/**
 * Hands part of a guided review to the user's agent in `root`'s terminal: a new session by
 * default, the agent already running there or the clipboard from its menu. `ask` is read on click.
 * `question`: the button first asks for one (Skip leaves it to the agent's prompt box); a risk's
 * Send to Agent is the ask itself.
 */
export function AskAgentButton({ root, ask, label = "Ask Agent", what = "this", question = true }: { root: string; ask: () => Ask; label?: string; what?: string; question?: boolean }) {
  const agent = agentName(reviewAgent(useSettings()).command);
  const tip = agent === "Claude Code" ? `A new Claude Code session in the terminal, with ${what} in its prompt box` : `Copies ${what} for ${agent} to paste`;
  const [open, setOpen] = useState(false);
  const choice = useAskModel("review");
  const start = (q = "") => {
    setOpen(false);
    void askAgent(root, ask(), q, choice.model);
  };
  const main = (
    <Button size="sm" variant="secondary" className="rounded-r-none" disabled={!root} onClick={question ? undefined : () => start()}>
      <MessageSquareText /> {label}
    </Button>
  );
  return (
    <div className="inline-flex shrink-0">
      {question ? (
        <Popover open={open} onOpenChange={setOpen}>
          <Tip label={tip}>
            <PopoverTrigger asChild>{main}</PopoverTrigger>
          </Tip>
          <PopoverContent align="end" className="w-80">
            <QuestionBox agent={agent} what={what} onAsk={start} model={choice.picker} />
          </PopoverContent>
        </Popover>
      ) : (
        <Tip label={tip}>{main}</Tip>
      )}
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
