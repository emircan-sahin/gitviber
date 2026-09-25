import type * as React from "react";
import { cn } from "@/lib/utils";

function Segmented<T extends string>({
  value,
  onChange,
  options,
}: {
  value: T;
  onChange: (v: T) => void;
  options: { value: T; label: string; icon?: React.ComponentType<{ className?: string }> }[];
}) {
  return (
    <div className="flex h-6 w-fit overflow-hidden rounded-md border border-border-strong">
      {options.map((o, i) => (
        <button
          key={o.value}
          // Inside a form, a plain button would submit it.
          type="button"
          onClick={() => onChange(o.value)}
          className={cn(
            // The wrapper's overflow-hidden would clip an outer focus ring.
            "flex items-center gap-1 px-2 text-[11.5px] font-medium transition-colors outline-none focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset",
            i > 0 && "border-l border-border-strong",
            value === o.value ? "bg-active text-foreground" : "text-subtle hover:text-foreground focus-visible:text-foreground",
          )}
        >
          {o.icon && <o.icon className="size-3.5" />}
          {o.label}
        </button>
      ))}
    </div>
  );
}

export { Segmented };
