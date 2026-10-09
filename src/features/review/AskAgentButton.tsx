import { ChevronDown, Copy, MessageSquareText, SquareTerminal } from "lucide-react";
import { useLayoutEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Textarea } from "@/components/ui/textarea";
import { Tip } from "@/components/ui/tooltip";
import { programOf, reviewAgent } from "@/lib/git/suggest";
import { useSettings } from "@/lib/settings";
import { type Ask, askAgent, copyAsPrompt, pasteToRunningAgent } from "./askAgent";

/**
 * Hands part of a guided review to the user's agent in `root`'s terminal: a new session by
 * default, the agent already running there or the clipboard from its menu. `ask` is read on click.
 * `question`: the button first asks for one (Skip leaves it to the agent's prompt box); a risk's
 * Send to Agent is the ask itself.
 */
export function AskAgentButton({ root, ask, label = "Ask Agent", what = "this", question = true }: { root: string; ask: () => Ask; label?: string; what?: string; question?: boolean }) {
  const program = programOf(reviewAgent(useSettings()).command);
  const agent = program.toLowerCase() === "claude" ? "Claude Code" : program || "your agent";
  const tip = agent === "Claude Code" ? `A new Claude Code session in the terminal, with ${what} in its prompt box` : `Copies ${what} for ${agent} to paste`;
  const [open, setOpen] = useState(false);
  const start = (q = "") => {
    setOpen(false);
    void askAgent(root, ask(), q);
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
            <QuestionBox agent={agent} what={what} onAsk={start} />
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

// How tall the question grows before it scrolls: about sixteen lines.
const MAX_HEIGHT = 320;

/** The question to send with the review, if any: ↵ asks it, Skip opens the session without one. */
function QuestionBox({ agent, what, onAsk }: { agent: string; what: string; onAsk: (question?: string) => void }) {
  const [text, setText] = useState("");
  const field = useRef<HTMLTextAreaElement>(null);
  // Three lines, growing down with what's typed; a scrollbar only once it stops growing.
  useLayoutEffect(() => {
    const el = field.current;
    if (!el) return;
    // Empty: back to its rows; measured before the popover has its width, it came out too tall.
    el.style.height = text ? "auto" : "";
    el.style.overflowY = "";
    if (!text) return;
    // Its border isn't in scrollHeight.
    const full = el.scrollHeight + el.offsetHeight - el.clientHeight;
    el.style.height = `${Math.min(full, MAX_HEIGHT)}px`;
    el.style.overflowY = full > MAX_HEIGHT ? "auto" : "hidden";
  }, [text]);
  return (
    <form
      className="flex flex-col gap-1.5 p-2.5"
      onSubmit={(e) => {
        e.preventDefault();
        if (text.trim()) onAsk(text);
      }}
    >
      <label htmlFor="ask-agent-question" className="text-[12px] font-medium">
        Ask {agent} about {what}
      </label>
      <Textarea
        ref={field}
        id="ask-agent-question"
        autoFocus
        rows={3}
        className="py-1.5"
        value={text}
        onChange={(e) => setText(e.target.value)}
        placeholder="Your question (optional)"
        onKeyDown={(e) => {
          // ↵ asks, ⇧↵ is a new line; not while an input method is composing a word.
          if (e.key !== "Enter" || e.shiftKey || e.nativeEvent.isComposing) return;
          e.preventDefault();
          e.currentTarget.form?.requestSubmit();
        }}
      />
      <div className="flex items-center gap-1.5">
        <span className="text-[11px] text-subtle">↵ asks · ⇧↵ new line</span>
        <Button type="button" size="sm" variant="ghost" className="ml-auto" onClick={() => onAsk()}>
          Skip
        </Button>
        <Button type="submit" size="sm" disabled={!text.trim()}>
          Ask
        </Button>
      </div>
    </form>
  );
}
