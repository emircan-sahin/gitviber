import type * as React from "react";
import { cn } from "@/lib/utils";

type Variant = "default" | "field";

// field: a settings row's control, a size up, with the choice in the primary tint.
const BOX: Record<Variant, string> = {
  default: "flex h-6 w-fit overflow-hidden rounded-md border border-border-strong",
  field: "flex h-7 overflow-hidden rounded-md border border-border-strong",
};

/** One segment's classes, for a control that composes its own segments (a dropdown among them). */
function segmentClass(variant: Variant, on: boolean) {
  return variant === "field"
    ? cn(
        "border-r border-border-strong px-2.5 text-[12px] last:border-r-0",
        on ? "bg-primary/15 text-primary" : "text-muted-foreground hover:bg-hover focus-visible:bg-hover hover:text-foreground focus-visible:text-foreground",
      )
    : cn(
        // The wrapper's overflow-hidden would clip an outer focus ring.
        "flex items-center gap-1 border-l border-border-strong px-2 text-[11.5px] font-medium transition-colors outline-none first:border-l-0 focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset",
        on ? "bg-active text-foreground" : "text-subtle hover:text-foreground focus-visible:text-foreground",
      );
}

function Segmented<T extends string>({
  value,
  onChange,
  options,
  variant = "default",
}: {
  value: T;
  onChange: (v: T) => void;
  options: { value: T; label: string; icon?: React.ComponentType<{ className?: string }> }[];
  variant?: Variant;
}) {
  return (
    <div className={BOX[variant]}>
      {options.map((o) => (
        <button
          key={o.value}
          // Inside a form, a plain button would submit it.
          type="button"
          onClick={() => onChange(o.value)}
          className={segmentClass(variant, value === o.value)}
        >
          {o.icon && <o.icon className="size-3.5" />}
          {o.label}
        </button>
      ))}
    </div>
  );
}

export { Segmented, segmentClass };
