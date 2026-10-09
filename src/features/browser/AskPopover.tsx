import { type ReactNode, useState } from "react";
import { QuestionBox, useAskModel } from "@/components/QuestionBox";
import { Popover, PopoverAnchor, PopoverContent } from "@/components/ui/popover";
import type { BrowserPick } from "@/lib/api";
import { formatPicks, type ShownAs } from "@/lib/browser/format";
import { askAgentAbout } from "@/lib/browser/handoff";
import { reviewAgent } from "@/lib/git/suggest";
import { useSettings } from "@/lib/settings";
import { agentName } from "@/lib/terminal/handoff";

/** What a browser tab hands its worktree's agent: picked elements, or the page's errors. */
export interface AskAbout {
  /** As the question's label names it: "SaveButton <button>", "3 elements", "these errors". */
  what: string;
  /** Shown above the question: what goes along with it. */
  summary: string;
  content: string;
  /** The agent session's name. */
  name: string;
}

const label = (p: BrowserPick) => `${p.components[0] ? `${p.components[0]} ` : ""}<${p.tag}>`;

/** Picked elements as the question names them: one by its components, tag and text, a few by count. */
export function picksAbout(picks: BrowserPick[], device: ShownAs | null): AskAbout {
  const [one] = picks;
  const what = picks.length === 1 ? label(one) : `${picks.length} elements`;
  const text = one.text ? ` "${one.text.length > 40 ? `${one.text.slice(0, 39)}…` : one.text}"` : "";
  const summary = picks.length === 1 ? `${one.components.length ? `${one.components.join(" < ")} · ` : ""}<${one.tag}>${text}` : picks.map(label).join(" · ");
  return { what, summary, content: formatPicks(picks, device), name: `Browser: ${what}` };
}

/** The page's errors (formatErrors), as the question names them. */
export function errorsAbout(errors: string): AskAbout {
  const count = errors.split("\n").filter((l) => l.startsWith("- ")).length;
  return { what: "these errors", summary: `${count} ${count === 1 ? "error" : "errors"} from the page's console`, content: errors, name: "Browser: console errors" };
}

/**
 * The question for the agent, as Ask Agent asks it, beside what it's about (`children`, the
 * anchor): Ask (↵) starts a session with it sent, Skip one with it waiting in the prompt box, Esc or
 * a click elsewhere neither. Meanwhile the page shows as its picture, as under any popover.
 * `pageErrors`: the page's errors the question may take along.
 */
export function AskPopover({
  root,
  about,
  pageErrors,
  onClose,
  children,
}: {
  root: string;
  about: AskAbout | null;
  pageErrors?: { count: number; read: () => Promise<string> } | null;
  onClose: () => void;
  children: ReactNode;
}) {
  const agent = agentName(reviewAgent(useSettings()).command);
  const choice = useAskModel("browser");
  const [include, setInclude] = useState(false);
  const close = () => {
    setInclude(false);
    onClose();
  };
  const ask = async (question = "") => {
    if (!about) return;
    const errors = include && pageErrors ? await pageErrors.read().catch(() => "") : "";
    askAgentAbout(root, errors ? `${about.content}\n\n${errors}` : about.content, about.name, question, choice.model);
    close();
  };
  return (
    <Popover open={!!about} onOpenChange={(open) => !open && close()}>
      <PopoverAnchor asChild>{children}</PopoverAnchor>
      {about && (
        <PopoverContent side="bottom" align="start" className="w-80">
          <div className="truncate px-2.5 pt-2 font-mono text-[11px] text-muted-foreground" title={about.summary}>
            {about.summary}
          </div>
          <QuestionBox
            key={about.content}
            agent={agent}
            what={about.what}
            onAsk={(q) => void ask(q)}
            model={choice.picker}
            extra={
              !!pageErrors?.count && (
                <label className="flex items-center gap-1.5 text-[11.5px] text-muted-foreground">
                  <input type="checkbox" checked={include} onChange={(e) => setInclude(e.target.checked)} />
                  Include {pageErrors.count} page {pageErrors.count === 1 ? "error" : "errors"}
                </label>
              )
            }
          />
        </PopoverContent>
      )}
    </Popover>
  );
}
