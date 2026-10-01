import type * as React from "react";
import { useFieldLabel } from "@/lib/ui/fieldLabel";
import { cn } from "@/lib/utils";

/** The system's own dropdown, styled like Input. */
function Select({ className, ...props }: React.ComponentProps<"select">) {
  return <select aria-labelledby={useFieldLabel()} className={cn("h-7 rounded-md border border-border-strong bg-background px-2 text-[12px] text-foreground outline-none focus:border-primary", className)} {...props} />;
}

export { Select };
