import { X } from "lucide-react";
import { DisabledTip } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { type Choice, type ListKind, scopesFor } from "./narrow";

/**
 * The chips under a list's header: All, then whose work it is (the account's, or the `author` a
 * row's name picked), and for pull requests drafts or those ready. One scope at a time; Draft and
 * Ready toggle. The ones that name the account give `meReason` as why they're off when it can't be read.
 */
export function NarrowBar({
  kind,
  choice,
  onChange,
  author,
  onAuthor,
  meReason,
}: {
  kind: ListKind;
  choice: Choice;
  onChange: (c: Choice) => void;
  author: string | null;
  onAuthor: (login: string | null) => void;
  meReason: string | null;
}) {
  const chip = (label: string, on: boolean, onClick: () => void, off: string | null = null) => {
    const button = (
      <button
        key={label}
        aria-pressed={on}
        disabled={off !== null}
        onClick={onClick}
        className={cn(
          "h-5 shrink-0 rounded-full border px-2 text-[11px] leading-4 whitespace-nowrap",
          on ? "border-primary/50 bg-primary/15 text-foreground" : "border-border text-muted-foreground enabled:hover:bg-hover enabled:focus-visible:bg-hover enabled:hover:text-foreground enabled:focus-visible:text-foreground",
          off !== null && "opacity-50",
        )}
      >
        {label}
      </button>
    );
    return off === null ? (
      button
    ) : (
      <DisabledTip key={label} label={off} disabled>
        {button}
      </DisabledTip>
    );
  };
  return (
    <div role="group" aria-label={kind === "pulls" ? "Narrow pull requests" : "Narrow issues"} className="flex shrink-0 flex-wrap items-center gap-1 border-b border-border px-2 py-1.5">
      {chip("All", choice.scope === null && author === null, () => {
        onChange({ ...choice, scope: null });
        onAuthor(null);
      })}
      {scopesFor(kind).map((s) => chip(s.label, choice.scope === s.id, () => onChange({ ...choice, scope: s.id }), meReason))}
      {author !== null && (
        <span className="inline-flex h-5 max-w-48 shrink-0 items-center gap-1 rounded-full border border-primary/50 bg-primary/15 pr-0.5 pl-2 text-[11px] leading-4 text-foreground">
          <span className="truncate">Author: {author}</span>
          <button
            aria-label={`Remove author ${author}`}
            onClick={() => onAuthor(null)}
            className="flex size-3.5 shrink-0 items-center justify-center rounded-full text-subtle hover:bg-hover focus-visible:bg-hover hover:text-foreground focus-visible:text-foreground"
          >
            <X className="size-2.5" />
          </button>
        </span>
      )}
      {kind === "pulls" && (
        <>
          <span aria-hidden className="mx-0.5 h-3 w-px bg-border" />
          {chip("Draft", choice.draft === true, () => onChange({ ...choice, draft: choice.draft === true ? null : true }))}
          {chip("Ready", choice.draft === false, () => onChange({ ...choice, draft: choice.draft === false ? null : false }))}
        </>
      )}
    </div>
  );
}
