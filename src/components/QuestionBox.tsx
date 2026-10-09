import { type ReactNode, useId, useLayoutEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { modelOf, presetOf, reviewAgent } from "@/lib/git/suggest";
import { useSettings } from "@/lib/settings";
import { readJson, writeJson } from "@/lib/storage";

/** The model the agent starts with, as the question's box picks it. */
export interface ModelPick {
  choices: string[];
  value: string;
  onChange: (model: string) => void;
}

// Claude Code's own names for its newest models of each size; Settings takes any id.
const CLAUDE_MODELS = ["opus", "sonnet", "haiku"];

/**
 * The model to ask with, each kind of ask (`review`, `browser`) remembering its own last one, not a
 * setting: Settings → Guided Review's model until one's picked. Only for Claude Code, the one agent
 * that starts with the question in hand; null elsewhere.
 */
export function useAskModel(kind: "review" | "browser"): { picker: ModelPick | null; model: string | undefined } {
  const { command, models } = reviewAgent(useSettings());
  const key = `gitviber.askModel.${kind}`;
  const [last, setLast] = useState(() => readJson(key, "", (v): v is string => typeof v === "string"));
  const preset = presetOf(command);
  if (preset !== "claude") return { picker: null, model: undefined };
  const value = last || modelOf(preset, models);
  const choices = [...new Set([value, modelOf(preset, models), ...CLAUDE_MODELS].filter(Boolean))];
  const onChange = (model: string) => {
    writeJson(key, model);
    setLast(model);
  };
  return { picker: { choices, value, onChange }, model: value || undefined };
}

// How tall the question grows before it scrolls: about sixteen lines.
const MAX_HEIGHT = 320;

/**
 * The question to send with something handed to the agent (a review, a browser tab's pick), if
 * any: ↵ asks it, Skip opens the session without one, Esc cancels where there's `onCancel`.
 */
export function QuestionBox({
  agent,
  what,
  onAsk,
  onCancel,
  model,
  extra,
}: {
  agent: string;
  what: string;
  onAsk: (question?: string) => void;
  onCancel?: () => void;
  model?: ModelPick | null;
  /** Under the question: what else can go with it. */
  extra?: ReactNode;
}) {
  const [text, setText] = useState("");
  const field = useRef<HTMLTextAreaElement>(null);
  const id = useId();
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
      <label htmlFor={id} className="text-[12px] font-medium">
        Ask {agent} about {what}
      </label>
      <Textarea
        ref={field}
        id={id}
        autoFocus
        rows={3}
        className="py-1.5"
        value={text}
        onChange={(e) => setText(e.target.value)}
        placeholder="Your question (optional)"
        onKeyDown={(e) => {
          if (e.key === "Escape" && onCancel) {
            e.preventDefault();
            return onCancel();
          }
          // ↵ asks, ⇧↵ is a new line; not while an input method is composing a word.
          if (e.key !== "Enter" || e.shiftKey || e.nativeEvent.isComposing) return;
          e.preventDefault();
          e.currentTarget.form?.requestSubmit();
        }}
      />
      {extra}
      <div className="flex items-center gap-1.5">
        {model ? (
          <select
            aria-label="Model"
            title="The model it starts with"
            value={model.value}
            onChange={(e) => model.onChange(e.target.value)}
            className="h-7 max-w-28 min-w-0 rounded-md border border-border-strong bg-background px-1.5 text-[11.5px] text-foreground outline-none focus:border-primary"
          >
            {model.choices.map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
          </select>
        ) : (
          <span className="text-[11px] text-subtle">↵ asks · ⇧↵ new line</span>
        )}
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
