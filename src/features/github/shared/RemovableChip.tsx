import { X } from "lucide-react";

/** A filter a list is narrowed by, with × to take it off; `label` names it for the button. */
export function RemovableChip({ label, onRemove, children }: { label: string; onRemove: () => void; children: React.ReactNode }) {
  return (
    <span className="inline-flex max-w-48 items-center gap-1 rounded-full border border-border-strong bg-active pr-0.5 pl-1.5 text-[10.5px] leading-4">
      {children}
      <button
        aria-label={`Remove ${label}`}
        onClick={onRemove}
        className="flex size-3.5 shrink-0 items-center justify-center rounded-full text-subtle hover:bg-hover focus-visible:bg-hover hover:text-foreground focus-visible:text-foreground"
      >
        <X className="size-2.5" />
      </button>
    </span>
  );
}
