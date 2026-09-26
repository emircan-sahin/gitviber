import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

/** A count beside a tab or a pane's title, as the Changes tab shows it; `active`: its tab is selected or its pane open. */
export function CountBadge({ active, className, children }: { active: boolean; className?: string; children: ReactNode }) {
  return (
    <span className={cn("shrink-0 rounded-sm px-1 font-mono text-[10px] leading-4 tracking-normal", active ? "bg-modified-fill text-on-status" : "bg-elevated text-muted-foreground", className)}>
      {children}
    </span>
  );
}
